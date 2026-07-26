//! The CARP screen — the design's car-parameter table, over what this install actually holds.
//!
//! # What the screen is
//!
//! The design draws CARP as a full handling editor: nine `[::SECTION]`s, four upgrade columns,
//! editable, with a live torque curve and a Save button. It labels the source `CARP.BIN`.
//!
//! **NFSU2 ships no `CARP.BIN`** — `find -iname '*carp*'` over a full install returns nothing; CARP
//! is the older NFS engine's format. The data is there, just not under that name: `GLOBALB.BUN`'s
//! `0x00034600` is a per-car physics record, `8 + 46 × 2192 == 100,840` — the chunk's size exactly —
//! one record per `CarTypeInfo` car and in the same order. `gizmo_nfs::globalb` reads it.
//!
//! # What fills, and what does not
//!
//! Twenty-four of the forty rows have a source, and the reasons are per row rather than per screen:
//!
//! * `[::ENGINE]` idle / red line / limiter, `[::TORQUE_CURVE]`, `[::GEARBOX]` and `drive_type` come
//!   straight out of the record.
//! * `[::GEARBOX]` is the only section whose **four upgrade columns** all fill, because it is the
//!   only thing the record stores four times. Everything else it stores once, so it is shown under
//!   `STOCK` and left blank under the upgrades — repeating a number across four columns would read
//!   as "this upgrade changes nothing", which the file does not say.
//! * `[::AERO]` and `[::BRAKES]` are **not in this game's files**, and that is measured rather than
//!   unsearched: the one brake-shaped triple is exactly zero for all 15 traffic vehicles, and the
//!   likeliest remaining candidate — `+0x38C`, which a modding tool calls `BrakeBias` — was set to
//!   0.02 on a real 240SX, installed and driven, and braking was unchanged. A negative anyone can
//!   re-run beats a negative nobody looked for.
//! * `[::STEERING]` half filled, and the half that did is a correction. It was ruled out here for a
//!   good reason that turned out to be about the wrong quantity: the only steering *angles* in the
//!   record are a global ±43 identical in all 46 cars, so nothing angle-shaped could be per-car.
//!   `steer_speed` is not an angle — it is a multiplier, 1.00 to 1.25 across the install — and
//!   setting a 240SX's to 0.05 produced a car that would barely turn. `steer_lock` stays empty; the
//!   angles really are global.
//! * `torque_scale` has no lane because the curve is stored in absolute N·m, so there is nothing to
//!   scale. `shift_time`'s only candidate is one constant shared by 45 of 46 cars. `grip_*`,
//!   `slip_angle`, `weight_bias_f` and `cg_height` were swept for and are absent.
//!
//! # The one deliberate departure
//!
//! The design's torque section is eight rows labelled by rpm, generated from its own `rpmSteps`. The
//! file holds **nine** magnitudes and **no rpm axis** — every 4-aligned lane of all 46 records was
//! swept for a nine-wide increasing run and there is none. So the rows here are the file's own nine
//! points, unlabelled by rpm. Putting eight invented rpms on nine real numbers is the single thing
//! this screen exists not to do.
//!
//! # Two things this screen has already got wrong
//!
//! Both are recorded because they are the failure modes it is built against.
//!
//! It came out **black** below its two panels the first time it ran: every other full-bleed screen
//! wraps itself in a `CentralPanel` whose frame fills [`token::BG`] and this one did not, so the
//! region was never painted. `--shot` caught it perfectly — the PNG held `RGBA(8, 8, 8, 180)` across
//! the whole unfilled area — and it was read as white off the preview and missed. The lesson is not
//! that a screenshot cannot see this; it is that one has to be *checked*, and the cheap check is a
//! pixel.
//!
//! And its note said the remaining sections "have not been located in this install" while they were
//! being located. A screen whose whole purpose is to distinguish *absent* from *unread* had the two
//! confused in its own caption for a commit.

use crate::app::PryHub;
use crate::theme::{self, token};
use crate::widget;
use egui::{RichText, Ui};

/// Where a cell's value comes from, now that `GLOBALB.BUN`'s per-car physics record is read.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Source {
    /// The car's name.
    CarName,
    /// Stock mass in kilograms.
    MassKg,
    /// Front / all / rear, from the rear-drive fraction.
    DriveType,
    /// One of the three rpm limits.
    Rpm(Rpm),
    /// One of the nine torque points, in N·m.
    Torque(usize),
    /// How many forward gears this level has.
    GearCount,
    /// The level's final drive.
    FinalDrive,
    /// Forward gear `n` (1-based) at this level.
    Gear(usize),
    /// Front tyre width in millimetres.
    TyreWidth,
    /// The steering multiplier — see the module note for why this row exists and its neighbour
    /// does not.
    SteerRatio,
    /// Nothing in this install answers it. See the module note.
    NotLocated,
}

/// Which rpm limit a row shows.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Rpm {
    Idle,
    RedLine,
    Limiter,
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

/// A parameter with no source.
const fn gap(label: &'static str, unit: &'static str) -> Param {
    Param { label, unit, source: Source::NotLocated }
}

/// A parameter the record answers.
const fn got(label: &'static str, unit: &'static str, source: Source) -> Param {
    Param { label, unit, source }
}

/// The design's nine sections. Seven of the rows it draws have no answer in this game's files and
/// stay listed rather than dropped — "the format has an aero section and this game does not store
/// one" is a different statement from "there is no aero", and only the first is true.
///
/// One deliberate departure: the design's torque section is eight rows labelled by rpm
/// (`trq_1000`..`trq_8000`), generated from its own `rpmSteps`. The file holds **nine** magnitudes
/// and **no rpm axis at all** — that absence is measured, not unsearched. Labelling nine numbers
/// with eight invented rpms would be the one thing this screen exists not to do, so the rows are the
/// file's own points instead.
static SECTIONS: &[Section] = &[
    Section {
        name: "[::VEHICLE]",
        params: &[
            got("car_name", "", Source::CarName),
            gap("car_class", ""),
            got("drive_type", "", Source::DriveType),
        ],
    },
    Section {
        name: "[::MASS]",
        params: &[
            got("mass", "kg", Source::MassKg),
            gap("weight_bias_f", "%"),
            gap("cg_height", "m"),
        ],
    },
    Section {
        name: "[::ENGINE]",
        params: &[
            got("idle_rpm", "rpm", Source::Rpm(Rpm::Idle)),
            got("red_line", "rpm", Source::Rpm(Rpm::RedLine)),
            got("max_rpm", "rpm", Source::Rpm(Rpm::Limiter)),
            // The file stores absolute torque, so there is no scale factor to store.
            gap("torque_scale", "×"),
        ],
    },
    Section {
        name: "[::TORQUE_CURVE]",
        params: &[
            got("trq_pt_1", "Nm", Source::Torque(0)),
            got("trq_pt_2", "Nm", Source::Torque(1)),
            got("trq_pt_3", "Nm", Source::Torque(2)),
            got("trq_pt_4", "Nm", Source::Torque(3)),
            got("trq_pt_5", "Nm", Source::Torque(4)),
            got("trq_pt_6", "Nm", Source::Torque(5)),
            got("trq_pt_7", "Nm", Source::Torque(6)),
            got("trq_pt_8", "Nm", Source::Torque(7)),
            got("trq_pt_9", "Nm", Source::Torque(8)),
        ],
    },
    Section {
        name: "[::GEARBOX]",
        params: &[
            got("gear_count", "", Source::GearCount),
            got("final_drive", ":1", Source::FinalDrive),
            got("gear_1", ":1", Source::Gear(1)),
            got("gear_2", ":1", Source::Gear(2)),
            got("gear_3", ":1", Source::Gear(3)),
            got("gear_4", ":1", Source::Gear(4)),
            got("gear_5", ":1", Source::Gear(5)),
            got("gear_6", ":1", Source::Gear(6)),
            gap("shift_time", "s"),
        ],
    },
    Section {
        name: "[::TIRES]",
        params: &[
            gap("grip_front", "g"),
            gap("grip_rear", "g"),
            gap("slip_angle", "°"),
            got("tire_width", "mm", Source::TyreWidth),
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
    Section {
        name: "[::STEERING]",
        params: &[
            // Still absent: the only angles in the record are a global ±43, identical in all 46.
            gap("steer_lock", "°"),
            got("steer_speed", "×", Source::SteerRatio),
        ],
    },
];

/// The design's four upgrade columns.
const LEVELS: [&str; 4] = ["STOCK", "L1", "L2", "L3"];

/// Which section is open and which level is highlighted.
#[derive(Clone, Copy, Default)]
pub struct State {
    /// Index into [`SECTIONS`].
    pub section: usize,
    /// Index into [`LEVELS`].
    pub level: usize,
}

/// How many of the design's parameters this install can answer.
#[must_use]
pub fn located() -> usize {
    SECTIONS
        .iter()
        .flat_map(|s| s.params.iter())
        .filter(|p| p.source != Source::NotLocated)
        .count()
}

/// Every parameter the screen draws.
#[must_use]
pub fn total() -> usize {
    SECTIONS.iter().map(|s| s.params.len()).sum()
}

/// The fields this screen reads out of a `CarTypeInfo`, as a small owned view.
///
/// `gizmo_nfs::CarTypeInfo` and its `Gearbox` are `#[non_exhaustive]` — the published crate
/// correctly refusing to be constructed from outside — so the cell logic takes this instead. That
/// keeps it a pure function over values that a test can drive without faking a parser struct, which
/// is the same reason the earlier version of this screen took a two-field view.
#[derive(Clone, Debug, PartialEq)]
pub struct Car {
    name: String,
    mass_kg: f32,
    rear_drive: f32,
    rpm: [f32; 3],
    torque_nm: [f32; 9],
    tyre_width_mm: f32,
    steer_ratio: f32,
    /// One per upgrade level: final drive, gear count, and the forward ratios.
    gearbox: [(f32, usize, [f32; 6]); 4],
}

impl Car {
    /// Take the view from a parsed record.
    fn of(c: &gizmo_nfs::CarTypeInfo) -> Self {
        let h = &c.handling;
        let mut gearbox = [(0.0, 0, [0.0; 6]); 4];
        for (slot, g) in gearbox.iter_mut().zip(h.gearbox.iter()) {
            *slot = (g.final_drive, g.count, g.forward);
        }
        Self {
            steer_ratio: h.steer_ratio,
            name: c.name.clone(),
            mass_kg: c.mass_kg,
            rear_drive: h.rear_drive,
            rpm: [h.engine.idle_rpm, h.engine.red_line_rpm, h.engine.limiter_rpm],
            torque_nm: h.torque_nm,
            tyre_width_mm: h.tyre_width_m[0] * 1000.0,
            gearbox,
        }
    }
}

/// The value for one cell, or `None` when nothing here answers it.
///
/// `level` is the design's upgrade column, and it is no longer decorative: the record carries four
/// transmission blocks, so every `[::GEARBOX]` row genuinely differs across the four. Everything
/// else the file stores once, so it is shown in `STOCK` and left blank under the upgrades rather
/// than repeated four times — a repeated number reads as "this upgrade changes nothing", which is a
/// claim the file does not make.
fn cell(param: &Param, car: Option<&Car>, level: usize) -> Option<String> {
    let car = car?;
    let stock_only = |v: String| if level == 0 { Some(v) } else { None };
    let (final_drive, count, forward) = *car.gearbox.get(level)?;
    match param.source {
        Source::CarName => stock_only(car.name.clone()),
        Source::MassKg => stock_only(format!("{:.0}", car.mass_kg)),
        Source::DriveType => stock_only(
            match car.rear_drive {
                r if r <= 0.01 => "FWD",
                r if r >= 0.99 => "RWD",
                _ => "AWD",
            }
            .to_owned(),
        ),
        Source::Rpm(which) => stock_only(format!(
            "{:.0}",
            car.rpm[match which {
                Rpm::Idle => 0,
                Rpm::RedLine => 1,
                Rpm::Limiter => 2,
            }]
        )),
        Source::Torque(i) => stock_only(format!("{:.0}", car.torque_nm.get(i).copied()?)),
        Source::TyreWidth => stock_only(format!("{:.0}", car.tyre_width_mm)),
        Source::SteerRatio => stock_only(format!("{:.2}", car.steer_ratio)),
        Source::GearCount => Some(count.to_string()),
        Source::FinalDrive => Some(format!("{final_drive:.3}")),
        Source::Gear(n) => forward.get(n - 1).filter(|_| n <= count).map(|r| format!("{r:.3}")),
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
    let asked = app.car_spec.is_some();
    // The panels take the screen's own state by value and hand back what changed, so the record can
    // stay *borrowed* out of the app for the whole draw. Copying it per frame would have worked too
    // and would have been a copy per frame for nothing.
    let mut state = app.carp;
    {
        // `Some(None)` is "asked, and the answer was no" — drawn differently from "not asked yet".
        let car = app.car_spec.as_ref().and_then(|o| o.as_deref()).map(Car::of);
        header(ui, t, d, &mut state);
        let full = ui.available_rect_before_wrap();
        ui.horizontal_top(|ui| {
            ui.set_min_height(full.height());
            sections_panel(ui, t, d, &mut state);
            widget::rule_v(ui, full.height(), token::DIVIDER);
            table_panel(ui, t, d, &mut state, car.as_ref(), asked);
        });
    }
    app.carp = state;
}

/// The row the design puts above everything: what is being read, and the level switch.
fn header(ui: &mut Ui, t: &crate::i18n::Strings, d: theme::Density, state: &mut State) {
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
                let mut level = state.level;
                let items: Vec<(usize, &str)> =
                    LEVELS.iter().enumerate().map(|(i, n)| (i, *n)).collect();
                if widget::segmented(ui, egui::Id::new("carp-level"), &mut level, &items, widget::Seg::Small) {
                    state.level = level;
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
fn sections_panel(ui: &mut Ui, t: &crate::i18n::Strings, d: theme::Density, state: &mut State) {
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
                    let on = i == state.section;
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
                        state.section = i;
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
    ui: &mut Ui,
    t: &crate::i18n::Strings,
    d: theme::Density,
    state: &mut State,
    car: Option<&Car>,
    asked: bool,
) {
    let section = &SECTIONS[state.section.min(SECTIONS.len() - 1)];
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
        let note = match (asked, car) {
            (_, Some(c)) => format!("{} · {}", t.cp_from_globalb, c.name),
            (true, None) => t.cp_no_install.to_owned(),
            (false, None) => String::new(),
        };
        if !note.is_empty() {
            ui.horizontal(|ui| {
                ui.add_space(token::SPACE_3);
                widget::tag(
                    ui,
                    &note,
                    if car.is_some() { widget::Tone::Accent } else { widget::Tone::Neutral },
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
                            if i == state.level { token::ACCENT } else { theme::muted(45) };
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
                            match cell(param, car, level) {
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
        // 40 rows: the design's 39, plus one, because its torque section is eight rows labelled by
        // rpm and the file holds nine magnitudes with no rpm axis. Labelling nine numbers with
        // eight invented rpms is the one thing this screen exists not to do.
        assert_eq!(total(), 40, "thirty-nine design rows, with the torque curve at the file's nine");
        assert_eq!(SECTIONS[3].params.len(), 9, "the file's nine torque points");
    }

    /// Exactly which rows claim a source. Asserted by name rather than by count, so that wiring a
    /// row to a guess has to be done in the open, with this list edited to say so.
    #[test]
    fn only_what_globalb_answers_is_claimed() {
        let named: Vec<&str> = SECTIONS
            .iter()
            .flat_map(|s| s.params.iter())
            .filter(|p| p.source != Source::NotLocated)
            .map(|p| p.label)
            .collect();
        assert_eq!(
            named,
            vec![
                "car_name", "drive_type", "mass", "idle_rpm", "red_line", "max_rpm", "trq_pt_1",
                "trq_pt_2", "trq_pt_3", "trq_pt_4", "trq_pt_5", "trq_pt_6", "trq_pt_7", "trq_pt_8",
                "trq_pt_9", "gear_count", "final_drive", "gear_1", "gear_2", "gear_3", "gear_4",
                "gear_5", "gear_6", "tire_width", "steer_speed",
            ]
        );
        assert_eq!(located(), 25);
        // `AERO` and `BRAKES` stay wholly unclaimed, and that is measured rather than unsearched —
        // the module note says how, including a candidate that was driven and did nothing. A row
        // here gaining a source later should be a deliberate edit to this test too, which is exactly
        // what happened to `STEERING` below.
        for section in &SECTIONS[6..8] {
            assert!(
                section.params.iter().all(|p| p.source == Source::NotLocated),
                "{} is not in this game's files",
                section.name
            );
        }
        // `STEERING` is half filled, and the halves are not interchangeable: the multiplier is
        // per-car and was confirmed by driving it, the lock angle is a global ±43 and stays a gap.
        let steering = &SECTIONS[8];
        assert_eq!(steering.name, "[::STEERING]");
        assert!(steering.params[0].source == Source::NotLocated, "steer_lock is still global");
        assert!(steering.params[1].source == Source::SteerRatio, "steer_speed reads the multiplier");
    }

    /// With no record nothing is filled. With one, the gearbox varies across all four upgrade
    /// columns and everything else is shown once, under `STOCK`.
    #[test]
    fn upgrade_columns_show_only_what_varies() {
        let mass = &SECTIONS[1].params[0];
        assert_eq!(cell(mass, None, 0), None, "no record, no value");

        let car = sample();
        assert_eq!(cell(mass, Some(&car), 0).as_deref(), Some("1220"));
        for level in 1..LEVELS.len() {
            assert_eq!(cell(mass, Some(&car), level), None, "mass is stored once, not per level");
        }

        // The gearbox is the one section the file stores four times, so it is the one section whose
        // upgrade columns are filled.
        let final_drive = &SECTIONS[4].params[1];
        assert_eq!(cell(final_drive, Some(&car), 0).as_deref(), Some("4.000"));
        assert_eq!(cell(final_drive, Some(&car), 3).as_deref(), Some("3.500"));

        // A sixth gear the stock box does not have stays empty at STOCK and fills at L3.
        let gear6 = &SECTIONS[4].params[7];
        assert_eq!(cell(gear6, Some(&car), 0), None, "the stock five-speed has no sixth gear");
        assert_eq!(cell(gear6, Some(&car), 3).as_deref(), Some("0.800"));

        // And a row this game does not store stays empty even where a record exists.
        let drag = &SECTIONS[6].params[0];
        assert_eq!(cell(drag, Some(&car), 0), None, "aero is not in the file");
    }

    /// Rear-drive fraction reads as the three words the design's row expects.
    #[test]
    fn drive_type_is_named_from_the_fraction() {
        let mut car = sample();
        let row = &SECTIONS[0].params[2];
        car.rear_drive = 0.0;
        assert_eq!(cell(row, Some(&car), 0).as_deref(), Some("FWD"));
        car.rear_drive = 0.5;
        assert_eq!(cell(row, Some(&car), 0).as_deref(), Some("AWD"));
        car.rear_drive = 1.0;
        assert_eq!(cell(row, Some(&car), 0).as_deref(), Some("RWD"));
    }

    /// A stand-in shaped like the 240SX's record: a stock five-speed that gains a sixth by L3.
    fn sample() -> Car {
        let five = (4.0, 5, [3.321, 1.902, 1.308, 1.0, 0.9, 0.0]);
        let six = (3.5, 6, [3.321, 1.902, 1.308, 1.09, 0.92, 0.8]);
        Car {
            name: "240SX".into(),
            mass_kg: 1220.0,
            rear_drive: 1.0,
            rpm: [800.0, 6500.0, 7000.0],
            torque_nm: [140.0, 150.0, 160.0, 180.0, 200.0, 216.0, 203.0, 170.0, 150.0],
            tyre_width_mm: 205.0,
            steer_ratio: 1.1,
            gearbox: [five, five, five, six],
        }
    }
}
