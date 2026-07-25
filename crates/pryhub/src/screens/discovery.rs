//! The discovery screen: read an undecoded chunk with a layout you type, and see it at once.
//!
//! This is the screen the project plan calls the most differentiating thing the tool can have, and
//! the reason is that the format will never be finished. A fixed parser can only show what it
//! already knows; this shows what a *guess* would say, and lets the guess be wrong cheaply.
//!
//! It owns no format knowledge — [`gizmo_nfs::discover`] reads the bytes, suggests the strides
//! that divide the payload exactly, and makes the timid column guesses. What lives here is the
//! table, and the arithmetic being visible while you type: how many records the stride implies,
//! how many bytes are left over (the number that says a stride is wrong), and how much of each
//! record no column has claimed.
//!
//! The design lays that out as a fixed *pattern* column beside the preview, and assumes the chunk
//! has already been chosen. The real tool has to offer that choice, so the tree keeps a rail of its
//! own to the left of both rather than taking the pattern column's place.

use crate::app::PryHub;
use crate::doc::Doc;
use crate::panels;
use crate::theme::{self, token};
use crate::widget;
use egui::{Align2, Color32, RichText, Sense, Stroke, Vec2};
use gizmo_nfs::discover::{self, Cell, Kind, Schema};

/// The design's `grid-template-columns: 340px 1fr`, and the measure the page is capped to.
const PATTERN_WIDTH: f32 = 340.0;
const PAGE_WIDTH: f32 = 1000.0;
/// `padding:30px 34px 40px`, beside the `max-width` above.
const PAGE_PAD: egui::Margin = egui::Margin { left: 34, right: 34, top: 30, bottom: 40 };

/// Characters the offset gutter occupies: `+0x` plus eight hex digits and a separator.
const OFFSET_WIDTH: usize = 12;

/// The screen's own state: a schema, and which chunk it was built for.
#[derive(Default)]
pub struct State {
    /// Header offset of the chunk the schema describes, so a new selection re-seeds it.
    chunk: Option<usize>,
    pub schema: Schema,
    /// The ranked strides, and what they were ranked for.
    ///
    /// Scoring a candidate means guessing every lane of every record, so doing it per frame cost
    /// 2 ms of a 16 ms budget while nothing was changing. It only depends on the chunk and the
    /// header offset, so it is computed when one of those moves and kept otherwise.
    candidates: Vec<discover::Candidate>,
    ranked_for: Option<(usize, usize)>,
    /// What the reader calls each column, one per `schema.columns`.
    ///
    /// Here rather than in `discover::Schema` because a name is not part of a *reading*: the parser
    /// needs the stride and the kinds to decode a record and could not care less whether lane two is
    /// called `normal`. Keeping it on this side leaves the published crate's shape alone and still
    /// gives the preview table the headers the design puts on it.
    names: Vec<String>,
}

impl State {
    /// Keep one name per column — the two are edited from different places and a mismatch would
    /// silently drop a header or index past the end.
    fn fit_names(&mut self) {
        self.names.resize(self.schema.columns.len(), String::new());
    }
}

/// Draw the screen; returns a chunk offset when the tree was clicked.
pub fn show(app: &mut PryHub, ui: &mut egui::Ui) -> Option<usize> {
    let mut select = None;
    let mut fold: Option<usize> = None;
    let tree_width = app.density.tree_width();
    egui::Panel::left("discovery_tree")
        .resizable(true)
        .default_size(tree_width)
        .frame(egui::Frame::new().fill(token::BG).inner_margin(egui::Margin::symmetric(6, 4)))
        .show_inside(ui, |ui| {
            panels::tree::caption(app, ui);
            match panels::tree::show(app, ui) {
                Some(panels::tree::Hit::Select(offset)) => select = Some(offset),
                // Applied after the panel closure: it is holding the app immutably while it draws.
                Some(panels::tree::Hit::Toggle(offset)) => fold = Some(offset),
                None => {}
            }
        });

    if let Some(offset) = fold {
        super::workspace::fold_container(app, offset);
    }
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(token::BG))
        .show_inside(ui, |ui| {
            // The design's screen is `overflow:auto` and this one was the only page without it, so
            // a stride with a dozen lanes pushed `+ Add field` under the bottom edge with no way to
            // reach it. The table inside keeps its own scrolling and is given a bounded height, or
            // 2,221 rows would make the page as tall as they are.
            let room = ui.available_height();
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                widget::page(ui, PAGE_WIDTH, PAGE_PAD, |ui| body(app, ui, room))
            });
        });
    select
}

fn body(app: &mut PryHub, ui: &mut egui::Ui, room: f32) {
    let t = app.lang.strings();
    // The design states every vertical gap on the block itself — 16 under the title, 10 under a
    // section label, 14 above the field list. A default row gap would be added to all of them.
    ui.spacing_mut().item_spacing.y = 0.0;
    widget::screen_header(ui, t.nav_discovery, Some(t.d_tag), Some(t.d_sub));

    // The payload of the selected chunk, copied out once: the schema reads it every frame, and
    // borrowing the doc through the closures below would fight the mutable app.
    // The document is an `Arc`, so the panel borrows the chunk's bytes out of it rather than
    // copying them per frame — which, with the root selected, meant copying 7.5 MB sixty times a
    // second.
    let Some(doc) = app.doc.clone() else {
        ui.label(RichText::new(t.no_file).color(theme::muted(50)));
        return;
    };
    let Some((chunk_at, label, bytes)) = payload(&doc, app.selection) else {
        ui.label(RichText::new(t.nothing_selected).color(theme::muted(50)));
        return;
    };

    // A new chunk gets a fresh proposal rather than the last chunk's schema, which would be a
    // reading of the wrong file half the time.
    if app.discover.chunk != Some(chunk_at) {
        app.discover.chunk = Some(chunk_at);
        app.discover.schema = discover::propose(bytes);
        app.discover.names.clear();
    }
    app.discover.fit_names();

    // The pattern column keeps its width until the two columns would meet, which is the only point
    // at which a fixed 340 px stops being a measure and starts being a squeeze.
    let left = PATTERN_WIDTH.min(((ui.available_width() - token::SPACE_6) * 0.5).max(200.0));
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = token::SPACE_6;
        // The window's height, measured outside the scroller. Zero was wrong in the other
        // direction: it squeezed the preview table into three rows of the 42,585 it was reporting,
        // because a child cannot lay out taller than the box it was allocated.
        let height = room;
        let column = egui::Layout::top_down(egui::Align::Min);
        ui.allocate_ui_with_layout(Vec2::new(left, height), column, |ui| {
            ui.set_width(left);
            pattern(app, ui, bytes);
        });
        let rest = ui.available_width();
        ui.allocate_ui_with_layout(Vec2::new(rest, height), column, |ui| {
            ui.set_width(rest);
            preview(app, ui, &label, bytes, room);
        });
    });
}

/// The selected chunk: where its header is, what to call it, and its payload — borrowed from the
/// document, not copied.
fn payload(doc: &Doc, selection: Option<usize>) -> Option<(usize, String, &[u8])> {
    let node = selection.and_then(|o| doc.node_at(o))?;
    let label = format!(
        "{:#010x} · {} · {} B",
        node.header.id,
        panels::inspector::chunk_label(node.header.id),
        node.header.size
    );
    Some((node.offset, label, node.data(&doc.bytes)))
}

/// The left column: the stride, the strides that divide exactly, the field list, and the principle
/// the whole screen exists to serve.
fn pattern(app: &mut PryHub, ui: &mut egui::Ui, bytes: &[u8]) {
    let t = app.lang.strings();
    widget::section_label(ui, t.d_pattern);
    ui.add_space(10.0);

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = token::SPACE_2;
        ui.label(RichText::new(t.d_stride).font(theme::font::body(12.0)).color(token::TEXT));
        number(ui, &mut app.discover.schema.stride, 4096);
        ui.label(RichText::new(t.d_bytes_row).font(theme::font::body(11.0)).color(theme::muted(50)));
    });
    ui.add_space(token::SPACE_2);

    // Ranked against the header the user typed, so the suggestions follow the edit rather than
    // the file — moving the header is how someone tests a filler theory.
    let key = (app.discover.chunk.unwrap_or_default(), app.discover.schema.header);
    if app.discover.ranked_for != Some(key) {
        app.discover.candidates = discover::ranked_candidates(bytes, app.discover.schema.header);
        app.discover.ranked_for = Some(key);
    }
    let candidates = app.discover.candidates.clone();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().button_padding = Vec2::new(5.0, 3.0);
        ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
        ui.label(RichText::new(t.d_candidates).font(theme::font::body(11.0)).color(theme::muted(60)));
        if candidates.is_empty() {
            ui.label(RichText::new("—").size(11.0).color(theme::muted(40)));
        }
        for c in candidates.iter().filter(|c| c.records >= 3).take(10) {
            let on = c.stride == app.discover.schema.stride;
            let text = RichText::new(format!("{}×{} · {}/{}", c.stride, c.records, c.explained, c.lanes))
                .font(theme::font::mono(10.5))
                .color(if on { token::ACCENT } else { theme::muted(60) });
            if ui.add(egui::Button::new(text).frame(on)).on_hover_text(t.d_candidate_hint).clicked() {
                app.discover.schema.stride = c.stride;
                app.discover.schema.columns = discover::guess_columns(bytes, app.discover.schema.header, c.stride);
            }
        }
    });
    ui.add_space(token::SPACE_2);

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = token::SPACE_2;
        ui.label(RichText::new(t.d_header).font(theme::font::body(12.0)).color(token::TEXT));
        number(ui, &mut app.discover.schema.header, bytes.len());
        if widget::button_secondary(ui, t.d_guess).on_hover_text(t.d_guess_hint).clicked() {
            let s = &mut app.discover.schema;
            s.columns = discover::guess_columns(bytes, s.header, s.stride);
        }
    });
    ui.add_space(token::SPACE_1);

    // The record count belongs to the preview's heading now; what stays here is the pair of numbers
    // that judge the *pattern* — a remainder says the stride is wrong, an unclaimed tail says the
    // record is only half read.
    let shape = discover::shape(bytes.len(), &app.discover.schema);
    ui.horizontal(|ui| {
        let mono = theme::font::mono(11.0);
        // A remainder is the whole point: it is what says the stride is wrong, so it is loud.
        let (colour, weight) = if shape.remainder == 0 {
            (theme::muted(45), false)
        } else {
            (token::ACCENT, true)
        };
        let left = RichText::new(format!("{} B {}", shape.remainder, t.d_left))
            .font(mono.clone())
            .color(colour);
        ui.label(if weight { left.strong() } else { left });
        if shape.unclaimed > 0 {
            ui.label(
                RichText::new(format!("· {} B {}", shape.unclaimed, t.d_unclaimed))
                    .font(mono)
                    .color(theme::muted(45)),
            );
        }
    });
    ui.add_space(14.0);

    fields(app, ui);

    ui.add_space(10.0);
    let width = ui.available_width();
    let add = ui
        .scope(|ui| {
            // `.btn-secondary` is transparent until the pointer arrives; egui's own inactive button
            // is a filled surface, so the three states are stated rather than inherited.
            let w = &mut ui.style_mut().visuals.widgets;
            w.inactive.weak_bg_fill = Color32::TRANSPARENT;
            w.hovered.weak_bg_fill = token::WASH_HOVER;
            w.active.weak_bg_fill = token::WASH_PRESS;
            let edge = Stroke::new(token::HAIRLINE, token::DIVIDER);
            w.inactive.bg_stroke = edge;
            w.hovered.bg_stroke = edge;
            w.active.bg_stroke = edge;
            ui.spacing_mut().button_padding = Vec2::new(token::SPACE_3 * 1.2, token::SPACE_2);
            ui.add(
                egui::Button::new(
                    RichText::new(format!("+ {}", t.d_add_field))
                        .font(theme::font::heading(12.0))
                        .color(token::TEXT),
                )
                .min_size(Vec2::new(width, 32.0)),
            )
        })
        .inner;
    if add.on_hover_text(t.d_add).clicked() {
        app.discover.schema.columns.push(Kind::Hex);
        app.discover.fit_names();
    }

    ui.add_space(token::SPACE_4);
    // In its own scope: the note paints its accent edge over the `min_rect` of the ui it is given,
    // which up here would be the whole column.
    ui.scope(|ui| widget::note_box(ui, t.d_hint));
}

/// The design's `.input` around a number: a surface box with a hairline border, 88 × 32.
fn number(ui: &mut egui::Ui, value: &mut usize, max: usize) {
    ui.scope(|ui| {
        // A `DragValue` takes its face from the style, and in egui 0.34 a button's text falls back
        // to `TextStyle::Body` rather than to `Button` — `override_font_id` is the only lever that
        // reaches it.
        ui.style_mut().override_font_id = Some(theme::font::mono(14.0));
        ui.spacing_mut().button_padding = Vec2::new(10.0, 6.0);
        ui.add_sized([88.0, 32.0], egui::DragValue::new(value).range(0..=max).speed(1.0));
    });
}

/// The bordered list of fields: one row per column, each saying where it starts and how it is read.
fn fields(app: &mut PryHub, ui: &mut egui::Ui) {
    let list_width = ui.available_width();
    // The margin is the border's own width, so the rows sit flush inside it instead of over it.
    egui::Frame::new()
        .stroke(Stroke::new(token::HAIRLINE, token::DIVIDER))
        .inner_margin(egui::Margin::same(1))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let row_width = list_width - 2.0 - 18.0;
            let count = app.discover.schema.columns.len();
            let mut remove = None;
            for i in 0..count {
                if field_row(app, ui, i, row_width, i + 1 < count) {
                    remove = Some(i);
                }
            }
            if let Some(i) = remove {
                app.discover.schema.columns.remove(i);
                if i < app.discover.names.len() {
                    app.discover.names.remove(i);
                }
            }
        });
}

/// One field row: its offset inside the record, its type, and the × that drops it. Returns whether
/// the × was clicked.
///
/// The design's row is `+0x0 | name | type | ×`, and the name is what the preview table puts at the
/// head of the column — which is the whole point of typing one.
fn field_row(app: &mut PryHub, ui: &mut egui::Ui, i: usize, width: f32, rule: bool) -> bool {
    let t = app.lang.strings();
    let mut delete = false;
    const ROW: f32 = 22.0;
    const BADGE: f32 = 34.0;
    const DELETE: f32 = 20.0;

    egui::Frame::new()
        .fill(token::SURFACE)
        .inner_margin(egui::Margin { left: 9, right: 9, top: 7, bottom: 7 })
        .show(ui, |ui| {
            ui.set_min_width(width);
            ui.spacing_mut().item_spacing.x = token::SPACE_2;
            ui.spacing_mut().button_padding = Vec2::new(5.0, 3.0);
            ui.spacing_mut().interact_size.y = ROW;
            ui.horizontal(|ui| {
                let at = format!("+0x{:X}", app.discover.schema.column_offset(i));
                let galley =
                    ui.painter().layout_no_wrap(at, theme::font::mono(10.0), Color32::PLACEHOLDER);
                let (rect, _) = ui.allocate_exact_size(
                    Vec2::new(BADGE.max(galley.size().x), ROW),
                    Sense::hover(),
                );
                let y = rect.center().y - galley.size().y * 0.5;
                ui.painter().galley(egui::pos2(rect.left(), y), galley, token::ACCENT);

                // What is left after the badge and the ×, split the way the design splits it:
                // the name takes rather more than half, the type the rest.
                let free = (width - rect.width() - token::SPACE_2 * 3.0 - DELETE).max(120.0);
                let name_w = free * 0.58;
                let combo = free - name_w;
                let name = app.discover.names.get_mut(i).expect("one name per column");
                widget::input(ui, name, name_w, t.d_name, true);

                let mut kind = app.discover.schema.columns[i];
                egui::ComboBox::from_id_salt(("discovery_field", i))
                    .width(combo)
                    .selected_text(
                        RichText::new(kind.label()).font(theme::font::mono(11.0)).color(token::TEXT),
                    )
                    .show_ui(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        for k in Kind::all() {
                            ui.selectable_value(
                                &mut kind,
                                *k,
                                RichText::new(k.label()).font(theme::font::mono(11.0)),
                            );
                        }
                    })
                    .response
                    .on_hover_text(t.d_cycle);
                app.discover.schema.columns[i] = kind;

                let (rect, resp) =
                    ui.allocate_exact_size(Vec2::new(DELETE, ROW), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(rect, 0.0_f32, token::WASH_HOVER);
                }
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    "×",
                    theme::font::mono(14.0),
                    token::ACCENT,
                );
                delete = resp.clicked();
                resp.on_hover_cursor(egui::CursorIcon::PointingHand);
            });
        });

    // Every row but the last is closed by a rule; the list's own border closes that one.
    if rule {
        let rect = ui.min_rect();
        theme::draw::rule_h(
            ui.painter(),
            rect.left()..=rect.right(),
            ui.cursor().top() - token::HAIRLINE,
            token::HAIRLINE,
            token::DIVIDER,
        );
    }
    delete
}

/// The right column: what the pattern makes of the bytes, and the arithmetic in one line above it.
fn preview(app: &mut PryHub, ui: &mut egui::Ui, label: &str, bytes: &[u8], room: f32) {
    let t = app.lang.strings();
    let shape = discover::shape(bytes.len(), &app.discover.schema);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = token::SPACE_2;
        widget::section_label(ui, t.d_preview);
        ui.label(
            RichText::new(format!(
                "{} {} → {} {}",
                t.d_stride, app.discover.schema.stride, shape.records, t.d_rows.of(shape.records)
            ))
            .font(theme::font::mono(10.5))
            .color(token::ACCENT),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add(
                egui::Label::new(
                    RichText::new(label).font(theme::font::mono(10.5)).color(theme::muted(45)),
                )
                .truncate(),
            );
        });
    });
    ui.add_space(10.0);
    table(app, ui, bytes, room);
}

/// The decoded table: a header row of column-type buttons over the records.
fn table(app: &mut PryHub, ui: &mut egui::Ui, bytes: &[u8], room: f32) {
    let shape = discover::shape(bytes.len(), &app.discover.schema);
    let mono = theme::font::mono(11.5);
    let head = theme::font::mono(10.0);

    egui::Frame::new()
        .stroke(Stroke::new(token::HAIRLINE, token::DIVIDER))
        .inner_margin(egui::Margin::same(1))
        .show(ui, |ui| {
            let width = ui.available_width();
            // Every cell is padded to a fixed number of characters, so the header lines up with the
            // body without a table widget — which is also why the header, set two sizes smaller
            // than the body, is given the body's character width to sit in rather than its own.
            let probe = ui.painter().layout_no_wrap(
                "0".repeat(16),
                mono.clone(),
                Color32::PLACEHOLDER,
            );
            let char_width = probe.size().x / 16.0;
            let row_height = probe.size().y + 10.0;

            ui.spacing_mut().item_spacing = Vec2::ZERO;
            // The box may not grow past the column it sits in. The design puts the whole table in
            // an `overflow:auto` div, so a stride with a dozen lanes scrolls *inside* the border;
            // here the header's cells were laid out one after another with nothing to stop them, so
            // the box grew, pushed through the page's right edge and was clipped by the window.
            ui.set_max_width(width);

            // The header strip is reserved now and filled at the end: it has to be drawn at the
            // body's horizontal scroll offset to stay over its own columns, and that offset is not
            // known until the body has been drawn. Reserving it keeps the layout in reading order
            // while the painting happens in dependency order.
            let head_h = head.size + 12.0;
            let (head_rect, _) =
                ui.allocate_exact_size(Vec2::new(width, head_h), Sense::hover());

            theme::draw::rule_h(
                ui.painter(),
                head_rect.left()..=head_rect.right(),
                ui.cursor().top(),
                token::RULE,
                token::DIVIDER,
            );
            ui.add_space(token::RULE);

            let schema = &app.discover.schema;
            // A ceiling rather than the remaining height: the page scrolls now, so "the rest" is as
            // tall as the content wants to be. Measured from the panel the page sits in, so the
            // table fills the window without ever deciding how long the page is.
            let body_h = (room - 250.0).clamp(220.0, 900.0);
            let scrolled = egui::ScrollArea::both()
                .auto_shrink([false, false])
                .max_height(body_h.min(shape.records as f32 * row_height + 4.0))
                .show_rows(
                ui,
                row_height,
                shape.records,
                |ui, range| {
                    for index in range {
                        let mut job = egui::text::LayoutJob::default();
                        // The offset recedes and the values carry the ink: the design sets the
                        // cells at weight 700 against a neutral gutter, and one monospace face has
                        // no second weight to say it with.
                        job.append(
                            &format!(
                                "{:<width$}",
                                format!("+0x{:X}", discover::row_offset(schema, index)),
                                width = OFFSET_WIDTH
                            ),
                            0.0,
                            format_of(&mono, token::NEUTRAL_600),
                        );
                        let mut values = String::new();
                        for (cell, kind) in
                            discover::row(bytes, schema, index).iter().zip(&schema.columns)
                        {
                            values.push_str(&format!(
                                "{:<width$}",
                                render(cell),
                                width = cell_width(*kind)
                            ));
                        }
                        job.append(&values, 0.0, format_of(&mono, token::TEXT));

                        let galley = ui.painter().layout_job(job);
                        // The rule under a row runs the full measure, so a row is never narrower
                        // than the viewport — and never wider than it for nothing, which is what
                        // would put a horizontal scrollbar under a table that fits.
                        let row =
                            Vec2::new((galley.size().x + 20.0).max(ui.available_width()), row_height);
                        let (rect, _) = ui.allocate_exact_size(row, Sense::hover());
                        let p = ui.painter();
                        p.galley(
                            egui::pos2(rect.left() + 10.0, rect.top() + 5.0),
                            galley,
                            token::TEXT,
                        );
                        theme::draw::rule_h(
                            p,
                            rect.left()..=rect.right(),
                            rect.bottom() - token::HAIRLINE,
                            token::HAIRLINE,
                            token::DIVIDER_SOFT,
                        );
                    }
                },
            );

            // …and now the header, shifted by what the body is showing and clipped to its strip, so
            // it scrolls with the columns instead of sliding out of the box.
            let dx = scrolled.state.offset.x;
            let mut strip = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(head_rect.translate(Vec2::new(-dx, 0.0)))
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            strip.set_clip_rect(head_rect.intersect(ui.clip_rect()));
            strip.spacing_mut().item_spacing = Vec2::ZERO;
            egui::Frame::new()
                .inner_margin(egui::Margin { left: 10, right: 10, top: 6, bottom: 6 })
                .show(&mut strip, |ui| header(app, ui, &head, char_width));
        });
}

/// The table's `th` row. The type is still cycled by clicking its header, so the accent is kept for
/// the hover that says so rather than spent on the resting label.
fn header(app: &mut PryHub, ui: &mut egui::Ui, font: &egui::FontId, char_width: f32) {
    let t = app.lang.strings();
    ui.horizontal(|ui| {
        let height = ui.painter().layout_no_wrap("0".to_owned(), font.clone(), Color32::PLACEHOLDER).size().y;
        let cell = |ui: &mut egui::Ui, text: String, chars: usize, sense: Sense| {
            let galley = ui.painter().layout_no_wrap(text, font.clone(), Color32::PLACEHOLDER);
            let (rect, resp) =
                ui.allocate_exact_size(Vec2::new(chars as f32 * char_width, height), sense);
            (rect, resp, galley)
        };

        let (rect, _, galley) =
            cell(ui, t.d_offset.to_uppercase(), OFFSET_WIDTH, Sense::hover());
        ui.painter().galley(rect.left_top(), galley, theme::muted(60));

        let mut cycle = None;
        for (i, kind) in app.discover.schema.columns.iter().enumerate() {
            // The name if one was typed, else what the lane is being read as — a column with no
            // name still has to say something, and its type is the truest thing available.
            let head = match app.discover.names.get(i) {
                Some(n) if !n.trim().is_empty() => n.trim().to_uppercase(),
                _ => kind.label().to_uppercase(),
            };
            let (rect, resp, galley) = cell(ui, head, cell_width(*kind), Sense::click());
            let ink = if resp.hovered() { token::ACCENT_600 } else { theme::muted(60) };
            ui.painter().galley(rect.left_top(), galley, ink);
            if resp.on_hover_text(t.d_cycle).clicked() {
                cycle = Some(i);
            }
        }
        if let Some(i) = cycle {
            if let Some(k) = app.discover.schema.columns.get_mut(i) {
                *k = k.next();
            }
        }
    });
}

/// One run of the table in a colour — the only way to give a row two inks without two galleys.
fn format_of(font: &egui::FontId, colour: Color32) -> egui::TextFormat {
    egui::TextFormat { font_id: font.clone(), color: colour, ..Default::default() }
}

/// Characters a column occupies, including the space that separates it from the next.
fn cell_width(kind: Kind) -> usize {
    match kind {
        Kind::U8 | Kind::I8 => 5,
        Kind::U16 | Kind::I16 => 7,
        Kind::Char4 => 6,
        Kind::Raw4 => 13,
        Kind::F32 => 13,
        _ => 12,
    }
}

fn render(cell: &Cell) -> String {
    match cell {
        Cell::Uint(v) => format!("{v} "),
        Cell::Int(v) => format!("{v} "),
        Cell::Hex(v) => format!("{v:08X} "),
        Cell::Float(v) => format!("{v:.4} "),
        Cell::Text(s) => format!("{s} "),
        Cell::Bytes(b) => format!("{:02X}{:02X}{:02X}{:02X} ", b[0], b[1], b[2], b[3]),
        Cell::Missing => "· ".to_string(),
        // `Cell` is `#[non_exhaustive]`: a kind added to the parser must show as something rather
        // than stop this crate compiling.
        _ => "? ".to_string(),
    }
}
