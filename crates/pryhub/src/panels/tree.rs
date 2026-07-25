//! The chunk tree.
//!
//! Rows are pre-flattened in [`crate::doc::Doc::rows`], so drawing is a filtered pass over a flat
//! list rather than a recursive walk each frame. The largest car file is ~7 500 nodes, which is
//! small enough to draw whole — the hex view is where virtualization is needed, not here.
//!
//! Each row carries what the design asks for: a caret for containers, a status dot, one label, a
//! codec badge, the verdict mark, and the id in hex. The label and the right-hand run are laid out
//! from opposite ends and meet in the middle, which is what the design's `flex:1` label does.

use crate::app::PryHub;
use crate::theme::{self, token};
use egui::{Color32, RichText, Sense};
use gizmo_nfs::validate::ChunkStatus;

/// The design's row rhythm. A row reserves 2 px for the selection bar and 6 px of pad whether or
/// not it is selected, then steps 13 px per level; inside that sit a 12 px caret column, a 6 px
/// dot, and 6 px between every element including the right-hand run.
const INDENT_BASE: f32 = token::RULE + 6.0;
const INDENT_STEP: f32 = 13.0;
const CARET_COL: f32 = 12.0;
const DOT: f32 = 6.0;
const GAP: f32 = 6.0;

/// Which rows a collapse state leaves visible.
///
/// One pass of depth comparisons over 7,246 rows costs microseconds; *drawing* all of them cost
/// 7.5 ms a frame, which was most of the frame budget spent on rows scrolled out of sight — egui
/// clipped the pixels but did the work anyway. Pure, and separate from the drawing, because this is
/// what the caret actually does and it is the half worth a test.
fn visible_rows<'a>(
    rows: &'a [crate::doc::Row],
    collapsed: &std::collections::HashSet<usize>,
) -> Vec<&'a crate::doc::Row> {
    let mut visible = Vec::with_capacity(rows.len());
    let mut hidden_below: Option<usize> = None;
    for row in rows {
        match hidden_below {
            // Deeper than the folded container: it is inside it, so it is not drawn.
            Some(d) if row.depth > d => continue,
            // Back at or above the fold's own depth: the subtree has ended.
            Some(_) => hidden_below = None,
            None => {}
        }
        if collapsed.contains(&row.offset) && row.has_children {
            hidden_below = Some(row.depth);
        }
        visible.push(row);
    }
    visible
}

/// What a click on the tree asked for.
///
/// Two things can be wanted from one row and the pointer says which: the caret column opens or
/// closes a container, everything else selects. The design toggles on the whole row and selects at
/// the same time, which costs you the ability to look at a solid without folding it away.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hit {
    /// Show this chunk.
    Select(usize),
    /// Open or close this container.
    Toggle(usize),
}

/// Draw the tree; returns what the user asked for, if anything.
pub fn show(app: &PryHub, ui: &mut egui::Ui) -> Option<Hit> {
    let Some(doc) = &app.doc else { return None };
    let mut hit = None;
    let row_h = app.density.row_height();

    let visible = visible_rows(&doc.rows, &app.collapsed);

    // The row pitch has to be set *before* `show_rows`, which sizes the viewport from
    // `row_height + item_spacing.y`. Setting it inside the closure left egui thinking every row was
    // three pixels taller than it is, so it handed out too short a range and the tree stopped
    // seven rows above the bottom of the panel.
    ui.spacing_mut().item_spacing.y = 0.0;
    // The list's own `padding: 4px 0`: a constant gap under the caption rule at every density,
    // rather than the panel's own vertical rhythm.
    ui.add_space(token::SPACE_1);
    egui::ScrollArea::both().auto_shrink([false, false]).show_rows(
        ui,
        row_h,
        visible.len(),
        |ui, range| {
            for row in &visible[range] {
                if let Some(h) = draw_row(app, doc, ui, row, row_h) {
                    hit = Some(h);
                }
            }
        },
    );
    hit
}

/// One row: its wash, its caret, its status dot, and the strings that make 610 identical
/// "SolidObject" rows navigable. Returns what was clicked, if anything.
fn draw_row(
    app: &PryHub,
    doc: &crate::doc::Doc,
    ui: &mut egui::Ui,
    row: &crate::doc::Row,
    row_h: f32,
) -> Option<Hit> {
    let selected = app.selection == Some(row.offset);
    let collapsed = app.collapsed.contains(&row.offset);

    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), row_h), Sense::click());
    // A fade rather than a snap: with 7,000 rows the eye needs a moment to see *which* row took the
    // selection, and the same wash is what the hover uses one step lighter.
    let lit = ui.ctx().animate_bool_with_time(
        ui.id().with(row.offset),
        selected,
        crate::chrome::MOVE_TIME,
    );
    let p = ui.painter().clone();
    if lit > 0.0 {
        p.rect_filled(rect, 0.0, token::BG.lerp_to_gamma(token::ACCENT_100, lit));
    }
    // The design's row hover is a plain CSS rule, so it lands *over* a selected row's tint rather
    // than being suppressed by it — the accent bar and the accent-800 ink are what keep the
    // selection legible under the pointer, and a selected row that answers nothing feels dead.
    if resp.hovered() {
        p.rect_filled(rect, 0.0, token::SURFACE);
    }
    if lit > 0.0 {
        // `border-left: 2px solid var(--color-accent)` — the same bar as `theme::draw::accent_left_bar`,
        // written out only because it has to fade in with the fill instead of snapping on.
        p.rect_filled(
            egui::Rect::from_min_size(rect.left_top(), egui::vec2(token::RULE, rect.height())),
            0.0,
            token::ACCENT.gamma_multiply(lit),
        );
    }

    let indent = INDENT_BASE + row.depth as f32 * INDENT_STEP;
    let mid = rect.center().y;
    let mono = theme::font::mono(app.density.body_size());

    // Caret — only containers have one, and it says whether it is open. Centred in its own 12 px
    // column, so a container and a leaf at the same depth start their labels at the same x.
    // The caret's own hit box, two pixels of slack either side of the 12 px column: it was drawn and
    // never listened to, so the tree could not be folded at all — `collapsed` was written by nothing
    // in the whole crate.
    let caret = egui::Rect::from_min_size(
        egui::pos2(rect.left() + indent - 2.0, rect.top()),
        egui::vec2(CARET_COL + 4.0, rect.height()),
    );
    if row.has_children {
        let over = resp.hovered() && ui.input(|i| i.pointer.latest_pos()).is_some_and(|q| caret.contains(q));
        p.text(
            egui::pos2(rect.left() + indent + CARET_COL * 0.5, mid),
            egui::Align2::CENTER_CENTER,
            if collapsed { "▸" } else { "▾" },
            mono.clone(),
            // It answers the pointer, because a control that looks identical whether or not it is
            // under the cursor reads as decoration — which is exactly what this one was.
            if over { token::ACCENT } else { theme::muted(45) },
        );
    }
    // Status dot — the design's at-a-glance "is there something wrong here". An unchecked leaf gets
    // no dot at all: a mark on something no rule read would be the tool vouching for what it does
    // not know, and a container still gets the neutral one because it is a place, not a verdict.
    let status = doc.report.status_of(row.offset);
    if let Some(colour) = dot_colour(status, row.container) {
        theme::draw::dot(
            &p,
            egui::pos2(rect.left() + indent + CARET_COL + GAP + DOT * 0.5, mid),
            DOT,
            colour,
        );
    }

    // The right-hand run, laid out right to left: the id, the verdict mark, the codec badge. What
    // is left over is the label's, which is the design's `flex:1`.
    let mut right = rect.right() - token::SPACE_2;
    // The id, always monospace and always the same size — this is what you match against a spec.
    right -= p
        .text(
            egui::pos2(right, mid),
            egui::Align2::RIGHT_CENTER,
            format!("{:#010x}", row.id),
            theme::font::mono(9.5),
            theme::muted(38),
        )
        .width()
        + GAP;
    right -= draw_mark(&p, status, right, mid, mono.size * 0.8);
    right -= badge(&p, doc, row, right, mid);

    // One label, not two: the part's name *replaces* the chunk type where there is one, which is
    // what makes a wall of "SolidObject" navigable. The design separates the three cases by weight;
    // the monospace family has one, so they are separated by ink instead.
    let label =
        row.name.as_deref().unwrap_or_else(|| crate::panels::inspector::chunk_label(row.id));
    let x = rect.left() + indent + CARET_COL + GAP + DOT + GAP;
    let room = right - x;
    if room > 0.0 {
        let text = elide(ui, label, &mono, room);
        p.text(
            egui::pos2(x, mid),
            egui::Align2::LEFT_CENTER,
            text,
            mono,
            if selected {
                token::ACCENT_800
            } else if row.name.is_some() {
                token::TEXT
            } else {
                theme::muted(75)
            },
        );
    }
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    // A double-click anywhere on a container folds it too — the accordion gesture people try before
    // they aim at a 12 px triangle.
    if row.has_children && resp.double_clicked() {
        return Some(Hit::Toggle(row.offset));
    }
    if resp.clicked() {
        let on_caret =
            row.has_children && resp.interact_pointer_pos().is_some_and(|q| caret.contains(q));
        return Some(if on_caret { Hit::Toggle(row.offset) } else { Hit::Select(row.offset) });
    }
    None
}

/// The row's verdict, drawn at `right` and returning how much width it took (gap included).
///
/// `⚠` and `✓` are drawn rather than typed for the reason [`theme::mark`] exists at all. Their
/// `size` is the ink's own extent where a glyph's font size is its em box, hence the caller's 0.8:
/// it puts the mark at the weight the design's text glyphs have beside a 12.5 px row.
fn draw_mark(p: &egui::Painter, status: ChunkStatus, right: f32, mid: f32, size: f32) -> f32 {
    let at = egui::pos2(right - size * 0.5, mid);
    match status {
        ChunkStatus::Error => theme::mark::error(p, at, size, token::ACCENT),
        ChunkStatus::Warn => theme::mark::warn(p, at, size, token::ACCENT),
        ChunkStatus::Ok => theme::mark::ok(p, at, size, token::NEUTRAL_500),
        _ => return 0.0,
    }
    size + GAP
}

/// The compression badge, when a chunk's payload carries a codec of its own.
///
/// The design only ever shows `JDLZ`, but the file decides: a chunk that starts with a RefPack or
/// HUFF signature says so rather than being labelled with the one codec the mock happened to draw.
/// `detect` reads four bytes, so this costs nothing per visible row.
fn badge(
    p: &egui::Painter,
    doc: &crate::doc::Doc,
    row: &crate::doc::Row,
    right: f32,
    mid: f32,
) -> f32 {
    let codec = doc
        .bytes
        .get(row.data_offset..)
        .map_or(gizmo_nfs::compression::Codec::None, gizmo_nfs::compression::detect);
    if matches!(codec, gizmo_nfs::compression::Codec::None) {
        return 0.0;
    }
    let galley = p.layout_no_wrap(
        format!("{codec:?}").to_uppercase(),
        theme::font::mono(8.5),
        token::NEUTRAL_800,
    );
    // `padding: 1px 4px` round the type.
    let size = galley.size() + egui::vec2(8.0, 2.0);
    let min = egui::pos2(right - size.x, mid - size.y * 0.5);
    p.rect_filled(egui::Rect::from_min_size(min, size), 0.0, token::NEUTRAL_200);
    p.galley(min + egui::vec2(4.0, 1.0), galley, token::NEUTRAL_800);
    size.x + GAP
}

/// Shorten `text` until it fits `room`, ending in an ellipsis.
pub(crate) fn elide(ui: &egui::Ui, text: &str, font: &egui::FontId, room: f32) -> String {
    let width = |s: &str| {
        ui.painter().layout_no_wrap(s.to_owned(), font.clone(), token::TEXT).size().x
    };
    if width(text) <= room {
        return text.to_owned();
    }
    // Names are ASCII in this format, but char boundaries are respected anyway.
    let mut cut: Vec<char> = text.chars().collect();
    while !cut.is_empty() {
        cut.pop();
        let candidate: String = cut.iter().collect::<String>() + "…";
        if width(&candidate) <= room {
            return candidate;
        }
    }
    String::new()
}

/// The row's status dot: what the checks concluded, falling back to a neutral shade that only says
/// container-or-leaf when nothing examined this chunk. `None` means no dot — an unchecked leaf is
/// the one row the design leaves blank.
fn dot_colour(status: ChunkStatus, container: bool) -> Option<Color32> {
    match status {
        ChunkStatus::Error | ChunkStatus::Warn => Some(token::ACCENT),
        ChunkStatus::Ok => Some(token::NEUTRAL_500),
        _ if container => Some(token::NEUTRAL_400),
        _ => None,
    }
}

/// The panel's caption row: title plus the open file's name.
pub fn caption(app: &PryHub, ui: &mut egui::Ui) {
    let t = app.lang.strings();
    let name = app.doc.as_ref().map(|d| d.file_name());
    crate::widget::caption_strip(ui, t.p_tree, |ui| {
        if let Some(name) = name {
            ui.label(RichText::new(name).font(theme::font::mono(10.0)).color(theme::muted(45)));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Row;
    use std::collections::HashSet;

    fn row(offset: usize, depth: usize, has_children: bool) -> Row {
        Row {
            offset,
            name: None,
            data_offset: offset + 8,
            id: 0x8000_0001,
            size: 0,
            depth,
            container: has_children,
            has_children,
        }
    }

    /// A tree shaped like a car's: a root, two solids, and a leaf after them at the root's depth.
    fn tree() -> Vec<Row> {
        vec![
            row(0, 0, true),    // SolidList
            row(10, 1, true),   //   SolidObject A
            row(20, 2, false),  //     header
            row(30, 2, false),  //     vertices
            row(40, 1, true),   //   SolidObject B
            row(50, 2, false),  //     header
            row(60, 0, false),  // a sibling of the root
        ]
    }

    #[test]
    fn nothing_folded_shows_every_row() {
        let rows = tree();
        assert_eq!(visible_rows(&rows, &HashSet::new()).len(), rows.len());
    }

    /// The accordion itself: folding a container hides what is *inside* it and nothing else.
    #[test]
    fn folding_a_container_hides_its_subtree_and_stops_there() {
        let rows = tree();
        let folded = HashSet::from([10]);
        let seen: Vec<usize> = visible_rows(&rows, &folded).iter().map(|r| r.offset).collect();
        // 20 and 30 are inside solid A; 40 is the next sibling and must come back.
        assert_eq!(seen, vec![0, 10, 40, 50, 60]);
    }

    /// Folding the root leaves the root itself and whatever follows it at the same depth — the case
    /// that goes wrong if the "we are out of the subtree" test uses `>=` instead of `>`.
    #[test]
    fn folding_the_root_keeps_the_root_and_its_siblings() {
        let rows = tree();
        let seen: Vec<usize> =
            visible_rows(&rows, &HashSet::from([0])).iter().map(|r| r.offset).collect();
        assert_eq!(seen, vec![0, 60]);
    }

    /// A leaf in the set is not a container, so it hides nothing — the caret is only drawn for rows
    /// that have children, and the fold has to agree with the caret.
    #[test]
    fn a_leaf_in_the_set_hides_nothing() {
        let rows = tree();
        assert_eq!(visible_rows(&rows, &HashSet::from([20])).len(), rows.len());
    }
}
