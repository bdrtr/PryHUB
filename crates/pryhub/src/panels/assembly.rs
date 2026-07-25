//! Montaj sekmesi: hangi parçalar takılı, ve onların oluşturduğu araç.
//!
//! Liste `gizmo_nfs::select_stock_car`'ın seçtiği parçalar — yani "standart" yapı — ve her satır
//! onlardan birini söküp takıyor. Parça *seçimi* kütüphanenin işi (`parts::select_car`), bu ekran
//! yalnızca o seçimin üstünden bir şey çıkarmayı sunuyor: hangi bileşenin hangi namespace'ten
//! geleceğine karar veren mantık burada ikinci kez yazılmıyor.
//!
//! Tasarım `MIRROR ×2` gibi satırlar gösteriyor. Buradaki ×N uydurma değil: yalnızca adının sonu
//! `_LEFT`/`_RIGHT` olan parçalar tek satırda toplanıyor, ki onlar gerçekten iki ayrı parça.
//! Tekerlek için ×4 *yazılmıyor* — bir `GEOMETRY.BIN` tek tekerlek ağı taşıyor ve dört köşeye
//! yerleştirme bilgisi `GLOBALB.BUN`'da; dosyada olmayan bir şeyi saymak bu aracın işi değil.

use crate::app::PryHub;
use crate::theme::{self, token};
use crate::widget;
use egui::{Align2, Pos2, RichText, Sense, Vec2};
use gizmo_nfs::NfsMeshPart;

/// Tasarımın parça panelinin genişliği.
pub const PANEL_W: f32 = 214.0;
/// Bir parça satırı: iki satır metin ve nefes payı.
const ROW_H: f32 = 47.0;
const PAD_X: f32 = 10.0;
/// Onay kutusu.
const BOX: f32 = 11.0;
/// Ayağın kapladığı yer: toplam satırı, iki düğme, ipucu ve kendi iç boşlukları.
const FOOTER_H: f32 = 104.0;
/// Boya satırının payı: başlık şeridi ve üç sıra swatch. 123 renk buraya sığmaz, o yüzden kendi
/// içinde kayar — panelin tamamını yutmasındansa üç sıra gösterip kaydırmak.
const PAINT_H: f32 = 104.0;

/// Listede tek bir satır: bir ya da iki parça, ortak bir adla.
pub struct Row {
    /// Hem etiket hem de açma/kapama anahtarı.
    pub key: String,
    /// Kaç parça bu satırın altında (`×2` bundan geliyor).
    pub count: usize,
    pub triangles: usize,
    /// Parçanın geldiği chunk — tasarım bunu satırın altında gösteriyor.
    pub offset: Option<usize>,
    /// Gövde sökülemez: altında araç kalmaz.
    pub base: bool,
}

/// Adın sonundaki taraf belirteci, varsa.
fn side_stripped(component: &str) -> &str {
    for suffix in ["_LEFT", "_RIGHT"] {
        if let Some(stem) = component.strip_suffix(suffix) {
            return stem;
        }
    }
    component
}

/// Satırın etiketi: aracın ve namespace'in adı atılmış bileşen adı.
///
/// `PEUGOT_KIT00_HOOD_A` → `HOOD`. Baştaki iki belirteç her satırda aynı olduğu için taşınmaları
/// listeyi okunmaz yapıyor; tasarım da onları taşımıyor.
fn label_of(name: &str) -> String {
    let component = side_stripped(gizmo_nfs::parts::component_key(name));
    let mut parts = component.splitn(3, '_');
    let (_car, ns, rest) = (parts.next(), parts.next(), parts.next());
    match (ns, rest) {
        // İkinci belirteç bir namespace ise (BASE/KIT00/STYLE03) at, kalanı etiket yap.
        (Some(ns), Some(rest))
            if ns == "BASE" || ns.starts_with("KIT") || ns.starts_with("STYLE") =>
        {
            rest.to_owned()
        }
        _ => component.to_owned(),
    }
}

/// Standart yapının satırları, dosya sırasında.
#[must_use]
pub fn rows(parts: &[NfsMeshPart]) -> Vec<Row> {
    let mut out: Vec<Row> = Vec::new();
    for part in gizmo_nfs::select_stock_car(parts) {
        let key = label_of(&part.name);
        let base = part.name.contains("_BODY") || key == "BODY";
        match out.iter_mut().find(|r| r.key == key) {
            Some(row) => {
                row.count += 1;
                row.triangles += part.triangle_count();
            }
            None => out.push(Row {
                key,
                count: 1,
                triangles: part.triangle_count(),
                offset: None,
                base,
            }),
        }
    }
    out
}

/// Şu an takılı olan parçalar, `parts` içindeki **indeksleri** olarak.
///
/// Referans değil indeks, çünkü çağıran bunu renderer'ı `&mut` ödünç almadan *önce* hesaplamak
/// zorunda: ikisi de aynı `PryHub`'ın alanları ve bir referans o ödüncün içine kadar yaşardı.
/// İndeks düz veri; kimseden ödünç almıyor.
#[must_use]
pub fn mounted_indices(app: &PryHub, parts: &[NfsMeshPart]) -> Vec<usize> {
    let wanted = gizmo_nfs::select_stock_car(parts);
    parts
        .iter()
        .enumerate()
        .filter(|(_, p)| {
            wanted.iter().any(|w| std::ptr::eq(*w, *p))
                && !app.unmounted.contains(&label_of(&p.name))
        })
        .map(|(i, _)| i)
        .collect()
}

/// Takılı kümeyi bir sayıya indir: görüntü bunu anahtar olarak kullanıp değiştiğinde yeniden
/// yüklüyor. Sıralı ve kararlı, çünkü `rows` dosya sırasını koruyor.
#[must_use]
pub fn build_key(app: &PryHub, parts: &[NfsMeshPart]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for part in gizmo_nfs::select_stock_car(parts) {
        let key = label_of(&part.name);
        let on = !app.unmounted.contains(&key);
        key.hash(&mut h);
        on.hash(&mut h);
    }
    h.finish()
}

/// Parça panelini çiz. Görüntünün kendisi [`super::viewport3d`], bu onun sağındaki liste.
pub fn panel(app: &mut PryHub, ui: &mut egui::Ui) {
    let t = app.lang.strings();
    let Some(doc) = app.doc.clone() else { return };
    let list = rows(&doc.parts);
    let on = list.iter().filter(|r| !app.unmounted.contains(&r.key)).count();
    let total: usize =
        list.iter().filter(|r| !app.unmounted.contains(&r.key)).map(|r| r.triangles).sum();

    widget::caption_strip(ui, t.asm_mounted, |ui| {
        ui.label(
            RichText::new(format!("{on} / {}", list.len()))
                .font(theme::font::mono(10.5))
                .color(theme::muted(55)),
        );
    });

    // Liste, ayağa yer bırakacak kadar: `auto_shrink([false, false])` kalan yüksekliğin hepsini
    // alıyor ve toplam ile iki düğme panelin altından taşıyordu — bu ekranda üç kez düzeltilen
    // aynı hata.
    // Üç bölümün de yeri baştan ayrılıyor: liste, boya, ayak. Biri "kalanın hepsi" derse diğer
    // ikisi panelin altından taşıyor — bu ekranda üç kez düzeltilen hata.
    let paint_h = if app.palette.as_ref().is_some_and(|p| !p.is_empty()) { PAINT_H } else { 0.0 };
    let list_h = (ui.available_height() - FOOTER_H - paint_h).max(ROW_H);
    let mut toggle = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).max_height(list_h).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for row in &list {
            if part_row(app, ui, row) {
                toggle = Some(row.key.clone());
            }
        }
    });
    if let Some(key) = toggle {
        if !app.unmounted.remove(&key) {
            app.unmounted.insert(key);
        }
    }

    paint_row(app, ui);

    // Ayak: toplam, iki düğme, ve ne yaptığını söyleyen satır.
    egui::Frame::new().inner_margin(egui::Margin::symmetric(10, 9)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(t.asm_total).font(theme::font::body(11.0)).color(theme::muted(55)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(format!(
                        "{} {}",
                        super::viewport3d::grouped(total as u32),
                        t.asm_tri.of(total)
                    ))
                    .font(theme::font::mono(11.5))
                    .color(token::TEXT),
                );
            });
        });
        ui.add_space(token::SPACE_2);
        ui.horizontal(|ui| {
            let half = (ui.available_width() - token::SPACE_2) * 0.5;
            if ui.add_sized([half, 26.0], egui::Button::new(strong(t.asm_all))).clicked() {
                app.unmounted.clear();
            }
            ui.add_space(token::SPACE_2);
            // "Standart" da hepsini takar: bu liste zaten standart yapının kendisi, ve bir gün
            // başka bir yapılandırma seçilebildiğinde ayrılacakları yer burası.
            if ui.add_sized([half, 26.0], egui::Button::new(strong(t.asm_stock))).clicked() {
                app.unmounted.clear();
            }
        });
        ui.add_space(token::SPACE_1);
        ui.label(RichText::new(t.asm_hint).font(theme::font::body(10.0)).color(theme::muted(45)));
    });
}

/// Düğme yazısı: gövde yüzü bir düğmeye ait değil, başlık yüzü ait.
fn strong(text: &str) -> RichText {
    RichText::new(text).font(theme::font::heading(11.0)).color(token::TEXT)
}

/// Bir parça satırı. Tıklandıysa `true`.
fn part_row(app: &PryHub, ui: &mut egui::Ui, row: &Row) -> bool {
    let on = !app.unmounted.contains(&row.key);
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, ROW_H), Sense::click());
    let p = ui.painter();
    if resp.hovered() && !row.base {
        p.rect_filled(rect, 0.0_f32, token::WASH_HOVER);
    }

    // Onay kutusu: dolu kare vurgu renginde, boş kare yalnızca çizgi. Çizilerek, çünkü egui'nin
    // kendi onay kutusu bu sistemin hiçbir ölçüsünü paylaşmıyor.
    let box_rect = egui::Rect::from_center_size(
        Pos2::new(rect.left() + PAD_X + BOX * 0.5, rect.center().y),
        Vec2::splat(BOX),
    );
    if on {
        p.rect_filled(box_rect, 0.0_f32, if row.base { theme::muted(35) } else { token::ACCENT });
    } else {
        p.rect_stroke(
            box_rect,
            0.0_f32,
            egui::Stroke::new(token::HAIRLINE, theme::muted(40)),
            egui::StrokeKind::Inside,
        );
    }

    let x = box_rect.right() + 8.0;
    let ink = if on { token::TEXT } else { theme::muted(45) };
    let name = if row.count > 1 {
        format!("{}  ×{}", row.key, row.count)
    } else {
        row.key.clone()
    };
    // A part name is as long as NFSU2's name field allows, which is longer than this column: cut it
    // rather than let it run under the tag and out of the panel.
    let tag_room = if row.base { 34.0 } else { 0.0 };
    let font = theme::font::heading(11.0);
    let name = super::tree::elide(ui, &name, &font, rect.right() - PAD_X - tag_room - x);
    let p = ui.painter();
    p.text(Pos2::new(x, rect.center().y - 8.0), Align2::LEFT_CENTER, name, font, ink);
    let meta = match row.offset {
        Some(o) => format!("{} tri · {o:#010x}", super::viewport3d::grouped(row.triangles as u32)),
        None => format!("{} tri", super::viewport3d::grouped(row.triangles as u32)),
    };
    p.text(
        Pos2::new(x, rect.center().y + 9.0),
        Align2::LEFT_CENTER,
        meta,
        theme::font::mono(9.5),
        theme::muted(if on { 50 } else { 35 }),
    );

    if row.base {
        let t = app.lang.strings();
        let tag = p.layout_no_wrap(
            t.asm_base.to_owned(),
            theme::font::body(10.0),
            theme::muted(45),
        );
        p.galley(
            Pos2::new(rect.right() - PAD_X - tag.size().x, rect.center().y - tag.size().y * 0.5),
            tag,
            theme::muted(45),
        );
    }

    theme::draw::rule_h(
        p,
        rect.left()..=rect.right(),
        rect.bottom() - token::HAIRLINE,
        token::HAIRLINE,
        token::DIVIDER_ON_SURFACE,
    );

    // Gövde sökülemez: tıklaması da bir şey yapmasın, ki cevapsız kalan bir tıklama olmasın.
    if row.base {
        return false;
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// Boya satırı: oyunun kendi paleti, `GLOBALB.BUN`'dan.
///
/// Renk `GEOMETRY.BIN`'de yok — NFSU2 gövdeyi dokuyla kaplamıyor, oyunda seçilen renkle boyuyor. Bu
/// satır o seçimi buraya taşıyor, ve gösterdiği renkler uydurma değil: `CarParts`'ın `RED`/`GREEN`/
/// `BLUE` özniteliklerinden okunan 123 rengin ta kendisi.
fn paint_row(app: &mut PryHub, ui: &mut egui::Ui) {
    const SW: f32 = 15.0;
    let t = app.lang.strings();
    // Sorulmadan gelmez: 8 MB'lık bir bundle, yalnızca bu sekme açıkken okunmaya değer.
    app.want_palette();
    let Some(palette) = app.palette.clone() else { return };
    if palette.is_empty() {
        return;
    }

    widget::caption_strip(ui, t.asm_paint, |ui| {
        // Seçimi geri almanın yolu, seçmenin yanında dursun.
        if !app.paint.is_none() {
            let resp = ui.add(
                egui::Label::new(
                    RichText::new(t.asm_paint_stock)
                        .font(theme::font::body(10.5))
                        .color(token::ACCENT),
                )
                .sense(Sense::click()),
            );
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                app.paint = None;
            }
        }
    });

    let mut picked = None;
    egui::Frame::new().inner_margin(egui::Margin::symmetric(10, 8)).show(ui, |ui| {
        ui.spacing_mut().item_spacing = Vec2::splat(4.0);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .max_height(PAINT_H - 40.0)
            .show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            for c in &palette {
                let (rect, resp) = ui.allocate_exact_size(Vec2::splat(SW), Sense::click());
                let p = ui.painter();
                p.rect_filled(rect, 0.0_f32, egui::Color32::from_rgb(c.red, c.green, c.blue));
                // Seçili olan vurgu çerçevesini alıyor; üzerinde durulan, ince bir çizgi.
                let edge = if app.paint == Some(*c) {
                    Some((token::RULE, token::ACCENT))
                } else if resp.hovered() {
                    Some((token::HAIRLINE, token::TEXT))
                } else {
                    None
                };
                if let Some((w, ink)) = edge {
                    p.rect_stroke(rect, 0.0_f32, egui::Stroke::new(w, ink), egui::StrokeKind::Outside);
                }
                if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    picked = Some(*c);
                }
            }
        });
            });
    });
    if let Some(c) = picked {
        // İkinci tık seçimi kaldırıyor: bir renge basıp vazgeçmenin yolu aynı yer olsun.
        app.paint = (app.paint != Some(c)).then_some(c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_car_and_the_namespace_come_off_the_label() {
        assert_eq!(label_of("PEUGOT_KIT00_HOOD_A"), "HOOD");
        assert_eq!(label_of("240SX_BASE_TRUNK_A"), "TRUNK");
        assert_eq!(label_of("240SX_STYLE03_HEADLIGHT_B"), "HEADLIGHT");
    }

    /// The two sides of one component are one row, so it can be taken off as the pair it is.
    #[test]
    fn left_and_right_share_a_label() {
        assert_eq!(label_of("240SX_BASE_MIRROR_LEFT"), label_of("240SX_BASE_MIRROR_RIGHT"));
        assert_eq!(label_of("240SX_BASE_MIRROR_LEFT"), "MIRROR");
    }

    /// A truncated name keeps whatever survived rather than being guessed at — the tail is gone and
    /// no amount of splitting brings it back.
    #[test]
    fn a_truncated_name_keeps_what_is_left_of_it() {
        assert_eq!(label_of("240SX_BASE_SIDE_MIRRO"), "SIDE_MIRRO");
    }
}
