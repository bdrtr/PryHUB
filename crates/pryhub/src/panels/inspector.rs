//! The inspector: the selected chunk's parsed fields.
//!
//! The reading is the parser's ([`gizmo_nfs::inspect`]) — this module only draws it. That split is
//! the point: an inspector that re-declared the offsets could disagree with the reader about what
//! a file says, and then the tool would be lying with a straight face.
//!
//! The design builds the pane out of three ruled blocks — identity, fields, matrix — each stating
//! its own padding, with only 2–3 px between the lines *inside* a block. egui's global
//! `item_spacing.y` is the opposite arrangement, so every block here zeroes it and spends its space
//! on a [`egui::Frame`] margin instead.

use crate::app::PryHub;
use crate::i18n::Strings;
use crate::theme::{self, token};
use crate::widget;
use egui::{Color32, Margin, Rect, RichText, Sense, Vec2};
use gizmo_nfs::inspect::{Field, Value};

/// The design's `padding: 6px 0` around the field list.
const LIST_PAD: f32 = 6.0;

/// Draw the inspector for the current selection.
pub fn show(app: &PryHub, ui: &mut egui::Ui) {
    let t = app.lang.strings();
    let (Some(model), Some(node)) = (app.selected_model(), app.selected_node()) else {
        egui::Frame::new().inner_margin(Margin::same(12)).show(ui, |ui| {
            ui.label(RichText::new(t.nothing_selected).color(theme::muted(50)));
        });
        return;
    };

    // One scrolling column under the caption: the identity block travels with the fields it names,
    // as the design has it.
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        header(ui, node, model);
        rule(ui, token::RULE, token::DIVIDER);

        ui.add_space(LIST_PAD);
        for f in &model.fields {
            // A matrix is a grid, not a row — the design gives it its own block below the list.
            if matches!(f.value, Value::Matrix(_)) {
                continue;
            }
            field_row(ui, t, f, node.offset);
        }
        ui.add_space(LIST_PAD);

        for f in &model.fields {
            if let Value::Matrix(m) = &f.value {
                rule(ui, token::RULE, token::DIVIDER);
                matrix_block(ui, t, m);
            }
        }
    });
}

/// The identity block: what kind of chunk this is, which one it is, and where it sits.
///
/// The type is the eyebrow and the name is the title — the other way round from how this pane used
/// to read. Twenty rows of the tree are `SolidObject`; the name is the thing being looked for, and
/// it is the line that should carry the size.
fn header(
    ui: &mut egui::Ui,
    node: &gizmo_nfs::chunk::ChunkNode,
    model: &gizmo_nfs::inspect::ChunkModel,
) {
    let kind = chunk_label(node.header.id);
    egui::Frame::new()
        .inner_margin(Margin { left: 12, right: 12, top: 12, bottom: 10 })
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 3.0;
            ui.add(egui::Label::new(theme::tracked(
                &kind.to_uppercase(),
                0.12,
                theme::font::body(9.0),
                token::ACCENT,
            )));
            ui.label(
                RichText::new(model.summary.clone().unwrap_or_else(|| kind.to_owned()))
                    .font(theme::font::mono(15.0))
                    .color(token::TEXT),
            );
            ui.label(
                RichText::new(format!(
                    "{:#010x} · {}",
                    node.header.id,
                    size_label(u64::from(node.header.size))
                ))
                .font(theme::font::mono(10.5))
                .color(theme::muted(50)),
            );
        });
}

/// One field: its label and offset on one line, the value under them, the note under that.
fn field_row(ui: &mut egui::Ui, t: &Strings, f: &Field, chunk_at: usize) {
    egui::Frame::new()
        .inner_margin(Margin { left: 12, right: 12, top: 6, bottom: 6 })
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(title_case(f.label))
                        .font(theme::font::body(11.5))
                        .color(theme::muted(60)),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    match f.offset {
                        // Chunk-relative, the way a format note is written: `+0x14` is what you
                        // compare against a spec, where an absolute file offset is only ever true
                        // of this one file.
                        Some(off) => {
                            ui.label(
                                RichText::new(format!("+{:#x}", off.saturating_sub(chunk_at)))
                                    .font(theme::font::mono(9.0))
                                    .color(theme::muted(40)),
                            );
                        }
                        // The parser leaves a field it *worked out* without an offset, which is
                        // exactly what the design's `derived` chip marks. The other two chips —
                        // `const` and `suspect` — have no data behind them yet, and inventing one
                        // would be the tool making a claim the reader never made.
                        None => {
                            widget::tag(ui, t.f_derived, widget::Tone::Accent);
                        }
                    }
                });
            });
            ui.label(
                RichText::new(render(&f.value)).font(theme::font::mono(13.0)).color(token::TEXT),
            );
            if let Some(note) = &f.note {
                ui.label(
                    RichText::new(note).font(theme::font::body(10.0)).color(theme::muted(48)),
                );
            }
        });
    // The rule between two rows of one list, not the rule that ends a region.
    rule(ui, token::HAIRLINE, token::DIVIDER_SOFT);
}

/// A rule across the whole pane, taking its own row in the layout.
fn rule(ui: &mut egui::Ui, width: f32, colour: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), width), Sense::hover());
    theme::draw::rule_h(ui.painter(), rect.left()..=rect.right(), rect.top(), width, colour);
}

/// One value, as the inspector shows it.
fn render(value: &Value) -> String {
    match value {
        Value::Num(n) => n.to_string(),
        Value::Hex(h) => format!("{h:#010x}"),
        // The arithmetic, not just the answer: this is the row that catches a packed vertex
        // layout, and it is only checkable by hand if the division is visible.
        Value::Ratio { num, den, value } => format!("{num} / {den} = {value:.1}"),
        Value::Text(s) => s.clone(),
        Value::Float(f) => format!("{f:.4}"),
        Value::Float3(v) => format!("{:.3}, {:.3}, {:.3}", v[0], v[1], v[2]),
        Value::Bytes(b) => b.iter().map(|x| format!("{x:02X}")).collect::<Vec<_>>().join(" "),
        Value::Matrix(_) => String::new(), // drawn as a grid
        _ => String::new(),
    }
}

/// The design's 4×4 matrix block: its own ruled region at the foot of the pane.
fn matrix_block(ui: &mut egui::Ui, t: &Strings, m: &gizmo_nfs::Mat4) {
    egui::Frame::new()
        .inner_margin(Margin { left: 12, right: 12, top: 10, bottom: 10 })
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.add(egui::Label::new(theme::tracked(
                &t.matrix.to_uppercase(),
                0.12,
                theme::font::body(9.0),
                theme::muted(50),
            )));
            ui.add_space(7.0);
            matrix_grid(ui, m);
        });
}

/// The grid itself: cells on the page colour, divided and boxed by a 1 px rule.
fn matrix_grid(ui: &mut egui::Ui, m: &gizmo_nfs::Mat4) {
    const PAD_X: f32 = 4.0;
    const PAD_Y: f32 = 5.0;

    let font = theme::font::mono(10.5);
    let line = ui.painter().layout_no_wrap("0.000".to_owned(), font.clone(), Color32::PLACEHOLDER);
    let cell_h = line.size().y + PAD_Y * 2.0;
    let width = ui.available_width();
    // Five 1 px rules across and five down: the box plus the three gaps between the cells.
    let cell_w = (width - 5.0) / 4.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, cell_h * 4.0 + 5.0), Sense::hover());

    let p = ui.painter();
    p.rect_filled(rect, 0.0_f32, token::DIVIDER);
    for (r, row) in m.iter().enumerate() {
        for (c, v) in row.iter().enumerate() {
            let cell = Rect::from_min_size(
                egui::pos2(
                    rect.left() + 1.0 + c as f32 * (cell_w + 1.0),
                    rect.top() + 1.0 + r as f32 * (cell_h + 1.0),
                ),
                Vec2::new(cell_w, cell_h),
            );
            p.rect_filled(cell, 0.0_f32, token::BG);
            // Lit by *position*, not by value: the diagonal is what makes a matrix the unit
            // matrix, and colouring round numbers instead only says "these happen to be 1 and 0".
            let ink = if r == c { token::ACCENT_700 } else { theme::muted(55) };
            p.text(
                egui::pos2(cell.right() - PAD_X, cell.center().y),
                egui::Align2::RIGHT_CENTER,
                format!("{v:.3}"),
                font.clone(),
                ink,
            );
        }
    }
}

/// The design's `fmtSize`: bytes only while they are still countable by eye.
fn size_label(bytes: u64) -> String {
    match bytes {
        n if n >= 1024 * 1024 => format!("{:.2} MB", n as f64 / (1024.0 * 1024.0)),
        n if n >= 1024 => format!("{:.1} KB", n as f64 / 1024.0),
        n => format!("{n} B"),
    }
}

/// The parser labels its fields in prose (`bytes per vertex`); the design sets them as titles.
/// Fixed here rather than in `gizmo_nfs::inspect` so the reader keeps saying what it reads and the
/// pane keeps deciding how it looks.
fn title_case(label: &str) -> String {
    let mut chars = label.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().collect::<String>() + chars.as_str()
    })
}

/// The panel caption.
pub fn caption(app: &PryHub, ui: &mut egui::Ui) {
    widget::caption_strip(ui, app.lang.strings().p_inspector, |_| {});
}

/// A readable name for a chunk id — the tree's label and the inspector's title. An id nobody has
/// decoded stays "chunk": an invented label would be worse than a number.
#[must_use]
pub fn chunk_label(id: u32) -> &'static str {
    gizmo_nfs::inspect::type_name(id).unwrap_or("chunk")
}
