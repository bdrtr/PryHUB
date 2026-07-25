//! The compare screen: the open file against another one, chunk by chunk.
//!
//! The comparison is [`gizmo_nfs::diff`] — paired by position among siblings of the same id, which
//! is the only pairing this format's ordered trees justify. What lives here is the second file (a
//! whole [`Doc`], so the right-hand side is as real as the left) and the reading of the report:
//! only the differences by default, since "what is different about these two cars" should not
//! arrive as seven thousand lines of *same*.
//!
//! The report is read as the design's two-column grid: the left file's chunk in the A cell, the
//! right file's in the B cell, an em dash where a side has none, and the row's state carried by the
//! wash behind it rather than by a glyph in front of it. The rows stay virtualised — the design's
//! mock holds seven, a car holds seven thousand — so each one is painted into its own rect instead
//! of being laid out as a table.

use crate::app::{PryHub, Screen};
use crate::doc::Doc;
use crate::panels;
use crate::theme::{self, token};
use crate::widget::{self, Seg};
use egui::{Align2, Color32, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use gizmo_nfs::diff::{self, Report, Status};

/// The design's column for this screen (`max-width: 920px`).
const PAGE_WIDTH: f32 = 920.0;
/// `padding:30px 34px 40px`, beside the `max-width` above.
const PAGE_PAD: egui::Margin = egui::Margin { left: 34, right: 34, top: 30, bottom: 40 };
/// A table cell's padding: `padding: 8px 14px`.
const CELL_PAD_X: f32 = 14.0;
const CELL_PAD_Y: f32 = 8.0;
/// What a side that has no chunk at all shows — the design's own placeholder.
const EM_DASH: &str = "—";

/// The screen's own state: the other file, and what comparing it said.
#[derive(Default)]
pub struct State {
    /// The path field — the same way the welcome screen takes a file, since there is no dialog.
    pub path_input: String,
    /// The other file.
    pub right: Option<Doc>,
    report: Option<Report>,
    /// Which left-hand file the report was computed against, so opening another one recomputes it.
    computed_for: Option<std::path::PathBuf>,
    /// Whether identical chunks are listed too.
    show_all: bool,
    /// The last failure to open the other file.
    error: Option<String>,
}

impl State {
    /// Take a parsed second file, or the failure to parse it. Called from the job collector.
    pub fn adopt(&mut self, result: Result<Doc, String>, _path: &std::path::Path) {
        match result {
            Ok(doc) => {
                self.right = Some(doc);
                self.report = None;
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }
}

/// Draw the screen; returns a chunk offset when a row was clicked (an offset in the *left* file).
pub fn show(app: &mut PryHub, ui: &mut egui::Ui) -> Option<usize> {
    let mut select = None;
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(token::BG))
        .show_inside(ui, |ui| {
            // `overflow:auto`, like every other full page: the legend under the table is the first
            // thing a short window loses otherwise.
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                select = widget::page(ui, PAGE_WIDTH, PAGE_PAD, |ui| body(app, ui));
            });
        });
    select
}

/// The page itself, inside the centred column.
fn body(app: &mut PryHub, ui: &mut egui::Ui) -> Option<usize> {
    let t = app.lang.strings();
    // The design's DIFF header is the title and nothing else; the caption that used to sit here is
    // the welcome card's line, not this screen's.
    widget::screen_header(ui, t.nav_diff, None, None);

    let Some(left) = &app.doc else {
        ui.label(RichText::new(t.no_file).color(theme::muted(50)));
        return None;
    };
    let left_name = left.file_name();
    let left_path = left.path.clone();

    picker(app, ui);

    // Recompute when either side changed. Both files are already parsed, so this is a walk
    // of two trees, not a re-read.
    let stale = app.diff.computed_for.as_deref() != Some(left_path.as_path());
    if app.diff.report.is_none() || stale {
        if let (Some(left), Some(right)) = (&app.doc, &app.diff.right) {
            app.diff.report =
                Some(diff::compare(&left.roots, &left.bytes, &right.roots, &right.bytes));
            app.diff.computed_for = Some(left_path);
        }
    }

    if let Some(e) = app.diff.error.clone() {
        ui.add_space(token::SPACE_2);
        ui.label(RichText::new(format!("{}: {e}", t.open_failed)).color(token::ACCENT));
    }
    if app.diff.report.is_none() {
        ui.add_space(token::SPACE_3);
        ui.label(RichText::new(t.df_pick).color(theme::muted(50)));
        return None;
    }

    // The design lists its seven rows unconditionally and has no filter at all. The choice is a
    // real one on a file with seven thousand chunks, so it is kept — but said in the design's own
    // vocabulary for a binary choice, beside the table's head rather than as egui's checkbox.
    ui.add_space(token::SPACE_3);
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            widget::segmented(
                ui,
                egui::Id::new("diff_filter"),
                &mut app.diff.show_all,
                &[(false, t.df_changed), (true, t.df_all)],
                Seg::Small,
            );
        });
    });
    ui.add_space(token::SPACE_2);

    let right_name = app.diff.right.as_ref().map(Doc::file_name);
    let show_all = app.diff.show_all;
    let Some(report) = &app.diff.report else {
        return None;
    };
    let select = table(t, report, show_all, (left_name.as_str(), right_name.as_deref()), ui);
    ui.add_space(14.0);
    legend(t, report, ui);
    select
}

/// How the second file is loaded. The mock has file B open already, so this pair has no design of
/// its own; it is the system's `.input` and `.btn`, which is what the design uses wherever it does
/// ask for a path.
///
/// Two buttons and not one: `Open` opens what the field already holds, `Browse` asks the desktop for
/// a file. The field's own hint says "drop a file or click", and on this screen there was nothing at
/// all behind the click — the chooser was wired into the welcome screen and nowhere else.
fn picker(app: &mut PryHub, ui: &mut egui::Ui) {
    let t = app.lang.strings();
    let mut browse = false;
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widget::button_secondary(ui, t.m_open).clicked() {
                let path = std::path::PathBuf::from(app.diff.path_input.trim());
                app.open_other(&path);
            }
            // Only where there is something to open: a button that reaches for a chooser this
            // machine does not have is worse than no button.
            if crate::picker::available() {
                ui.add_space(token::SPACE_2);
                browse = widget::button_secondary(ui, t.browse).clicked();
            }
            ui.add_space(token::SPACE_2);
            // Whatever the buttons left: the field is the row, not a guessed width.
            let width = ui.available_width();
            widget::input(ui, &mut app.diff.path_input, width, t.w_drop, true);
        });
    });
    if browse {
        app.ask_for_file(ui.ctx(), crate::jobs::Side::Other);
    }
}

/// The table: a hairline box holding the A/B head and one virtualised row per chunk.
fn table(
    t: &crate::i18n::Strings,
    report: &Report,
    show_all: bool,
    names: (&str, Option<&str>),
    ui: &mut egui::Ui,
) -> Option<usize> {
    let rows: Vec<&diff::Entry> =
        report.entries.iter().filter(|e| show_all || e.status.differs()).collect();
    let mono = theme::font::mono(12.0);
    let row_h = line_height(ui, &mono) + CELL_PAD_Y * 2.0;
    // The legend follows the table and the page has its own bottom padding under that, so the rows
    // take what is left less both.
    let body_h =
        (ui.available_height() - 46.0 - f32::from(PAGE_PAD.bottom)).max(row_h * 4.0);
    let mut select = None;
    egui::Frame::new().stroke(Stroke::new(token::HAIRLINE, token::DIVIDER)).show(ui, |ui| {
        // The grid has `gap: 0`: head and rows butt together, and the virtualiser reads this
        // spacing to place the rows it skipped.
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        header(t, ui, names);
        egui::ScrollArea::vertical()
            .auto_shrink([false, true])
            .max_height(body_h)
            .show_rows(ui, row_h, rows.len(), |ui, range| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                for entry in rows[range].iter().copied() {
                    if let Some(offset) = row(t, ui, entry, &mono, row_h) {
                        select = Some(offset);
                    }
                }
            });
    });
    select
}

/// The table's head: each file's name over the side it names, closed by the 2 px rule.
fn header(t: &crate::i18n::Strings, ui: &mut egui::Ui, names: (&str, Option<&str>)) {
    // The design sets the name in `.mono` at `font-weight: 800`; the monospace family here is a
    // single face, so the 13 px against the caption's 11 is what separates them.
    let name_font = theme::font::mono(13.0);
    let cap_font = theme::font::body(11.0);
    let name_h = line_height(ui, &name_font);
    let cap_h = line_height(ui, &cap_font);
    let width = ui.available_width();
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(width, name_h + cap_h + 20.0), Sense::hover());
    let split = rect.center().x;
    let p = ui.painter();
    p.rect_filled(rect, 0.0_f32, token::SURFACE);
    column_rule(p, rect, split);
    theme::draw::rule_h(
        p,
        rect.left()..=rect.right(),
        rect.bottom() - token::RULE,
        token::RULE,
        token::DIVIDER,
    );
    for (x, name, caption) in
        [(rect.left(), names.0, t.df_left), (split, names.1.unwrap_or(EM_DASH), t.df_right)]
    {
        let cell = Rect::from_min_max(
            Pos2::new(x, rect.top()),
            Pos2::new(x + rect.width() * 0.5, rect.bottom()),
        );
        let p = p.with_clip_rect(cell);
        let at = Pos2::new(x + CELL_PAD_X, rect.top() + 10.0);
        p.text(at, Align2::LEFT_TOP, name, name_font.clone(), token::TEXT);
        p.text(
            Pos2::new(at.x, at.y + name_h),
            Align2::LEFT_TOP,
            caption,
            cap_font.clone(),
            theme::muted(52),
        );
    }
}

/// One chunk as the design's pair of cells, washed in what became of it.
fn row(
    t: &crate::i18n::Strings,
    ui: &mut egui::Ui,
    entry: &diff::Entry,
    mono: &egui::FontId,
    height: f32,
) -> Option<usize> {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    let split = rect.center().x;
    let a_rect = Rect::from_min_max(rect.min, Pos2::new(split, rect.bottom()));
    let b_rect = Rect::from_min_max(Pos2::new(split, rect.top()), rect.max);

    let (a_fill, b_fill) = fills(entry.status);
    let p = ui.painter();
    for (cell, fill) in [(a_rect, a_fill), (b_rect, b_fill)] {
        if fill != Color32::TRANSPARENT {
            p.rect_filled(cell, 0.0_f32, fill);
        }
    }
    // The row answers the pointer, since clicking it goes somewhere. The design has no hover state
    // for a table row, so it is the system's own wash and nothing louder.
    if resp.hovered() {
        p.rect_filled(rect, 0.0_f32, token::WASH_HOVER);
    }
    // Rows are separated by the softer of the two rules; the one *between* A and B is structural.
    theme::draw::rule_h(
        p,
        rect.left()..=rect.right(),
        rect.bottom() - token::HAIRLINE,
        token::HAIRLINE,
        token::DIVIDER_SOFT,
    );
    column_rule(p, rect, split);

    // The mock is one flat list; a real file is a tree, so the cell keeps its depth. Capped so a
    // deep chunk cannot push its own label out of the cell.
    let indent = CELL_PAD_X + (entry.depth as f32 * 10.0).min(60.0);
    let label = format!("{:#010x} {}", entry.id, panels::inspector::chunk_label(entry.id));
    let y = rect.center().y;

    let (a_text, a_ink) = cell_text(&label, entry.left.is_some());
    p.with_clip_rect(a_rect).text(
        Pos2::new(a_rect.left() + indent, y),
        Align2::LEFT_CENTER,
        a_text,
        mono.clone(),
        a_ink,
    );

    let b = p.with_clip_rect(b_rect);
    let (b_text, b_ink) = cell_text(&label, entry.right.is_some());
    let galley = b.layout_no_wrap(b_text.to_owned(), mono.clone(), b_ink);
    let (b_x, b_w, b_h) = (b_rect.left() + indent, galley.size().x, galley.size().y);
    b.galley(Pos2::new(b_x, y - b_h * 0.5), galley, b_ink);
    // The annotation lives in the B cell only, after its text — the design's one accent mark on an
    // otherwise inked row.
    if let Some(note) = note(t, entry) {
        b.text(
            Pos2::new(b_x + b_w + token::SPACE_2, y),
            Align2::LEFT_CENTER,
            note,
            theme::font::body(10.5),
            token::ACCENT,
        );
    }

    // The offsets and sizes the flat line used to carry: they are what the row is *about* only once
    // you have decided to look at it, so they wait in the hover beside what clicking does.
    let where_is = match (entry.left, entry.right) {
        (Some(l), Some(r)) if l.size != r.size => {
            format!("@{} · {} → {} B\n", l.offset, l.size, r.size)
        }
        (Some(l), _) => format!("@{} · {} B\n", l.offset, l.size),
        (None, Some(r)) => format!("@{} · {} B\n", r.offset, r.size),
        (None, None) => String::new(),
    };
    let resp = resp.on_hover_text(format!("{where_is}{}", t.df_go));
    // Only a chunk that exists on the left can be selected there — the tree it would jump to is the
    // left file's.
    let resp = if entry.left.is_some() {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    };
    if resp.clicked() {
        entry.left.map(|l| l.offset)
    } else {
        None
    }
}

/// The design's legend, under the table: the wash a row wears, and what it means.
///
/// The counts are the app's own — the mock has none — but they belong to the swatch rather than to
/// a strip of their own, which is why the tally moved down here from above the table.
fn legend(t: &crate::i18n::Strings, report: &Report, ui: &mut egui::Ui) {
    if report.identical() {
        ui.label(RichText::new(t.df_identical).color(token::NEUTRAL_600));
        return;
    }
    let [same, changed, resized, only_left, only_right] = report.tally();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = token::SPACE_4;
        for (n, label, colour) in [
            (changed, t.df_changed, token::ACCENT_200),
            (resized, t.df_resized, token::ACCENT_200),
            (only_left, t.df_only_left, token::ACCENT_100),
            (only_right, t.df_only_right, token::ACCENT_100),
            (same, t.df_same, token::NEUTRAL_200),
        ] {
            widget::swatch(ui, colour, &format!("{n} {label}"));
        }
    });
}

/// The design's `cellStyle`: a row's state is the cell background, one wash per side.
fn fills(status: Status) -> (Color32, Color32) {
    // The absent half of a one-sided row: the design greys it and drops it to half opacity, so it
    // reads as "there is nothing here" rather than as a second colour.
    let ghost = token::NEUTRAL_100.gamma_multiply(0.5);
    match status {
        // The mock has no `resized` kind — it is a change with a size to it, and reads as one; the
        // note carries the delta.
        Status::Changed | Status::Resized => (token::ACCENT_200, token::ACCENT_200),
        Status::OnlyLeft => (token::ACCENT_100, ghost),
        Status::OnlyRight => (ghost, token::ACCENT_100),
        _ => (Color32::TRANSPARENT, Color32::TRANSPARENT),
    }
}

/// What a cell says, and in which ink — a chunk, or the placeholder for a side that has none.
fn cell_text(label: &str, present: bool) -> (&str, Color32) {
    if present {
        (label, token::TEXT)
    } else {
        (EM_DASH, theme::muted(40))
    }
}

/// The B cell's accent annotation: the one thing about this row worth a word.
fn note(t: &crate::i18n::Strings, entry: &diff::Entry) -> Option<String> {
    match entry.status {
        Status::OnlyLeft => Some(t.df_only_left.to_owned()),
        Status::OnlyRight => Some(t.df_only_right.to_owned()),
        Status::Resized => match (entry.left, entry.right) {
            (Some(l), Some(r)) => Some(format!("{} → {} B", l.size, r.size)),
            _ => None,
        },
        Status::Changed => entry
            .first_difference
            .map(|at| format!("{} +0x{at:X} ({} B)", t.df_first, entry.differing_bytes)),
        _ => None,
    }
}

/// The `1px` rule between the A and B halves of a row.
fn column_rule(p: &egui::Painter, rect: Rect, x: f32) {
    p.rect_filled(
        Rect::from_min_size(Pos2::new(x, rect.top()), Vec2::new(token::HAIRLINE, rect.height())),
        0.0_f32,
        token::DIVIDER,
    );
}

/// One line of `font`. egui exposes row height per *text style*, and these sizes are the design's
/// own rather than the density's, so they are measured instead.
fn line_height(ui: &egui::Ui, font: &egui::FontId) -> f32 {
    ui.painter().layout_no_wrap("0".to_owned(), font.clone(), Color32::PLACEHOLDER).size().y
}

/// A file dropped while this screen is showing loads the *other* side rather than replacing the
/// open file — on a compare screen, that is what dropping a file obviously means.
pub fn accepts_drop(app: &PryHub) -> bool {
    app.screen == Screen::Diff && app.doc.is_some()
}
