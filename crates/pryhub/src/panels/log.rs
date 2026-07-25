//! The log panel: what the tool noticed, and which chunk it noticed it about.
//!
//! Clicking a row selects the chunk it names — the design's third path into the same selection,
//! after the tree and the hex view.

use crate::app::{LogFilter, PryHub};
use crate::doc::{Level, Note};
use crate::i18n::Strings;
use crate::theme::{self, token};
use crate::widget::{self, Seg};
use egui::{Pos2, Sense, Vec2};

/// A row: the design's `padding: 3px 12px` over an 11.5 px line at `line-height: 1.55`.
const ROW_H: f32 = 24.0;
/// The row's horizontal inset, and the list's own `padding: 4px 0`.
const PAD_X: f32 = 12.0;
const LIST_PAD: f32 = 4.0;
/// The three columns. The 12 px mark and the 88 px floor under the chunk id are `min-width`s in the
/// design: they keep the messages in one column down the list however long an id runs — or whether
/// the note names a chunk at all.
const MARK_W: f32 = 12.0;
const GAP: f32 = 9.0;
const ID_W: f32 = 88.0;
/// The mark itself, a shade under the line so it sits inside its column rather than over it.
const MARK: f32 = 11.0;
/// The whole list is monospace at one size — the design sets it on the list, not per column.
const ROW_TEXT: f32 = 11.5;

/// Draw the log; returns the chunk offset to select, if a row was clicked.
pub fn show(app: &PryHub, ui: &mut egui::Ui) -> Option<usize> {
    let t = app.lang.strings();
    let Some(doc) = &app.doc else { return None };
    let mut clicked = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        // Rows are flush in the design: the wash under the hovered one is the only thing that
        // separates one line from the next, and a gutter would read as a table.
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.add_space(LIST_PAD);
        // What parsing found, then what the session did: the document is immutable, so its notes
        // and the session's are two lists that read as one.
        let notes = doc.notes.iter().chain(app.log.iter());
        for note in notes.filter(|n| app.log_filter.accepts(n.level)) {
            if row(ui, note, t) {
                clicked = note.chunk;
            }
        }
        ui.add_space(LIST_PAD);
    });
    clicked
}

/// One line: its mark, the chunk it names, and what happened. Returns whether it was clicked.
fn row(ui: &mut egui::Ui, note: &Note, t: &Strings) -> bool {
    // A note that names no chunk leads nowhere, so it answers the pointer with nothing — the design
    // has a destination behind every row, this app does not.
    let navigable = note.chunk.is_some();
    let sense = if navigable { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), sense);
    if navigable && resp.hovered() {
        ui.painter().rect_filled(rect, 0.0_f32, token::SURFACE);
    }

    let p = ui.painter().clone();
    let mid = rect.center().y;
    let font = theme::font::mono(ROW_TEXT);

    mark(&p, Pos2::new(rect.left() + PAD_X + MARK_W * 0.5, mid), note.level);

    let id_x = rect.left() + PAD_X + MARK_W + GAP;
    let id = p.layout_no_wrap(note.chunk_id.clone(), font.clone(), theme::muted(42));
    let id_w = id.size().x.max(ID_W);
    p.galley(Pos2::new(id_x, mid - id.size().y * 0.5), id, theme::muted(42));

    // The message carries the level too, not just the mark beside it.
    let ink = match note.level {
        Level::Warn | Level::Error => token::ACCENT_800,
        Level::Info => token::TEXT,
    };
    let msg_x = id_x + id_w + GAP;
    let mut job = egui::text::LayoutJob::single_section(
        note.kind.text(t),
        egui::TextFormat { font_id: font, color: ink, ..Default::default() },
    );
    // One line per note. A parser message can run to a paragraph, and a row that wrapped would
    // break the pitch the eye is scanning down.
    job.wrap = egui::text::TextWrapping::truncate_at_width((rect.right() - PAD_X - msg_x).max(1.0));
    let msg = p.layout_job(job);
    p.galley(Pos2::new(msg_x, mid - msg.size().y * 0.5), msg, ink);

    navigable && resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// The leading column's mark, drawn rather than typed: `✕` is in none of the bundled fallbacks and
/// `⚠` is missing from the heading face, so either would land as a tofu box — see [`theme::mark`].
fn mark(p: &egui::Painter, at: Pos2, level: Level) {
    match level {
        Level::Warn => theme::mark::warn(p, at, MARK, token::ACCENT),
        Level::Error => theme::mark::error(p, at, MARK, token::ACCENT_700),
        // The design's `ℹ`, in the neutral it sets it in. `theme::mark` has no info glyph and a
        // typed one would fall through to whatever emoji face the machine happens to carry, so the
        // tree's own status dot stands in — the two lists then mark a chunk the same way.
        Level::Info => theme::draw::dot(p, at, 5.0, token::NEUTRAL_600),
    }
}

/// The caption row: title plus the four filter buttons.
pub fn caption(app: &mut PryHub, ui: &mut egui::Ui) {
    let t = app.lang.strings();
    // The band, its rule and the list under it are flush; egui's default gutter would open a strip
    // of page colour between them. Set on the panel's own ui, so it holds for all three.
    ui.spacing_mut().item_spacing.y = 0.0;
    widget::caption_strip(ui, t.p_log, |ui| {
        // The strip's slot is right-aligned — every other region puts a filename or a count there.
        // The log's filters belong *beside* the title, so the slot is turned back around.
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            // The group's own `margin-left`, on top of the strip's 8 px gap.
            ui.add_space(10.0);
            widget::segmented(
                ui,
                egui::Id::new("log_filter"),
                &mut app.log_filter,
                &[
                    (LogFilter::All, t.log_all),
                    (LogFilter::Warn, t.log_warn),
                    (LogFilter::Error, t.log_err),
                    (LogFilter::Info, t.log_info),
                ],
                Seg::Small,
            );
        });
    });
}
