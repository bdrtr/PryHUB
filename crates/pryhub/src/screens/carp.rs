//! The CARP screen — the design's car-parameter table, over what this install actually holds.
//!
//! # Why most of it is empty, and why that is the feature
//!
//! The design draws CARP as a full handling editor: nine sections, thirty-five parameters, four
//! upgrade levels each, editable, with a live torque curve and a Save button. It labels the source
//! `CARP.BIN` and notes that the format is flat, so "the first write feature lands here".
//!
//! **NFSU2 ships no `CARP.BIN`.** `find -iname '*carp*'` over a full install returns nothing; CARP
//! is the older NFS engine's format. What this game does carry, and what this crate can read, is the
//! `CarTypeInfo` record in `GLOBAL/GLOBALB.BUN` — 46 of them, with the car's name, its wheel mounts,
//! wheel radius and its **mass**. That is two of the design's thirty-five parameters.
//!
//! So the screen is built exactly as drawn and then tells the truth cell by cell: what GLOBALB
//! answers is shown with its source named, and what nothing here answers is drawn in the design's
//! disabled treatment rather than filled with a plausible number. This is the same choice the export
//! dialog makes for its DDS row, and the same rule the validation screen counts by — a rule that
//! read nothing is not a pass. A screen full of invented gear ratios would look far more finished
//! and would be worth nothing.
//!
//! The upgrade levels are drawn as the design's four columns, and only `STOCK` is filled today.
//!
//! # What has since been found, and is not read here yet
//!
//! `GLOBALB.BUN`'s `0x00034600` is a per-car physics record — `8 + 46 × 2192 == 100,840`, the
//! chunk's size exactly, one per `CarTypeInfo` car and in the same order. It carries the engine's
//! rpm limits at `+0x300`, a 9-point torque curve at `+0x310` that rises *and falls* in all 46 cars,
//! and four 64-byte gearbox blocks at `+0x2C0`/`+0x460`/`+0x4A0`/`+0x4E0` — which are the design's
//! STOCK and three upgrade columns. So most of this table is readable and simply is not read yet:
//! [`Source`] has no variant for it and `gizmo-nfs` has no parser for it.
//!
//! That is why the empty cells say *"this screen does not read it yet"* and not *"not located"*. The
//! distinction is the whole point of the screen, and it was wrong here for one commit's worth of
//! time — the note was written before the record was found and had to be corrected rather than left
//! to age into a lie.
//!
//! `profile` settled a neighbouring question from the other end: the game's *displayed* torque and
//! power are computed at run time and appear in no static asset at any scale. The curve at `+0x310`
//! is normalised magnitudes with no rpm axis stored beside it, which is consistent with that.

use crate::app::PryHub;
use crate::theme::{self, token};
use crate::widget;
use egui::{RichText, Ui};

/// Where a cell's value comes from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Source {
    /// The car's name in its `CarTypeInfo` record.
    CarName,
    /// Stock mass in kilograms, from the same record.
    MassKg,
    /// Nothing in this install answers it. See the module note.
    NotLocated,
}

/// One row of a section: the design's own key, label and unit.
struct Param {
    label: &'static str,
    unit: &'static str,
    source: Source,
}

/// One `[::SECTION]` of the design's table.
struct Section {
    name: &'static str,
    params: &'static [Param],
}

/// A parameter with no source, which is all but two of them.
const fn gap(label: &'static str, unit: &'static str) -> Param {
    Param { label, unit, source: Source::NotLocated }
}

/// The design's nine sections, verbatim — including the seven this install cannot fill. They are
/// listed rather than dropped because "the format has a gearbox section and we cannot read it" is a
/// different statement from "there is no gearbox", and only the first one is true.
static SECTIONS: &[Section] = &[
    Section {
        name: "[::VEHICLE]",
        params: &[
            Param { label: "car_name", unit: "", source: Source::CarName },
            gap("car_class", ""),
            gap("drive_type", ""),
        ],
    },
    Section {
        name: "[::MASS]",
        params: &[
            Param { label: "mass", unit: "kg", source: Source::MassKg },
            gap("weight_bias_f", "%"),
            gap("cg_height", "m"),
        ],
    },
    Section {
        name: "[::ENGINE]",
        params: &[
            gap("idle_rpm", "rpm"),
            gap("max_rpm", "rpm"),
            gap("red_line", "rpm"),
            gap("torque_scale", "×"),
        ],
    },
    Section {
        name: "[::TORQUE_CURVE]",
        params: &[
            gap("trq_1000", "Nm"),
            gap("trq_2000", "Nm"),
            gap("trq_3000", "Nm"),
            gap("trq_4000", "Nm"),
            gap("trq_5000", "Nm"),
            gap("trq_6000", "Nm"),
            gap("trq_7000", "Nm"),
            gap("trq_8000", "Nm"),
        ],
    },
    Section {
        name: "[::GEARBOX]",
        params: &[
            gap("gear_count", ""),
            gap("final_drive", ":1"),
            gap("gear_1", ":1"),
            gap("gear_2", ":1"),
            gap("gear_3", ":1"),
            gap("gear_4", ":1"),
            gap("gear_5", ":1"),
            gap("gear_6", ":1"),
            gap("shift_time", "s"),
        ],
    },
    Section {
        name: "[::TIRES]",
        params: &[
            gap("grip_front", "g"),
            gap("grip_rear", "g"),
            gap("slip_angle", "°"),
            gap("tire_width", "mm"),
        ],
    },
    Section {
        name: "[::AERO]",
        params: &[gap("drag_coef", "Cd"), gap("downforce_f", "N"), gap("downforce_r", "N")],
    },
    Section {
        name: "[::BRAKES]",
        params: &[gap("brake_force", "×"), gap("brake_bias_f", ""), gap("handbrake", "×")],
    },
    Section { name: "[::STEERING]", params: &[gap("steer_lock", "°"), gap("steer_speed", "×")] },
];

/// The design's four upgrade columns.
const LEVELS: [&str; 4] = ["STOCK", "L1", "L2", "L3"];

/// Which section is open and which level is highlighted.
#[derive(Clone, Copy, Default)]
pub struct State {
    /// Index into [`SECTIONS`].
    pub section: usize,
    /// Index into [`LEVELS`]; only `0` can ever carry a value.
    pub level: usize,
}

/// How many of the design's parameters this install can actually answer.
#[must_use]
pub fn located() -> usize {
    SECTIONS
        .iter()
        .flat_map(|s| s.params.iter())
        .filter(|p| p.source != Source::NotLocated)
        .count()
}

/// Every parameter the design draws.
#[must_use]
pub fn total() -> usize {
    SECTIONS.iter().map(|s| s.params.len()).sum()
}

/// The two facts this screen can get out of a `CarTypeInfo`.
///
/// Taken as a small copy rather than a borrow of the record, so the cell logic is a pure function
/// over values and can be tested without building a parser struct — `CarTypeInfo` is
/// `#[non_exhaustive]`, which is the published crate correctly refusing to be faked.
#[derive(Clone, Copy)]
struct Facts<'a> {
    name: &'a str,
    mass_kg: f32,
}

/// The value for one cell, or `None` when nothing here answers it.
///
/// `level` is the design's upgrade column. Only `STOCK` is ever answerable: a `CarTypeInfo` record
/// is the car as it ships, and the tables that would say what an upgrade does to it have not been
/// found. Returning `None` for `L1..L3` is therefore not a stub — it is the measurement.
fn cell(param: &Param, facts: Option<Facts<'_>>, level: usize) -> Option<String> {
    let facts = facts?;
    if level != 0 {
        return None;
    }
    match param.source {
        Source::CarName => Some(facts.name.to_owned()),
        Source::MassKg => Some(format!("{:.0}", facts.mass_kg)),
        Source::NotLocated => None,
    }
}

/// Draw the screen.
///
/// Everything is inside a `CentralPanel` whose frame fills [`token::BG`], the same way
/// [`super::workspace`] does it. Without that fill the region is never painted and the window shows
/// a dark wash instead — the screen came out **black** below its two panels the first time it was
/// run.
///
/// `--shot` caught it and it was still missed: the PNG held `RGBA(8, 8, 8, 180)` across the whole
/// unfilled area, exactly the black that was on screen, and it was read as white off the preview.
/// So the lesson is not that a screenshot cannot see this — it saw it perfectly. It is that a
/// screenshot has to be *checked* rather than glanced at, and the cheap check is a pixel: filled
/// background here is `token::BG`, opaque, and anything semi-transparent means a region nobody
/// painted.
pub fn show(app: &mut PryHub, ui: &mut Ui) {
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(token::BG))
        .show_inside(ui, |ui| body(app, ui));
}

fn body(app: &mut PryHub, ui: &mut Ui) {
    app.want_car_spec();
    let t = app.lang.strings();
    let d = theme::density_of(ui.ctx());
    // `Some(None)` is "asked, and the answer was no" — the two are drawn differently below.
    // Copied out rather than borrowed: the panels below take `&mut app`, and two of the record's
    // fields are cheaper to own than to fight the borrow checker over.
    let owned: Option<(String, f32)> = app
        .car_spec
        .as_ref()
        .and_then(|o| o.as_deref())
        .map(|s| (s.name.clone(), s.mass_kg));
    let asked = app.car_spec.is_some();
    let facts = owned.as_ref().map(|(name, mass)| Facts { name, mass_kg: *mass });

    header(app, ui, t, d);

    let full = ui.available_rect_before_wrap();
    ui.horizontal_top(|ui| {
        ui.set_min_height(full.height());
        sections_panel(app, ui, t, d);
        widget::rule_v(ui, full.height(), token::DIVIDER);
        table_panel(app, ui, t, d, facts, asked);
    });
}

/// The row the design puts above everything: what is being read, and the level switch.
fn header(app: &mut PryHub, ui: &mut Ui, t: &crate::i18n::Strings, d: theme::Density) {
    egui::Frame::new()
        .fill(token::SURFACE)
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("CARP.BIN")
                        .font(theme::font::mono(d.mono_size()))
                        .color(token::TEXT)
                        .strong(),
                );
                ui.label(RichText::new("→").color(token::ACCENT).strong());
                widget::tag(ui, "CARP parser", widget::Tone::Accent);
                ui.label(
                    RichText::new(t.cp_not_chunk)
                        .font(theme::font::body(d.small_size()))
                        .color(theme::muted(50)),
                );
                widget::rule_v(ui, 20.0, token::DIVIDER);
                ui.label(
                    RichText::new(t.cp_levels)
                        .font(theme::font::body(d.small_size()))
                        .color(theme::muted(55)),
                );
                let mut level = app.carp.level;
                let items: Vec<(usize, &str)> =
                    LEVELS.iter().enumerate().map(|(i, n)| (i, *n)).collect();
                if widget::segmented(ui, egui::Id::new("carp-level"), &mut level, &items, widget::Seg::Small) {
                    app.carp.level = level;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Drawn, and disabled: there is no CARP.BIN to write back to. The design's own
                    // disabled treatment, the same one the export dialog gives its DDS row.
                    ui.add_enabled_ui(false, |ui| {
                        let _ = widget::button_primary(ui, t.cp_save);
                        let _ = widget::button_secondary(ui, t.cp_revert);
                    });
                    ui.label(
                        RichText::new(t.cp_no_changes)
                            .font(theme::font::mono(d.small_size()))
                            .color(theme::muted(45)),
                    );
                });
            });
        });
    let rect = ui.max_rect();
    theme::draw::rule_h(
        ui.painter(),
        rect.left()..=rect.right(),
        ui.cursor().top(),
        token::RULE,
        token::DIVIDER,
    );
    ui.add_space(token::RULE);
}

/// The left column: the nine sections, and the note that says what this screen is.
fn sections_panel(app: &mut PryHub, ui: &mut Ui, t: &crate::i18n::Strings, d: theme::Density) {
    ui.vertical(|ui| {
        ui.set_width(d.tree_width());
        widget::caption_strip(ui, t.cp_sections, |ui| {
            ui.label(
                RichText::new(format!("{} · {}p", SECTIONS.len(), total()))
                    .font(theme::font::mono(d.small_size()))
                    .color(theme::muted(45)),
            );
        });
        egui::ScrollArea::vertical().id_salt("carp-sections").max_height(ui.available_height() - 96.0).show(
            ui,
            |ui| {
                for (i, section) in SECTIONS.iter().enumerate() {
                    let on = i == app.carp.section;
                    let filled = section
                        .params
                        .iter()
                        .filter(|p| p.source != Source::NotLocated)
                        .count();
                    let (rect, resp) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), d.row_height()),
                        egui::Sense::click(),
                    );
                    if on {
                        ui.painter().rect_filled(rect, 0.0, token::ACCENT_100);
                        ui.painter().rect_filled(
                            egui::Rect::from_min_size(rect.min, egui::vec2(2.0, rect.height())),
                            0.0,
                            token::ACCENT,
                        );
                    } else if resp.hovered() {
                        ui.painter().rect_filled(rect, 0.0, token::SURFACE);
                    }
                    let ink = if on { token::ACCENT_800 } else { token::TEXT };
                    ui.painter().text(
                        rect.left_center() + egui::vec2(10.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        section.name,
                        theme::font::mono(d.mono_size()),
                        ink,
                    );
                    // A filled dot marks a section this install can say anything at all about.
                    let mut right = rect.right() - 8.0;
                    ui.painter().text(
                        egui::pos2(right, rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        format!("{}p", section.params.len()),
                        theme::font::mono(d.small_size() - 1.0),
                        theme::muted(38),
                    );
                    right -= 26.0;
                    if filled > 0 {
                        ui.painter().circle_filled(
                            egui::pos2(right, rect.center().y),
                            3.0,
                            token::ACCENT,
                        );
                    }
                    if resp.clicked() {
                        app.carp.section = i;
                    }
                }
            },
        );
        ui.add_space(token::SPACE_2);
        theme::draw::rule_h(
            ui.painter(),
            ui.max_rect().left()..=ui.max_rect().right(),
            ui.cursor().top(),
            token::RULE,
            token::DIVIDER,
        );
        ui.add_space(token::RULE + token::SPACE_2);
        widget::note_box(ui, t.cp_write_note);
    });
}

/// The right column: the parameter table for the open section.
fn table_panel(
    app: &mut PryHub,
    ui: &mut Ui,
    t: &crate::i18n::Strings,
    d: theme::Density,
    facts: Option<Facts<'_>>,
    asked: bool,
) {
    let section = &SECTIONS[app.carp.section.min(SECTIONS.len() - 1)];
    ui.vertical(|ui| {
        widget::caption_strip(ui, t.cp_raw, |ui| {
            ui.label(
                RichText::new(t.cp_located_of)
                    .font(theme::font::body(d.small_size()))
                    .color(theme::muted(45)),
            );
            ui.label(
                RichText::new(format!("{}/{}", located(), total()))
                    .font(theme::font::mono(d.small_size()))
                    .color(token::ACCENT),
            );
        });
        ui.add_space(token::SPACE_2);
        ui.horizontal(|ui| {
            ui.add_space(token::SPACE_3);
            ui.label(
                RichText::new(section.name)
                    .font(theme::font::mono(d.mono_size()))
                    .color(token::TEXT)
                    .strong(),
            );
            ui.add_space(token::SPACE_2);
            ui.label(
                RichText::new(t.cp_raw_sub)
                    .font(theme::font::body(d.small_size()))
                    .color(theme::muted(50)),
            );
        });
        ui.add_space(token::SPACE_2);

        // Where the record came from, or why there is none — the screen must distinguish "no
        // install" from "an install with no record for this car".
        let note = match (asked, facts) {
            (_, Some(f)) => format!("{} · {}", t.cp_from_globalb, f.name),
            (true, None) => t.cp_no_install.to_owned(),
            (false, None) => String::new(),
        };
        if !note.is_empty() {
            ui.horizontal(|ui| {
                ui.add_space(token::SPACE_3);
                widget::tag(
                    ui,
                    &note,
                    if facts.is_some() { widget::Tone::Accent } else { widget::Tone::Neutral },
                );
            });
            ui.add_space(token::SPACE_2);
        }

        egui::ScrollArea::vertical().id_salt("carp-table").show(ui, |ui| {
            egui::Grid::new("carp-grid")
                .num_columns(2 + LEVELS.len())
                .spacing(egui::vec2(6.0, 5.0))
                .min_col_width(56.0)
                .show(ui, |ui| {
                    ui.label("");
                    ui.label("");
                    for (i, name) in LEVELS.iter().enumerate() {
                        let ink =
                            if i == app.carp.level { token::ACCENT } else { theme::muted(45) };
                        ui.label(theme::tracked(name, 0.1, theme::font::mono(9.5), ink));
                    }
                    ui.end_row();

                    for param in section.params {
                        ui.label(
                            RichText::new(param.label)
                                .font(theme::font::mono(d.mono_size()))
                                .color(token::TEXT),
                        );
                        ui.label(
                            RichText::new(param.unit)
                                .font(theme::font::mono(d.small_size() - 1.5))
                                .color(theme::muted(42)),
                        );
                        for level in 0..LEVELS.len() {
                            match cell(param, facts, level) {
                                Some(v) => {
                                    ui.label(
                                        RichText::new(v)
                                            .font(theme::font::mono(d.mono_size()))
                                            .color(token::TEXT)
                                            .strong(),
                                    );
                                }
                                None => {
                                    ui.label(
                                        RichText::new("—")
                                            .font(theme::font::mono(d.mono_size()))
                                            .color(theme::muted(30)),
                                    )
                                    .on_hover_text(t.cp_not_located);
                                }
                            }
                        }
                        ui.end_row();
                    }
                });
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The design's table, counted. If a section is ever dropped to make the screen look fuller,
    /// this is what says so.
    #[test]
    fn the_screen_carries_the_designs_whole_table() {
        assert_eq!(SECTIONS.len(), 9, "the design draws nine sections");
        // 39, and the torque curve is 8 of them: the design generates that section from its
        // own `rpmSteps` (1000..8000 in thousands), which is easy to miscount by hand — this
        // assertion is here because it already caught one.
        assert_eq!(total(), 39, "and thirty-nine parameters across them");
        assert_eq!(SECTIONS[3].params.len(), 8, "the torque curve is one row per rpm step");
    }

    /// Exactly two parameters have a source, and both are in the record this crate can read. The
    /// number is asserted so that adding a third is a deliberate act with a test to update, and so
    /// that quietly wiring one to a guess fails here.
    #[test]
    fn only_what_globalb_answers_is_claimed() {
        assert_eq!(located(), 2, "car_name and mass, and nothing else");
        let named: Vec<&str> = SECTIONS
            .iter()
            .flat_map(|s| s.params.iter())
            .filter(|p| p.source != Source::NotLocated)
            .map(|p| p.label)
            .collect();
        assert_eq!(named, vec!["car_name", "mass"]);
    }

    /// Without a record nothing is filled in, and with one only the stock column is.
    #[test]
    fn upgrade_levels_are_never_invented() {
        let mass = &SECTIONS[1].params[0];
        assert_eq!(cell(mass, None, 0), None, "no record, no value");
        let facts = Facts { name: "240SX", mass_kg: 1220.0 };
        assert_eq!(cell(mass, Some(facts), 0).as_deref(), Some("1220"), "the real stock mass");
        for level in 1..LEVELS.len() {
            assert_eq!(cell(mass, Some(facts), level), None, "L{level} is not known");
        }
        // And a parameter with no source stays empty even in the column that has a record.
        assert_eq!(cell(&SECTIONS[4].params[2], Some(facts), 0), None, "gear_1 is not known");
    }
}
