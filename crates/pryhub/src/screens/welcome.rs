//! The welcome screen: open something, or read what the tool is for.
//!
//! The design lays this out as a 940 px column with a `1.1fr / 1fr` grid under the wordmark: what
//! you can open on the left, what the tool can do with it on the right. The vertical rhythm is
//! written out in full below rather than left to the shared item spacing, because the design gives
//! this screen its own (16 here, 34 there, 26 before RECENT) and nothing else reuses it.

use crate::app::{PryHub, Screen};
use crate::theme::{self, token};
use crate::widget;
use egui::{Align, Color32, Layout, Pos2, RichText, Sense, Vec2};

/// `max-width: 940px` under the design's `box-sizing: border-box`, so the column *inside* the
/// 40 px of side padding measures 860 — which is what [`widget::page`] caps (it pads by its own 34).
/// The design's `max-width:940px; padding:56px 40px 40px` — the one page with its own measures.
const PAGE_WIDTH: f32 = 940.0;
const PAGE_PAD: egui::Margin = egui::Margin { left: 40, right: 40, top: 56, bottom: 40 };
/// `padding-top: 56px` — [`widget::page`] already carries the 30 every other screen starts at.
const EXTRA_TOP: f32 = 26.0;
/// The mode grid's `gap: 10px`, both ways.
const GRID_GAP: f32 = 10.0;
/// `.card`'s `padding: var(--space-3)`, which [`widget::card`] applies on all four sides.
const CARD_PAD: f32 = 12.0;
/// The accent glyph at the top of a mode card.
const CARD_ICON: f32 = 22.0;

/// A drawn icon, the shape [`theme::icon`] hands round.
type Glyph = fn(&egui::Painter, Pos2, f32, Color32);

pub fn show(app: &mut PryHub, ui: &mut egui::Ui) {
    let t = app.lang.strings();
    // Copied out of the app before the closures below take it mutably.
    let mark = app.logo.as_ref().map(crate::logo::Logo::mark);
    let mut open: Option<std::path::PathBuf> = None;
    let mut goto: Option<Screen> = None;

    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(token::BG))
        .show_inside(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                widget::page(ui, PAGE_WIDTH, PAGE_PAD, |ui| {
                    // Every gap on this screen is stated; the shared 8 px would land on top of it.
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.add_space(EXTRA_TOP);
                    header(ui, t.brand_sub, mark);

                    ui.add_space(token::SPACE_4);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::splat(token::SPACE_2);
                        for (text, accent) in [
                            ("Linux-native", false),
                            (t.w_open_source, false),
                            (t.w_validates, true),
                            (t.w_discovers, true),
                        ] {
                            widget::tag(
                                ui,
                                text,
                                if accent { widget::Tone::Accent } else { widget::Tone::Outline },
                            );
                        }
                    });
                    ui.add_space(34.0);

                    // `grid-template-columns: 1.1fr 1fr; gap: 32px`.
                    let gap = token::SPACE_8;
                    let total = ui.available_width();
                    let left = ((total - gap) * (1.1 / 2.1)).floor().max(0.0);
                    let right = (total - gap - left).max(0.0);
                    let height = ui.available_height();
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.allocate_ui_with_layout(
                            Vec2::new(left, height),
                            Layout::top_down(Align::Min),
                            |ui| open_column(app, ui, &mut open),
                        );
                        ui.add_space(gap);
                        ui.allocate_ui_with_layout(
                            Vec2::new(right, height),
                            Layout::top_down(Align::Min),
                            |ui| goto = modes_column(app, ui),
                        );
                    });
                });
            });
        });

    if let Some(screen) = goto {
        app.screen = screen;
    }
    if let Some(path) = open {
        app.open(&path);
    }
}

/// The brand row: the mark, the wordmark, the two-line note beside it, and the 2 px rule that
/// closes it.
fn header(ui: &mut egui::Ui, brand_sub: &str, mark: Option<crate::logo::Mark>) {
    let brand = ui
        .painter()
        .layout_job(theme::tracked("PryHUB", -0.03, theme::font::heading(68.0), token::TEXT));
    let sub = ui.painter().layout_no_wrap(
        format!("{brand_sub}\nv0.1 · gizmo-nfs"),
        theme::font::body(13.0),
        theme::muted(60),
    );

    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, brand.size().y), Sense::hover());

    // The mark leads the wordmark, centred on the line box and set well under it: at the wordmark's
    // own height it would be a picture standing next to a word rather than one lockup.
    const MARK_H: f32 = 52.0;
    let lead = mark.map_or(0.0, |m| m.width_for(MARK_H) + token::SPACE_3);

    // The design aligns the note on the wordmark's *baseline*, 6 px above it — `align-items:
    // flex-end` over a `.85` line box. egui reports neither ascent nor baseline, so it comes from
    // the face's own metrics: Archivo puts 80.7 % of a line box above the baseline.
    const BASELINE: f32 = 0.807;
    let line = sub.size().y * 0.5;
    let sub_pos = Pos2::new(
        rect.left() + lead + brand.size().x + token::SPACE_4,
        rect.top() + brand.size().y * BASELINE - 6.0 - line * (1.0 + BASELINE),
    );
    let p = ui.painter();
    if let Some(m) = mark {
        let top = rect.top() + (brand.size().y - MARK_H) * 0.5;
        m.paint(
            p,
            egui::Rect::from_min_size(
                Pos2::new(rect.left(), top),
                Vec2::new(m.width_for(MARK_H), MARK_H),
            ),
        );
    }
    p.galley(sub_pos, sub, theme::muted(60));
    p.galley(Pos2::new(rect.left() + lead, rect.top()), brand, token::TEXT);

    ui.add_space(20.0);
    theme::draw::rule_h(
        ui.painter(),
        rect.left()..=rect.right(),
        ui.cursor().top(),
        token::RULE,
        token::DIVIDER,
    );
    ui.add_space(token::RULE);
}

/// The left column: the drop target, and what has been opened before.
fn open_column(app: &mut PryHub, ui: &mut egui::Ui, open: &mut Option<std::path::PathBuf>) {
    let t = app.lang.strings();
    // The page zeroed the vertical gap; the horizontal one is still the design's.
    ui.spacing_mut().item_spacing = Vec2::new(token::SPACE_2, 0.0);

    // Not in the design — the mock never failed to open anything — but it belongs to the affordance
    // it is about, so it sits with it in this column rather than across the page.
    if let Some(err) = &app.error {
        ui.label(
            RichText::new(format!("{} — {err}", t.open_failed))
                .font(theme::font::body(12.0))
                .color(token::ACCENT),
        );
        ui.add_space(token::SPACE_3);
    }

    widget::section_label(ui, t.w_open_file);
    ui.add_space(token::SPACE_3);
    drop_zone(app, ui, open);

    if !app.recents.is_empty() {
        ui.add_space(26.0);
        widget::section_label(ui, t.w_recent);
        ui.add_space(10.0);
        let current = app.doc.as_ref().map(|doc| doc.path.clone());
        for path in app.recents.clone() {
            if recent_row(ui, &path, current.as_deref() == Some(path.as_path())) {
                *open = Some(path);
            }
        }
    }
}

/// The design's drop target: `2px dashed`, a surface fill, and both going accent under the pointer.
///
/// It says "drop a file **or click**", so it has to answer a click. There is no file-dialog crate in
/// this binary and there is not going to be one (`Cargo.toml` says why), so the click runs whatever
/// chooser the desktop already has — see [`crate::picker`]. On a machine with none, it puts the
/// caret in the path field instead, which is then the only way in and should at least be pointed at.
fn drop_zone(app: &mut PryHub, ui: &mut egui::Ui, open: &mut Option<std::path::PathBuf>) {
    let t = app.lang.strings();
    let margin = egui::Margin { left: 22, right: 22, top: 30, bottom: 30 };
    // The row of controls inside the zone, so a click that landed on the field or on `Open` is not
    // also taken as a click on the zone behind them.
    let controls: egui::Rect;
    let mut field_id: Option<egui::Id> = None;
    let mut prepared = egui::Frame::new().inner_margin(margin).begin(ui);
    {
        let ui = &mut prepared.content_ui;
        let width = ui.available_width();
        ui.set_min_width(width);

        let (icon, _) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::hover());
        theme::icon::export(ui.painter(), icon.center(), 26.0, token::ACCENT);
        ui.add_space(10.0);
        ui.label(RichText::new(t.w_drop).font(theme::font::heading(15.0)));
        ui.add_space(3.0);
        ui.label(
            RichText::new("GEOMETRY.BIN · TEXTURES.BIN · GLOBALB.BUN · *.viv")
                .font(theme::font::body(12.0))
                .color(theme::muted(55)),
        );
        ui.add_space(token::SPACE_3);
        controls = ui
            .horizontal(|ui| {
                let hint = PryHub::suggested_dir();
                let field =
                    widget::input(ui, &mut app.path_input, (width - 92.0).max(0.0), &hint, true);
                field_id = Some(field.id);
                let go = widget::button_secondary(ui, t.m_open).clicked();
                if go || (field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                    *open = Some(std::path::PathBuf::from(app.path_input.trim()));
                }
            })
            .response
            .rect;
    }
    let rect = prepared.content_ui.min_rect() + margin;
    let hovered = ui.rect_contains_pointer(rect);
    prepared.frame.fill = if hovered { token::ACCENT_100 } else { token::SURFACE };
    let resp = prepared.end(ui);
    theme::draw::dashed_rect(
        ui.painter(),
        resp.rect,
        token::RULE,
        if hovered { token::ACCENT } else { token::NEUTRAL_400 },
    );

    // Read from the pointer rather than by claiming an interaction over the whole zone: an
    // interaction registered here would sit on top of the field and the button inside it and take
    // their clicks.
    let on_bare_zone = |p: egui::Pos2| resp.rect.contains(p) && !controls.contains(p);
    let pointer = ui.input(|i| i.pointer.interact_pos());
    if hovered && pointer.is_some_and(on_bare_zone) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let clicked = ui.input(|i| i.pointer.primary_clicked()) && pointer.is_some_and(on_bare_zone);
    // `picking.is_none()`: a second click while a chooser is already up would open a second one.
    if clicked && !app.ask_for_file(ui.ctx(), crate::jobs::Side::Main) {
        // No chooser on this machine: the field is the only way in, so say so by putting the caret
        // in it rather than by doing nothing at all.
        if let Some(id) = field_id {
            ui.ctx().memory_mut(|m| m.request_focus(id));
        }
    }
}

/// One recent file: its name, where it came from, and a dot that says whether it is the open one.
///
/// Painted rather than laid out, because the design's row is three things at three alignments —
/// a 132 px name column, a meta line that takes the rest, and a dot pinned to the right edge.
fn recent_row(ui: &mut egui::Ui, path: &std::path::Path, current: bool) -> bool {
    const PAD_X: f32 = 4.0;
    const PAD_Y: f32 = 10.0;
    const GAP: f32 = 11.0;
    const NAME_MIN: f32 = 132.0;
    const DOT: f32 = 8.0;

    let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let parent = path.parent().map_or_else(String::new, |p| p.display().to_string());
    let name = ui.painter().layout_no_wrap(name, theme::font::mono(12.0), token::TEXT);

    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(width, name.size().y.max(DOT) + PAD_Y * 2.0),
        Sense::click(),
    );
    if resp.hovered() {
        ui.painter().rect_filled(rect, 0.0_f32, token::SURFACE);
    }

    let meta_x = rect.left() + PAD_X + name.size().x.max(NAME_MIN) + GAP;
    let dot_x = rect.right() - PAD_X - DOT * 0.5;
    let mut job = egui::text::LayoutJob::simple_singleline(
        parent,
        theme::font::body(11.5),
        theme::muted(52),
    );
    job.wrap =
        egui::text::TextWrapping::truncate_at_width((dot_x - DOT * 0.5 - GAP - meta_x).max(0.0));
    let meta = ui.painter().layout_job(job);

    let p = ui.painter();
    p.galley(
        Pos2::new(rect.left() + PAD_X, rect.center().y - name.size().y * 0.5),
        name,
        token::TEXT,
    );
    p.galley(Pos2::new(meta_x, rect.center().y - meta.size().y * 0.5), meta, theme::muted(52));
    theme::draw::dot(
        p,
        Pos2::new(dot_x, rect.center().y),
        DOT,
        if current { token::ACCENT } else { token::NEUTRAL_500 },
    );
    theme::draw::rule_h(
        p,
        rect.left()..=rect.right(),
        rect.bottom() - token::HAIRLINE,
        token::HAIRLINE,
        token::DIVIDER,
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// The right column: the four modes as a 2 × 2 grid, and what the tool is trying to be.
fn modes_column(app: &mut PryHub, ui: &mut egui::Ui) -> Option<Screen> {
    let t = app.lang.strings();
    ui.spacing_mut().item_spacing = Vec2::new(token::SPACE_2, 0.0);
    widget::section_label(ui, t.w_modes);
    ui.add_space(token::SPACE_3);

    let cards = [
        (Screen::Validation, t.nav_validation, t.w_card_validation, glyph::shield as Glyph),
        (Screen::Discovery, t.nav_discovery, t.w_card_discovery, glyph::search as Glyph),
        (Screen::Diff, t.nav_diff, t.w_card_diff, glyph::branch as Glyph),
        (Screen::Dictionary, t.nav_dict, t.w_card_dict, glyph::hash as Glyph),
    ];

    // A grid row is as tall as its tallest cell. egui would leave the four ragged, so the body that
    // wraps to the most lines sets the height of all of them.
    let cell = ((ui.available_width() - GRID_GAP) * 0.5).max(0.0);
    let inner = (cell - CARD_PAD * 2.0).max(0.0);
    let body = theme::font::body(12.0);
    let tallest = cards
        .iter()
        .map(|(_, _, text, _)| {
            ui.painter().layout((*text).to_owned(), body.clone(), Color32::PLACEHOLDER, inner).size().y
        })
        .fold(0.0_f32, f32::max);
    let title_h = ui
        .painter()
        .layout_no_wrap(t.nav_validation.to_owned(), theme::font::heading(15.0), Color32::PLACEHOLDER)
        .size()
        .y;
    // `gap: 6px` between the icon, the title and the body.
    let content = CARD_ICON + 6.0 + title_h + 6.0 + tallest;

    let mut goto = None;
    for (i, row) in cards.chunks(2).enumerate() {
        if i > 0 {
            ui.add_space(GRID_GAP);
        }
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = GRID_GAP;
            for (screen, title, text, icon) in row {
                ui.allocate_ui_with_layout(
                    Vec2::new(cell, content + CARD_PAD * 2.0),
                    Layout::top_down(Align::Min),
                    |ui| {
                        if mode_card(ui, title, text, *icon, content) {
                            goto = Some(*screen);
                        }
                    },
                );
            }
        });
    }

    ui.add_space(18.0);
    widget::note_box(ui, t.w_strategy);
    goto
}

/// One mode card; returns true when clicked.
fn mode_card(ui: &mut egui::Ui, title: &str, body: &str, icon: Glyph, height: f32) -> bool {
    let (resp, ()) = widget::card(ui, |ui| {
        let width = ui.available_width();
        ui.set_min_size(Vec2::new(width, height));
        ui.spacing_mut().item_spacing.y = 6.0;
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(CARD_ICON), Sense::hover());
        icon(ui.painter(), rect.center(), CARD_ICON, token::ACCENT);
        ui.label(RichText::new(title).font(theme::font::heading(15.0)));
        ui.label(RichText::new(body).font(theme::font::body(12.0)).color(theme::muted(80)));
    });
    resp.clicked()
}

/// The four mode glyphs, transcribed from the design's own Lucide outlines the way
/// [`theme::icon`] transcribes the top bar's — same 24 × 24 box, same 2-unit stroke.
///
/// They live here rather than in `theme::icon` because no other screen draws them; if a second one
/// ever does, they belong up there beside `open` and `export`.
mod glyph {
    use egui::{Color32, Painter, Pos2, Shape, Stroke, Vec2};

    /// A point of the 24 × 24 viewBox, in screen space.
    fn pt(at: Pos2, size: f32, x: f32, y: f32) -> Pos2 {
        at + Vec2::new(x - 12.0, y - 12.0) * (size / 24.0)
    }

    fn stroke(size: f32, colour: Color32) -> Stroke {
        Stroke::new((size * 2.0 / 24.0).max(1.1), colour)
    }

    /// `shield-check` — validation: the file came back the way it went in.
    pub fn shield(p: &Painter, at: Pos2, size: f32, colour: Color32) {
        let v = |x, y| pt(at, size, x, y);
        let stroke = stroke(size, colour);
        p.add(Shape::line(
            vec![
                v(4.0, 13.0),
                v(4.0, 6.0),
                v(5.0, 5.0),
                v(12.0, 2.2),
                v(19.0, 5.0),
                v(20.0, 6.0),
                v(20.0, 13.0),
                v(16.5, 19.0),
                v(12.0, 21.9),
                v(7.5, 19.0),
                v(4.0, 13.0),
            ],
            stroke,
        ));
        p.add(Shape::line(vec![v(9.0, 12.0), v(11.0, 14.0), v(15.0, 10.0)], stroke));
    }

    /// `search` — discovery.
    pub fn search(p: &Painter, at: Pos2, size: f32, colour: Color32) {
        let v = |x, y| pt(at, size, x, y);
        let stroke = stroke(size, colour);
        p.circle_stroke(v(11.0, 11.0), 8.0 * size / 24.0, stroke);
        p.line_segment([v(16.7, 16.7), v(21.0, 21.0)], stroke);
    }

    /// `git-branch` — two files, and where they part.
    pub fn branch(p: &Painter, at: Pos2, size: f32, colour: Color32) {
        let v = |x, y| pt(at, size, x, y);
        let stroke = stroke(size, colour);
        let r = 3.0 * size / 24.0;
        p.circle_stroke(v(5.0, 6.0), r, stroke);
        p.circle_stroke(v(19.0, 18.0), r, stroke);
        p.add(Shape::line(vec![v(12.0, 6.0), v(17.0, 6.0), v(19.0, 8.0), v(19.0, 15.0)], stroke));
        p.add(Shape::line(vec![v(12.0, 18.0), v(7.0, 18.0), v(5.0, 16.0), v(5.0, 9.0)], stroke));
    }

    /// `hash` — the dictionary: a number, until someone names it.
    pub fn hash(p: &Painter, at: Pos2, size: f32, colour: Color32) {
        let v = |x, y| pt(at, size, x, y);
        let stroke = stroke(size, colour);
        p.line_segment([v(4.0, 9.0), v(20.0, 9.0)], stroke);
        p.line_segment([v(4.0, 15.0), v(20.0, 15.0)], stroke);
        p.line_segment([v(10.0, 3.0), v(8.0, 21.0)], stroke);
        p.line_segment([v(16.0, 3.0), v(14.0, 21.0)], stroke);
    }
}
