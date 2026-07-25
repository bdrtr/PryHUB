//! The hex view.
//!
//! A car file is 2–11 MB, so at 16 bytes to the row this is up to ~700 000 rows: the list is
//! virtualized with `show_rows` and reads straight out of the document's single buffer, so no
//! byte is ever copied to draw it.
//!
//! The selected chunk's whole extent is a wash, and the fields the inspector named sit on top of
//! it. The strip over the grid names the two layers the design names — the chunk's region and its
//! counter bytes — and states the range the wash covers. Clicking a byte selects the chunk that
//! owns it, which is the design's "hex → tree" direction of the same synchronisation.

use crate::app::PryHub;
use crate::theme::{self, token};
use egui::{Color32, RichText};

/// Bytes per row. 16 is what every hex editor uses and what the design draws.
pub const BYTES_PER_ROW: usize = 16;

/// The design's `line-height: 1.55` over the monospace step — the leading is what keeps a dump
/// readable, so it is derived from the type size rather than picked per density.
const LINE_HEIGHT: f32 = 1.55;

/// The design's fixed 14 px between the offset gutter, the grid and the ascii column. Fixed, not a
/// multiple of the glyph: the three columns must not drift apart as the type grows.
const COLUMN_GAP: f32 = 14.0;

/// `min-width: 58px` on the offset gutter.
const OFFSET_WIDTH: f32 = 58.0;

/// How long a cell stays flooded after the selection moves (the design's `syncflash`).
const FLASH_TIME: f32 = 0.5;

/// What a byte is, for colouring.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Class {
    Outside,
    /// Inside the selected chunk's payload.
    Region,
    /// Inside the selected chunk's own 8-byte header.
    Header,
    /// A field the inspector named: a counter, a name, a matrix, the alignment filler.
    Key(gizmo_nfs::inspect::SpanRole),
}

/// The byte ranges the view highlights, derived from the selection once per frame.
struct Highlights {
    region: std::ops::Range<usize>,
    header: std::ops::Range<usize>,
    /// The inspector's own field offsets. These are the same numbers the inspector shows, so a row
    /// in the pane and a run of bytes in the grid are literally the same fact.
    key: Vec<gizmo_nfs::inspect::KeySpan>,
}

impl Highlights {
    fn class_of(&self, byte: usize) -> Class {
        if let Some(span) = self.key.iter().find(|s| byte >= s.start && byte < s.start + s.len) {
            Class::Key(span.role)
        } else if self.header.contains(&byte) {
            Class::Header
        } else if self.region.contains(&byte) {
            Class::Region
        } else {
            Class::Outside
        }
    }
}

/// Draw the hex view; returns a byte offset the user clicked, if any.
pub fn show(app: &PryHub, ui: &mut egui::Ui) -> Option<usize> {
    let Some(doc) = &app.doc else {
        empty(app, ui);
        return None;
    };

    let selected = app.selection.and_then(|o| doc.node_at(o));
    let highlights = selected.map(|n| Highlights {
        region: n.offset..n.data_offset + n.header.size as usize,
        header: n.offset..n.data_offset,
        key: app.selected_model().map(|m| m.key_spans.clone()).unwrap_or_default(),
    });

    // The design's `padding: 10px 12px` on the pane; the panel frame contributes the rest.
    ui.add_space(token::SPACE_2);
    if let Some(node) = selected {
        header_strip(app, ui, node);
        ui.add_space(token::SPACE_2);
    }
    let flash = sync_flash(app, ui);

    // The design tunes the grid's type on its own scale (`--hfs`), not as an offset from the body.
    let mono = theme::font::mono(app.density.mono_size());
    let row_h = (app.density.mono_size() * LINE_HEIGHT).round();
    let total_rows = doc.bytes.len().div_ceil(BYTES_PER_ROW);
    // `FontsView::glyph_width` needs a mutable view that `Ui::fonts` will not hand out; laying out
    // one digit gives the same number, and hoisted here it costs one layout a frame, not one a row.
    let char_w = ui.painter().layout_no_wrap("0".to_owned(), mono.clone(), token::TEXT).size().x;
    // Six digits address 16 MB; nothing this tool opens comes close, but a `.VIV` might.
    let digits = if doc.bytes.len() > 0xFF_FFFF { 8 } else { 6 };
    let offset_w = (char_w * digits as f32).max(OFFSET_WIDTH);
    let cell_w = char_w * 3.0;
    let mut clicked = None;

    let mut area = egui::ScrollArea::vertical().auto_shrink([false, false]);
    // A tree click asks the hex view to follow. The row height is uniform, so the target offset
    // is exact arithmetic — no scroll-to-rect hunting through rows that were never rendered.
    if let Some(target) = app.scroll_hex_to.filter(|_| app.doc.is_some()) {
        let row = target / BYTES_PER_ROW;
        let y = row as f32 * row_h - ui.available_height() * 0.35;
        area = area.vertical_scroll_offset(y.max(0.0));
    }

    area.show_rows(ui, row_h, total_rows, |ui, rows| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for row in rows {
            let start = row * BYTES_PER_ROW;
            let end = (start + BYTES_PER_ROW).min(doc.bytes.len());
            // The right inset the design has, which the shared panel frame cannot give this pane
            // alone — without it a selected last column runs into the scrollbar.
            let (rect, resp) = ui.allocate_exact_size(
                egui::vec2((ui.available_width() - 6.0).max(0.0), row_h),
                egui::Sense::click(),
            );
            let p = ui.painter();
            let mid = rect.center().y;

            // Offset column. 6 px here plus the panel frame's own 6 is the design's 12 px inset.
            let off_x = rect.left() + 6.0;
            let hex_left = off_x + offset_w + COLUMN_GAP;
            let ascii_x = hex_left + cell_w * BYTES_PER_ROW as f32 + COLUMN_GAP;
            p.text(
                egui::pos2(off_x, mid),
                egui::Align2::LEFT_CENTER,
                format!("{start:0width$x}", width = digits),
                mono.clone(),
                token::NEUTRAL_600,
            );

            for (i, b) in doc.bytes[start..end].iter().enumerate() {
                let byte = start + i;
                let class = highlights.as_ref().map_or(Class::Outside, |h| h.class_of(byte));
                let cell = egui::Rect::from_min_size(
                    egui::pos2(hex_left + cell_w * i as f32, rect.top()),
                    egui::vec2(cell_w, row_h),
                );
                if class != Class::Outside {
                    // The wash fills the whole cell, so consecutive selected bytes tile edge to
                    // edge and a chunk reads as one band rather than as a row of chips.
                    p.rect_filled(cell, 0.0, wash(class).lerp_to_gamma(token::ACCENT_400, flash));
                }
                p.text(
                    cell.center(),
                    egui::Align2::CENTER_CENTER,
                    format!("{b:02x}"),
                    mono.clone(),
                    ink(class, *b),
                );

                let glyph = egui::Rect::from_min_size(
                    egui::pos2(ascii_x + char_w * i as f32, rect.top()),
                    egui::vec2(char_w, row_h),
                );
                if class != Class::Outside {
                    // The ascii column carries the selection too — at a tenth of the strength, and
                    // without the flash, which the design puts on the grid alone.
                    p.rect_filled(glyph, 0.0, token::ACCENT_100);
                }
                // A middle dot, not a full stop: a period is a byte a name can actually contain,
                // and 0x20 is a space rather than "unprintable".
                let ch = if (0x20..0x7f).contains(b) { *b as char } else { '·' };
                p.text(
                    glyph.center(),
                    egui::Align2::CENTER_CENTER,
                    ch,
                    mono.clone(),
                    if class == Class::Outside { theme::muted(42) } else { token::ACCENT_800 },
                );
            }

            if resp.clicked() {
                // Which byte was clicked: the column under the pointer, so a click in the hex
                // grid selects the chunk that owns exactly that byte.
                if let Some(pos) = resp.interact_pointer_pos() {
                    let col = ((pos.x - hex_left) / cell_w).floor();
                    let col = col.clamp(0.0, (BYTES_PER_ROW - 1) as f32) as usize;
                    clicked = Some((start + col).min(doc.bytes.len().saturating_sub(1)));
                }
            }
        }
    });
    clicked
}

/// The strip over the grid: what is selected, where it sits, and what the washes under it mean.
fn header_strip(app: &PryHub, ui: &mut egui::Ui, node: &gizmo_nfs::chunk::ChunkNode) {
    let t = app.lang.strings();
    let mono = theme::font::mono(11.0);
    let dim = theme::muted(55);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        // The design's `font-weight:800`; in this port a weight is a face, so the chunk's type is
        // set in the heading one.
        ui.label(
            RichText::new(super::inspector::chunk_label(node.header.id))
                .font(theme::font::heading(11.0))
                .color(token::TEXT),
        );
        ui.label(RichText::new(format!("{:#010x}", node.header.id)).font(mono.clone()).color(dim));
        ui.label(RichText::new("·").font(mono.clone()).color(dim));
        ui.label(
            RichText::new(format!(
                "0x{:06x} … +{}",
                node.offset,
                fmt_size(node.header.size as usize)
            ))
            .font(mono.clone())
            .color(dim),
        );
        ui.add_space(6.0); // the design sets the first chip off from the range by a further 6 px
        legend(ui, token::ACCENT_200, t.hx_region, &mono, dim);
        legend(ui, token::ACCENT_500, t.hx_field, &mono, dim);
    });
}

/// One legend chip: a 10 px swatch, 5 px, the name of what it marks.
fn legend(ui: &mut egui::Ui, colour: Color32, label: &str, font: &egui::FontId, text: Color32) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 0.0, colour);
        ui.label(RichText::new(label).font(font.clone()).color(text));
    });
}

/// The design's `syncflash`: when the selection moves, its cells flood `accent-400` and fade back
/// over half a second. It is the one thing in the view that *shows* the tree click and these bytes
/// to be the same fact, rather than leaving the reader to notice that a highlight moved.
fn sync_flash(app: &PryHub, ui: &egui::Ui) -> f32 {
    let id = egui::Id::new("hex_syncflash");
    let key = app.selection.unwrap_or(usize::MAX);
    let ctx = ui.ctx();
    if ctx.data(|d| d.get_temp::<usize>(id)) != Some(key) {
        ctx.data_mut(|d| d.insert_temp(id, key));
        // Zero duration is a jump, not an animation: it re-arms the decay below.
        ctx.animate_value_with_time(id, 1.0, 0.0);
    }
    ctx.animate_value_with_time(id, 0.0, FLASH_TIME)
}

/// A chunk's size in the largest unit that stays readable — the design's `fmtSize`.
fn fmt_size(n: usize) -> String {
    if n >= 1 << 20 {
        format!("{:.2} MB", n as f64 / (1_usize << 20) as f64)
    } else if n >= 1 << 10 {
        format!("{:.1} KB", n as f64 / (1_usize << 10) as f64)
    } else {
        format!("{n} B")
    }
}

/// The wash behind a highlighted byte — the design's own swatches: `accent-200` marks the chunk's
/// region, `accent-500` the bytes that carry its counters. (`accent-100` reads as white against the
/// page, which is what made the region invisible before.)
///
/// The four layers past those two come from the inspector and have no name in the design, so the
/// legend cannot yet spell them out; they stay on the same ramp so the order still reads as one.
fn wash(class: Class) -> Color32 {
    use gizmo_nfs::inspect::SpanRole;
    match class {
        Class::Outside => Color32::TRANSPARENT,
        Class::Region => token::ACCENT_200,
        Class::Header => token::ACCENT_300,
        Class::Key(SpanRole::Counter) => token::ACCENT_500,
        Class::Key(SpanRole::Name) => token::ACCENT_2,
        Class::Key(SpanRole::Matrix) => token::ACCENT_400,
        // Filler is inert by definition: showing it as dead grey is itself the information.
        Class::Key(_) => token::NEUTRAL_300,
    }
}

/// The ink over a wash.
///
/// The design has one rule for the whole selection — `accent-800`, or the page colour where the
/// wash is dark enough to need it — and only *outside* the selection does a byte's value change
/// anything.
fn ink(class: Class, b: u8) -> Color32 {
    if light_type(class) {
        token::BG
    } else if class != Class::Outside {
        token::ACCENT_800
    } else if b == 0 {
        // A zero byte is dimmed: in a multi-megabyte file the padding is most of what you scroll
        // past, and dimming it is what makes the real data findable. Not the design's — the mock
        // had no padding to hide — so it is kept quiet rather than faint.
        theme::muted(45)
    } else {
        token::TEXT
    }
}

/// Whether type on this wash needs to be light.
fn light_type(class: Class) -> bool {
    use gizmo_nfs::inspect::SpanRole;
    matches!(class, Class::Key(SpanRole::Counter | SpanRole::Name))
}

/// What the centre shows before a file is open.
fn empty(app: &PryHub, ui: &mut egui::Ui) {
    let t = app.lang.strings();
    ui.vertical_centered(|ui| {
        ui.add_space(60.0);
        ui.label(RichText::new(t.no_file).color(theme::muted(50)));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_wins_over_region_where_they_overlap() {
        // The header is the first 8 bytes of the chunk's own extent, so a byte can be in both;
        // the stronger layer must win or the header stops being visible.
        let h = Highlights { region: 0x100..0x200, header: 0x100..0x108, key: Vec::new() };
        assert_eq!(h.class_of(0x100), Class::Header);
        assert_eq!(h.class_of(0x108), Class::Region);
        assert_eq!(h.class_of(0x200), Class::Outside);
    }

    #[test]
    fn a_named_field_outranks_the_region_and_the_header() {
        // The counter bytes are inside the chunk's region and may sit inside its header too; the
        // whole point of the layer is that it wins, so the inspector's row and the highlighted
        // bytes are visibly the same fact.
        use gizmo_nfs::inspect::{KeySpan, SpanRole};
        let h = Highlights {
            region: 0x100..0x200,
            header: 0x100..0x108,
            key: vec![KeySpan::new(0x140, 4, SpanRole::Counter)],
        };
        assert_eq!(h.class_of(0x140), Class::Key(SpanRole::Counter));
        assert_eq!(h.class_of(0x144), Class::Region);
    }

    #[test]
    fn a_short_last_row_is_still_a_row() {
        // 17 bytes is two rows, the second holding one byte — an off-by-one here truncates the
        // end of every file.
        assert_eq!(17usize.div_ceil(BYTES_PER_ROW), 2);
        assert_eq!(16usize.div_ceil(BYTES_PER_ROW), 1);
        assert_eq!(0usize.div_ceil(BYTES_PER_ROW), 0);
    }

    #[test]
    fn a_selected_byte_never_wears_the_padding_ink() {
        // Zeros are dimmed only outside the selection: inside it the design gives every byte the
        // same accent ink, and a dimmed zero over the wash all but disappears.
        assert_eq!(ink(Class::Region, 0), token::ACCENT_800);
        assert_eq!(ink(Class::Header, 0), token::ACCENT_800);
        assert_ne!(ink(Class::Outside, 0), ink(Class::Outside, 0x4a));
    }

    #[test]
    fn sizes_read_in_the_largest_unit_that_stays_readable() {
        assert_eq!(fmt_size(64), "64 B");
        assert_eq!(fmt_size(1536), "1.5 KB");
        assert_eq!(fmt_size(3 << 20), "3.00 MB");
    }
}
