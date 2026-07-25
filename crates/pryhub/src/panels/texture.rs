//! The texture tab: what the car is painted with.
//!
//! A grid of thumbnails and one large preview. The thumbnails are downscaled on the CPU and only
//! the selected image is uploaded at full size — a car ships 57–76 textures and several are
//! 512×512, so uploading them all would cost tens of megabytes of GPU memory to show a contact
//! sheet.
//!
//! Every image keeps its own aspect ratio, in the grid as well as in the preview. A contact sheet
//! that squares everything off would show a badge strip as a badge, and the shape of a texture is
//! part of reading it. What is square is the *slot*, which is what makes the sheet a grid.
//!
//! Textures in this format carry no names, only hashes; what names there are come from the
//! `DebugName` the compiler left in the pool, which is why the label falls back to the hash
//! rather than to something invented.

use crate::app::PryHub;
use crate::theme::{self, token};
use egui::{Color32, ColorImage, Rect, RichText, Sense, Stroke, TextureHandle, Vec2};
use gizmo_nfs::{AssetHash, NfsTexture};

/// Thumbnail edge, in points — the design's `minmax(128px, 1fr)` floor, so a tile that is stretched
/// to fill its share of the row is never asked to upscale by much.
const THUMB: usize = 128;

/// The grid's gap, in both axes. Stated rather than taken from `item_spacing`, which carries the
/// density's vertical padding and would make the rows and the columns disagree.
const GAP: f32 = token::SPACE_3;

/// The tab body's own padding. The design gives it 16 on all four sides; the workspace host frame
/// has already spent 6 × 4 of that around this panel.
const PAD: egui::Margin = egui::Margin { left: 10, right: 10, top: 12, bottom: 12 };

/// `padding: 6px 8px` — the caption's inset inside a tile, and the vertical rhythm of the
/// preview's field rows.
const CAP_X: f32 = 8.0;
const CAP_Y: f32 = 6.0;

/// The transparency board behind an image, in the design's own square.
const CHECKER: f32 = 22.0;

/// The whole of a texture, for an image drawn straight through the painter.
const UV: Rect = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));

/// Draw the tab.
pub fn show(app: &mut PryHub, ui: &mut egui::Ui) {
    let t = app.lang.strings();
    if app.doc.is_none() {
        note(ui, t.no_file);
        return;
    }
    // The decode is a job, so this panel may be drawn while it is still running. Saying so beats a
    // frozen window, which is what the lazy version did on first open.
    app.want_textures();
    let Some(tpk) = app.textures.ready().map(std::sync::Arc::clone) else {
        note(ui, if matches!(app.textures, crate::app::Textures::Decoding) { t.decoding } else { t.no_textures });
        return;
    };
    let tpk = tpk.as_ref();

    // A stable order, so the grid does not reshuffle between frames (the table is a HashMap).
    let mut entries: Vec<(&AssetHash, &NfsTexture)> = tpk.textures.iter().collect();
    entries.sort_by(|a, b| a.1.name.cmp(&b.1.name).then(a.0 .0.cmp(&b.0 .0)));
    let undecoded = tpk.entries.len().saturating_sub(tpk.textures.len());

    // The preview opens on the first image rather than empty — the sheet is there to be read, and
    // one image already up says what kind of file this is.
    if app.texture_selection.is_none() {
        app.texture_selection = entries.first().map(|(hash, _)| **hash);
    }
    let selected = app.texture_selection.filter(|h| tpk.texture(*h).is_some());
    let mut pick = None;
    let mut save_one = None;

    egui::Panel::right("texture_preview")
        .resizable(true)
        .default_size(260.0)
        // egui's resize separator is a hairline, and every panel edge in this design is 2 px. One
        // rule, painted below, in the width the design asks for.
        .show_separator_line(false)
        .frame(egui::Frame::new().fill(token::BG).inner_margin(PAD))
        .show_inside(ui, |ui| {
            let area = ui.max_rect();
            let edge = area.left() - f32::from(PAD.left);
            ui.painter().rect_filled(
                Rect::from_min_max(
                    egui::pos2(edge, area.top() - f32::from(PAD.top)),
                    egui::pos2(edge + token::RULE, area.bottom() + f32::from(PAD.bottom)),
                ),
                0.0_f32,
                token::DIVIDER,
            );
            save_one = preview(app, ui, tpk, selected);
        });

    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(token::BG).inner_margin(PAD))
        .show_inside(ui, |ui| {
            pick = sheet(app, ui, &entries, selected, undecoded);
        });

    if let Some(hash) = pick {
        app.texture_selection = Some(hash);
    }
    if let Some(hash) = save_one {
        app.export_now(crate::export::Kind::OneTexture(hash));
    }
}

/// The right-hand pane: the selected image at size, what the file says about it, and the button that
/// writes just that one. Returns the texture to save, if it was pressed.
///
/// The design has no preview pane, so this is set as its *inspector* — the one place the system
/// defines a titled key/value column (design:236-249): a kicker, the name, the numbers that
/// identify it, then one block per field.
fn preview(
    app: &mut PryHub,
    ui: &mut egui::Ui,
    tpk: &gizmo_nfs::Tpk,
    selected: Option<AssetHash>,
) -> Option<AssetHash> {
    let t = app.lang.strings();
    let mut save_one = None;
    // A panel only keeps the width it was given if its contents claim it; an image and a few short
    // rows do not, and the panel would collapse to its 96 px minimum.
    ui.set_min_width(ui.max_rect().width());
    let Some(tex) = selected.and_then(|h| tpk.texture(h)) else {
        ui.label(RichText::new(t.pick_texture).color(theme::muted(50)));
        return None;
    };
    // Every gap in this pane is one the design states; egui's row spacing would add its own on top
    // of each of them.
    ui.spacing_mut().item_spacing.y = 0.0;

    let handle = upload(ui.ctx(), &mut app.texture_cache, tex, None);
    // Fill the pane, upscaling a small image rather than leaving it postage-stamp sized —
    // the size it is drawn at is stated underneath, and the point of a preview is to see
    // the texels. The fitted size is computed here so no `ImageFit` rule can distort it.
    let side = ui.available_width();
    let natural = egui::vec2(tex.width.max(1) as f32, tex.height.max(1) as f32);
    let fitted = natural * (side / natural.x.max(natural.y));
    let (image, _) = ui.allocate_exact_size(fitted, Sense::hover());
    theme::draw::checker(ui.painter(), image, CHECKER);
    ui.painter().image(handle.id(), image, UV, Color32::WHITE);

    ui.add_space(token::SPACE_3);
    // The kind of thing first, in the accent — the design's header names what it is before it
    // names which one. `tab_tex` is the interface's own word for it, in either language.
    ui.add(egui::Label::new(theme::tracked(
        t.tab_tex,
        0.12,
        theme::font::heading(9.0),
        token::ACCENT,
    )));
    ui.add_space(3.0);
    // A name from the dictionary wins over the file's own: the file's is truncated, and
    // the dictionary's has been checked against the hash.
    let shown = app.names.get(tex.hash.0).map(str::to_string).unwrap_or_else(|| label_of(tex));
    ui.label(RichText::new(shown).font(theme::font::mono(15.0)).color(token::TEXT));
    ui.add_space(3.0);
    ui.label(
        RichText::new(format!("{:#010x} · {} × {}", tex.hash.0, tex.width, tex.height))
            .font(theme::font::mono(10.5))
            .color(theme::muted(50)),
    );
    ui.add_space(10.0);
    rule(ui, token::RULE, token::DIVIDER);
    ui.add_space(CAP_Y);

    for (k, v) in [
        (t.tx_hash, format!("{:#010x}", tex.hash.0)),
        (t.tx_size, format!("{} × {}", tex.width, tex.height)),
        (t.tx_format, format!("{:?}", tex.source_format)),
        (t.tx_opaque, format!("{}%", tex.opaque_permille() / 10)),
    ] {
        field(ui, k, &v);
    }

    ui.add_space(token::SPACE_3);
    // Just this one image. The toolbar's export writes the whole pack; someone who came
    // for a single texture should not have to take 73. "PNG" is the format's name rather than a
    // word, which is why it is not in the string table.
    if crate::widget::action(ui, "PNG", true, theme::icon::export)
        .on_hover_text(t.export_hint)
        .clicked()
    {
        save_one = Some(tex.hash);
    }
    save_one
}

/// One field of the preview: the label on its own line, the value under it in mono, and the soft
/// rule that closes the row (design:243-249).
fn field(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.label(RichText::new(label).size(11.5).color(theme::muted(60)));
    ui.add_space(2.0);
    ui.label(RichText::new(value).font(theme::font::mono(13.0)).color(token::TEXT));
    ui.add_space(CAP_Y);
    // `color-mix(divider 60%, transparent)`: the rule between two rows of one table, not the one
    // that ends a region.
    rule(ui, token::HAIRLINE, token::DIVIDER.gamma_multiply(0.6));
    ui.add_space(CAP_Y);
}

/// A rule across the whole pane, reaching past its padding — in the design the padding is inside
/// the field block, so the line under it runs the full width of the column.
fn rule(ui: &mut egui::Ui, width: f32, colour: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), width), Sense::hover());
    theme::draw::rule_h(
        ui.painter(),
        (rect.left() - f32::from(PAD.left))..=(rect.right() + f32::from(PAD.right)),
        rect.top(),
        width,
        colour,
    );
}

/// The contact sheet: every decoded texture as a thumbnail, and the count of the ones that could
/// not be read. Returns the texture that was clicked.
fn sheet(
    app: &mut PryHub,
    ui: &mut egui::Ui,
    entries: &[(&AssetHash, &NfsTexture)],
    selected: Option<AssetHash>,
    undecoded: usize,
) -> Option<AssetHash> {
    let t = app.lang.strings();
    let mut pick = None;
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("{} {}", entries.len(), t.textures_count.of(entries.len())))
                .font(theme::font::mono(10.5))
                .color(theme::muted(48)),
        );
        if undecoded > 0 {
            // Said out loud: a contact sheet that quietly omits what it could not read
            // would have the user believe the car has fewer textures than it does.
            ui.label(
                RichText::new(format!("· {undecoded} {}", t.textures_undecoded))
                    .font(theme::font::mono(10.5))
                    .color(token::ACCENT_2),
            );
        }
    });
    ui.add_space(token::SPACE_4);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        // `auto-fill` with `1fr`: as many ≥128 px columns as fit, then every column widened to
        // share out what is left over, so the last tile in a row ends flush with the right edge
        // instead of leaving the ragged margin a wrapped run of fixed-width cells does.
        let avail = ui.available_width();
        let cols = ((avail + GAP) / (THUMB as f32 + GAP)).floor().max(1.0);
        let width = ((avail - GAP * (cols - 1.0)) / cols).max(24.0);
        let cols = cols as usize;
        ui.spacing_mut().item_spacing = Vec2::splat(GAP);
        for row in entries.chunks(cols) {
            ui.horizontal(|ui| {
                for (hash, tex) in row {
                    let handle = upload(ui.ctx(), &mut app.texture_cache, tex, Some(THUMB));
                    let label = app.names.get(hash.0).map(str::to_string);
                    let label = label.unwrap_or_else(|| label_of(tex));
                    let resp =
                        cell(ui, &handle, &label, &meta_of(tex), width, selected == Some(**hash));
                    if resp.clicked() {
                        pick = Some(**hash);
                    }
                    resp.on_hover_text(format!(
                        "{}\n{} × {} · {:?}",
                        label_of(tex),
                        tex.width,
                        tex.height,
                        tex.source_format
                    ));
                }
            });
        }
    });
    pick
}

/// One tile of the contact sheet: a square image slot flush to the tile's border, and the two-line
/// mono caption inset under it. The slot is square so the grid stays a grid; the image inside it is
/// not, so a strip still reads as a strip.
///
/// Drawn by hand rather than assembled from a button and a label: the design's tile is *one* box —
/// one border, one fill, one click — and a framed button with a caption underneath it is two.
fn cell(
    ui: &mut egui::Ui,
    handle: &TextureHandle,
    name: &str,
    meta: &str,
    width: f32,
    on: bool,
) -> egui::Response {
    let ink = if on { token::ACCENT_800 } else { token::TEXT };
    let inner = (width - CAP_X * 2.0).max(1.0);
    let name_line = elided(ui, name, theme::font::mono(10.5), ink, inner);
    let meta_line = elided(ui, meta, theme::font::mono(9.5), theme::muted(50), inner);
    let (name_h, meta_h) = (name_line.size().y, meta_line.size().y);
    let height = width + CAP_Y * 2.0 + name_h + meta_h;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());

    let slot = Rect::from_min_size(rect.left_top(), Vec2::splat(width));
    // The image fades up the first time this cell is drawn. 73 thumbnails appearing at once
    // is a flash; the same 73 arriving over a tenth of a second reads as the sheet filling.
    let seen =
        ui.ctx().animate_bool_with_time(ui.id().with(("thumb", name)), true, crate::chrome::MOVE_TIME);
    let p = ui.painter();
    p.rect_filled(rect, 0.0_f32, if on { token::ACCENT_100 } else { token::SURFACE });
    // Alpha has to read as alpha: on a flat fill a `_MASK` looks like a texture that happens to be
    // pale, and a car's pack is half masks.
    theme::draw::checker(p, slot, CHECKER);
    let size = handle.size_vec2();
    let fit = (width / size.x.max(1.0)).min(width / size.y.max(1.0));
    p.image(
        handle.id(),
        Rect::from_center_size(slot.center(), size * fit),
        UV,
        Color32::WHITE.gamma_multiply(seen),
    );
    p.galley(egui::pos2(rect.left() + CAP_X, slot.bottom() + CAP_Y), name_line, ink);
    p.galley(
        egui::pos2(rect.left() + CAP_X, slot.bottom() + CAP_Y + name_h),
        meta_line,
        theme::muted(50),
    );
    if resp.hovered() {
        p.rect_filled(rect, 0.0_f32, token::WASH_HOVER);
    }
    p.rect_stroke(
        rect,
        0.0_f32,
        Stroke::new(token::HAIRLINE, if on { token::ACCENT } else { token::DIVIDER }),
        egui::StrokeKind::Inside,
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// One caption line, cut with an ellipsis rather than wrapped: a tile is a fixed width and a
/// texture's name is not.
fn elided(
    ui: &egui::Ui,
    text: &str,
    font: egui::FontId,
    colour: Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, colour);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width);
    ui.painter().layout_job(job)
}

/// Upload a texture (or its thumbnail) once and keep the handle.
fn upload(
    ctx: &egui::Context,
    cache: &mut std::collections::HashMap<(u32, bool), TextureHandle>,
    tex: &NfsTexture,
    thumb: Option<usize>,
) -> TextureHandle {
    let key = (tex.hash.0, thumb.is_some());
    if let Some(handle) = cache.get(&key) {
        return handle.clone();
    }
    let image = match thumb {
        Some(side) => downscale(tex, side),
        None => {
            let size = [tex.width as usize, tex.height as usize];
            ColorImage::from_rgba_unmultiplied(size, &tex.rgba)
        }
    };
    // Nearest at full size: the preview upscales, and a filter would smear exactly the texel edges
    // someone opens a texture to look at. Thumbnails are already downscaled to their drawn size.
    let options =
        if thumb.is_some() { egui::TextureOptions::LINEAR } else { egui::TextureOptions::NEAREST };
    let name = format!("tex{:08x}{}", tex.hash.0, u8::from(thumb.is_some()));
    let handle = ctx.load_texture(name, image, options);
    cache.insert(key, handle.clone());
    handle
}

/// Nearest-neighbour downscale into a `side`-bounded box, keeping the aspect ratio. Nearest rather
/// than a filter on purpose: these are texture atlases, and a blurred one hides the seams you are
/// looking for.
fn downscale(tex: &NfsTexture, side: usize) -> ColorImage {
    let (w, h) = (tex.width as usize, tex.height as usize);
    if w == 0 || h == 0 {
        return ColorImage::from_rgba_unmultiplied([1, 1], &[0, 0, 0, 0]);
    }
    let (tw, th) = if w >= h {
        (side.min(w), (side * h / w).clamp(1, h))
    } else {
        ((side * w / h).clamp(1, w), side.min(h))
    };
    let mut out = Vec::with_capacity(tw * th * 4);
    for y in 0..th {
        let sy = y * h / th;
        for x in 0..tw {
            let sx = x * w / tw;
            let i = (sy * w + sx) * 4;
            out.extend_from_slice(tex.rgba.get(i..i + 4).unwrap_or(&[0, 0, 0, 0]));
        }
    }
    ColorImage::from_rgba_unmultiplied([tw, th], &out)
}

/// A texture's label: its `DebugName` when the compiler left one, else its hash.
fn label_of(tex: &NfsTexture) -> String {
    if tex.name.is_empty() {
        format!("{:#010x}", tex.hash.0)
    } else {
        tex.name.clone()
    }
}

/// The caption's second line: what the file stored this image as, and how big it is. Square is the
/// common case and the design writes it as one number; anything else states both edges rather than
/// pretending the shape does not matter.
fn meta_of(tex: &NfsTexture) -> String {
    let fmt = format!("{:?}", tex.source_format).to_uppercase();
    if tex.width == tex.height {
        format!("{fmt} · {}²", tex.width)
    } else {
        format!("{fmt} · {} × {}", tex.width, tex.height)
    }
}

/// The tab with nothing to show: the design's accent-edged strip at the top left, not a message
/// floating in the middle of an empty pane.
fn note(ui: &mut egui::Ui, message: &str) {
    egui::Frame::new().inner_margin(PAD).show(ui, |ui| {
        crate::widget::note_box(ui, message);
    });
}
