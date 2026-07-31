//! The CARP screen — the design's car-parameter table, over what this install actually holds, and
//! now writing back into it.
//!
//! # What the screen is
//!
//! The design draws CARP as a full handling editor: nine `[::SECTION]`s, four upgrade columns,
//! editable, with a live torque curve and a Save button. It labels the source `CARP.BIN`.
//!
//! **NFSU2 ships no `CARP.BIN`** — `find -iname '*carp*'` over a full install returns nothing; CARP
//! is the older NFS engine's format. The data is there, just not under that name: `GLOBALB.BUN`'s
//! `0x00034600` is a per-car physics record, `8 + 46 × 2192 == 100,840` — the chunk's size exactly —
//! one record per `CarTypeInfo` car and in the same order. `gizmo_nfs::globalb` reads it, and
//! `gizmo_nfs::globalb::edit` writes it.
//!
//! # Three states, not two
//!
//! A row is **proved**, a **candidate**, or **empty**, and the middle one is new. It exists because
//! the alternative is worse in both directions: a lane that reads plausibly and is not proved gets
//! drawn as a dash (and so nobody ever tests it) or gets drawn as a number (and so it becomes a
//! fact by being printed). Candidates are drawn in the second accent, say so on hover, are counted
//! separately in the header, and are **editable** — because the only thing that settles one is
//! changing it, installing it and driving. Each one's evidence, its rival readings and the
//! experiment that would decide it are in [`gizmo_nfs::Unproven`].
//!
//! Thirty-two of the forty-seven rows have a lane; **twenty-six of those are proved and six are
//! candidates**. It was seven: `steer_lock` was wired to the per-car angle at `+0x284`, set from
//! 37° to 12° on a 240SX, installed and driven, and the car steered exactly as before. The row is a
//! dash again, and the dash means something it did not mean before — *tested*, not *unexamined*.
//!
//! # What fills, and what does not
//!
//! The reasons are per row rather than per screen:
//!
//! * `[::ENGINE]` idle / red line / limiter, `[::TORQUE_CURVE]`, `[::GEARBOX]` and `drive_type` come
//!   straight out of the record.
//! * **The upgrade columns fill for two sections now, not one.** `[::GEARBOX]` always did, because
//!   the record stores four transmissions. `[::TORQUE_CURVE]` joins it: the record also keeps four
//!   nine-point torque tables at `+0x530`, `+0x570`, `+0x5B0`, `+0x5F0`, graduated 34 % / 68 % /
//!   100 % of a per-car maximum in every one of the 31 playable cars — see
//!   [`gizmo_nfs::CarHandling::torque_gain_nm`], which sets out what that does and does not prove.
//!   The `L1`–`L3` cells show **stock + gain**, which is the number someone wants; editing one
//!   stores the difference, because the gain is what the lane holds.
//! * Everything else the record stores **once**, so it is shown under `STOCK` and left blank under
//!   the upgrades — repeating a number across four columns would read as "this upgrade changes
//!   nothing", which the file does not say.
//! * `[::AERO]` is the one section with nothing at all, and it now has a reason rather than a
//!   shrug: the sweep over the 46 records found nothing, and `SPEED2.EXE` holds `aero_drag`,
//!   `aero_lift` and `downforce` in a single **global** block. Aero may simply not be per-car in
//!   this game, which would explain a null that no amount of further sweeping was going to fix.
//! * `[::BRAKES]` keeps two of three empty and promotes the third to a candidate. That row is
//!   `+0x38C`: 0.47–0.60 in all 46 records, a *front fraction* just over half, ordering by
//!   drivetrain (FWD 0.564, RWD 0.531, AWD 0.530) so front-heavy cars get more front brake, and the
//!   game's own tuning text reads "Brake bias controls how much braking the front tires do verses
//!   the rears." Against it: set to 0.02 on a real 240SX, installed and driven, braking unchanged
//!   — but a bias moves force between axles and does not change **stopping distance**, so that
//!   experiment very likely watched the wrong thing. A brake *force* and a handbrake stay dashes.
//! * `[::STEERING]` is half filled and the two halves are the two ways this screen has been wrong.
//!   `steer_speed` was ruled out for a good reason about the wrong quantity — the only steering
//!   *angles* found were a global ±43 — and it is not an angle at all; setting a 240SX's to 0.05
//!   produced a car that would barely turn. `steer_lock` went the other way: a per-car angle at
//!   `+0x284` looked right enough to wire in as a candidate, and the drive refused it. It also reads
//!   100 on every traffic vehicle against a bus's 75, and 60 on a HUMMER against 27 on a 106 — small
//!   cars have *more* lock, not less. So the row is empty for the third time and for a new reason:
//!   it has been tested. The lane and its refutation are kept in
//!   [`gizmo_nfs::Unproven::angle_284_deg`] rather than deleted, so nobody proposes it again.
//! * `torque_scale` has no lane because the curve is stored in absolute N·m, so there is nothing to
//!   scale. `shift_time`'s only candidate is one constant shared by 45 of 46 cars. `grip_*`,
//!   `slip_angle`, `weight_bias_f` and `cg_height` were swept for and are absent.
//! * `[::TIRES]` has **two** width rows where the design has one, because the file has two: 12 of
//!   the 46 records give the rear axle a different width from the front — a SUPRA is 225 / 245 —
//!   and one row writing all four corners would quietly square them off. Its grip rows stay empty:
//!   the only claim anyone has made for them reads `+0.730, −0.730, −0.730, +0.730` across the four
//!   corners, which is a mirrored coordinate rather than a grip, and whose "rear" is the front-right
//!   wheel.
//! * `[::SUSPENSION]` is a **tenth section the design does not draw**, and it is here because the
//!   game names it. `LANGUAGES/English.bin` lists front and rear springs, shocks and sway bars among
//!   its ten tuning sliders, so stopping at nine sections was inheriting the design's omission
//!   rather than being faithful to it. Four of the six rows have a candidate lane in two eight-lane
//!   blocks at `+0x1E0` and `+0x200`; the sway bars have none and stay dashes.
//!
//! # Writing
//!
//! Save goes into **`GLOBAL/GlobalB.lzc`**, which is the file the game opens, and `GLOBALB.BUN`
//! beside it, which is the file every tool here reads. A `.bak` of each is taken first and never
//! replaced. All of that is `gizmo_nfs::globalb::install`'s, and the reasons are there; what belongs
//! here is that the path is drawn **next to the button**, before it is pressed.
//!
//! Edits are held rather than written per keystroke, so the whole set goes in one pass and Revert
//! means something. They are also kept per car and dropped when the open car changes — a pending
//! `idle_rpm` applied to whichever record happened to be open next is the bug this screen would
//! otherwise have.
//!
//! # The one deliberate departure
//!
//! The design's torque section is rows labelled by rpm, generated from its own `rpmSteps`, and this
//! screen refused to label its nine for a long time — the axis was recorded as absent, having been
//! swept for as a nine-wide increasing run in every 4-aligned lane of all 46 records and not found.
//! That search was right and looked for the wrong shape: the axis is not a run, it is **idle to
//! limiter in eight equal steps**, two numbers this screen already had. It was settled on the game's
//! own dynamometer, on two cars — a 240SX reads 115.8 kW where this computes 115.86, and a Mustang
//! GT reads 223.6 where this computes 223.64. The second one matters because its span is different
//! (800 → 6500, so a 712.5 step) and its peak sits on a different point, so it is a second test
//! rather than the first one repeated. The rows carry their rpm now — and because the axis is
//! arithmetic over two editable fields, dragging the limiter moves every label on the section.
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
use gizmo_nfs::CarField;
use std::collections::BTreeMap;

/// Where a cell's value comes from, now that `GLOBALB.BUN`'s per-car physics record is read — and
/// which lane it goes back into.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Source {
    /// The car's name.
    CarName,
    /// Stock mass in kilograms.
    MassKg,
    /// Front / all / rear, from the rear-drive fraction.
    DriveType,
    /// One of the three rpm limits.
    Rpm(Rpm),
    /// One of the nine torque points, in N·m. Under `STOCK` that is the curve itself; under an
    /// upgrade it is the curve plus that level's gain table.
    Torque(usize),
    /// How many forward gears this level has.
    GearCount,
    /// The level's final drive.
    FinalDrive,
    /// Forward gear `n` (1-based) at this level.
    Gear(usize),
    /// Front tyre width in millimetres — corners 0 and 1, which are equal in all 46 records.
    TyreWidthFront,
    /// Rear tyre width, corners 2 and 3. A separate row because 12 records give it its own number.
    TyreWidthRear,
    /// The steering multiplier — see the module note for why this row exists and its neighbour
    /// does not.
    SteerRatio,
    /// Candidate: centre-of-gravity height at `+0x388`.
    CgHeight,
    /// Candidate: front brake bias at `+0x38C`.
    BrakeBias,
    /// Candidate: spring rate of axle `0` front / `1` rear.
    Spring(usize),
    /// Candidate: damping rate of the same axle.
    Damper(usize),
    /// Nothing in this install answers it. See the module note.
    NotLocated,
}

/// How well the lane behind a row is understood. Only meaningful where there is a lane at all.
///
/// The distinction is the whole point of this screen. A table that draws a measured number and a
/// guess in the same ink is worse than one that draws the guess as a dash, because the reader cannot
/// tell which they are looking at — and this program's own history has that failure in it twice.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Claim {
    /// Measured across the install, and for several of them settled by driving the car.
    Proved,
    /// Reads plausibly and is **not proved**. Drawn marked, and editable — because the experiment
    /// that settles one is to change it, install it, and drive.
    Candidate,
}

/// Which rpm limit a row shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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
    claim: Claim,
}

/// One `[::SECTION]` of the design's table.
struct Section {
    name: &'static str,
    params: &'static [Param],
}

/// A parameter with no source.
const fn gap(label: &'static str, unit: &'static str) -> Param {
    Param { label, unit, source: Source::NotLocated, claim: Claim::Proved }
}

/// A parameter the record answers, and that has been shown to.
const fn got(label: &'static str, unit: &'static str, source: Source) -> Param {
    Param { label, unit, source, claim: Claim::Proved }
}

/// A parameter with a lane behind it and no proof that the lane is the parameter.
const fn cand(label: &'static str, unit: &'static str, source: Source) -> Param {
    Param { label, unit, source, claim: Claim::Candidate }
}

/// The design's nine sections. Six of the rows it draws have no answer in this game's files and
/// stay listed rather than dropped — "the format has an aero section and this game does not store
/// one" is a different statement from "there is no aero", and only the first is true.
///
/// The torque section is the design's, at the file's own resolution: it draws eight rows from its
/// `rpmSteps` and the file holds **nine** points, so there are nine here. Their rpm is real and not
/// invented — idle to limiter in eight equal steps, confirmed against the game's dynamometer — so
/// [`row_label`] puts each row's own rpm in front of it.
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
            cand("cg_height", "m", Source::CgHeight),
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
            got("tire_width_f", "mm", Source::TyreWidthFront),
            got("tire_width_r", "mm", Source::TyreWidthRear),
        ],
    },
    Section {
        name: "[::AERO]",
        params: &[gap("drag_coef", "Cd"), gap("downforce_f", "N"), gap("downforce_r", "N")],
    },
    Section {
        name: "[::BRAKES]",
        params: &[
            gap("brake_force", "×"),
            cand("brake_bias_f", "", Source::BrakeBias),
            gap("handbrake", "×"),
        ],
    },
    Section {
        name: "[::STEERING]",
        params: &[
            // Empty again, and this time it is a **result** rather than a gap nobody has looked
            // into. The per-car angle at `+0x284` was proposed for this row, wired in as a
            // candidate, set from 37° to 12° on a 240SX, installed and driven — and the car steered
            // exactly as before. It also reads 100 on every traffic vehicle against a bus's 75, and
            // 60 on a HUMMER against 27 on a 106. See `gizmo_nfs::Unproven::angle_284_deg`, which
            // keeps the lane and the refutation; the row keeps the dash.
            gap("steer_lock", "°"),
            got("steer_speed", "×", Source::SteerRatio),
        ],
    },
    // The tenth section, which the design does not draw. It is here because **the game names it**:
    // `LANGUAGES/English.bin` lists front and rear springs, shocks and sway bars among its ten
    // tuning sliders, so a table that stops at nine sections is not being faithful to the design so
    // much as inheriting its omission. Four of the six have a candidate lane; the sway bars do not,
    // and stay dashes rather than being fitted to whatever is nearby.
    Section {
        name: "[::SUSPENSION]",
        params: &[
            cand("spring_f", "", Source::Spring(0)),
            cand("damper_f", "", Source::Damper(0)),
            cand("spring_r", "", Source::Spring(1)),
            cand("damper_r", "", Source::Damper(1)),
            gap("sway_bar_f", ""),
            gap("sway_bar_r", ""),
        ],
    },
];

/// The design's four upgrade columns.
const LEVELS: [&str; 4] = ["STOCK", "L1", "L2", "L3"];

/// Which section is open, which level is highlighted, and what has been changed but not written.
#[derive(Clone, Default)]
pub struct State {
    /// Index into [`SECTIONS`].
    pub section: usize,
    /// Index into [`LEVELS`].
    pub level: usize,
    /// Which car is being shown, by the name its record carries. Empty until the bundle lands.
    car: String,
    /// Every lane the user has changed, **keyed by car**, as the file would store it — so the top
    /// torque table's entry here is a gain, not the number on screen.
    ///
    /// Keyed by car because the screen is now a picker over all 46 records, and losing an edit by
    /// glancing at another car would be a bad trade for a lookup that costs nothing. The earlier
    /// version held one car's edits and dropped them on a switch; that was the right rule when a
    /// switch meant opening a different file, and the wrong one now that it is a click.
    edits: BTreeMap<String, BTreeMap<CarField, f32>>,
}

impl State {
    /// Which car is being shown, once one has been chosen.
    #[must_use]
    pub fn selected(&self) -> Option<&str> {
        (!self.car.is_empty()).then_some(self.car.as_str())
    }

    /// The selected car's pending edits, in the form a save takes.
    #[must_use]
    pub fn pending(&self) -> Vec<(CarField, f32)> {
        match self.edits.get(&self.car) {
            Some(edits) => edits.iter().map(|(&f, &v)| (f, v)).collect(),
            None => Vec::new(),
        }
    }

    /// Forget the selected car's — after a save that took, or on Revert. The other cars keep theirs.
    pub fn clear_edits(&mut self) {
        self.edits.remove(&self.car);
    }

    /// How many *other* cars have something pending, so a Save that writes one of several says so.
    #[must_use]
    fn others_pending(&self) -> usize {
        self.edits.iter().filter(|(car, e)| *car != &self.car && !e.is_empty()).count()
    }

    /// Open on `car` if nothing has been chosen yet. Never overrides a choice the user has made.
    fn default_to(&mut self, car: &str) {
        if self.car.is_empty() {
            self.car = car.to_owned();
        }
    }
}

/// Resolve a `--section` argument to an index into [`SECTIONS`].
///
/// This screen is the one that could not be photographed. `--screen carp` always opened on
/// `[::VEHICLE]`, which is three rows of the forty-one and none of the ones worth looking at, and
/// the section is chosen by clicking — so on a machine whose compositor will not hand out a screen
/// grab there was no way to check the table at all. Same reason `--select` and `--stage` exist.
///
/// Matched **by name**, because `--section torque` survives a section being inserted and
/// `--section 3` does not. The design writes its keys as `[::TORQUE_CURVE]` and that punctuation
/// is noise to type, so what is compared is the bare word, case-folded, as a prefix: `torque`,
/// `Torque_Curve` and `[::TORQUE_CURVE]` all land on the same row. A bare index is still taken,
/// for the two sections nobody wants to spell.
#[must_use]
pub fn section_index(query: &str) -> Option<usize> {
    let bare = |s: &str| s.trim().trim_start_matches("[::").trim_end_matches(']').to_ascii_uppercase();
    let want = bare(query);
    if want.is_empty() {
        return None;
    }
    if let Ok(n) = want.parse::<usize>() {
        return (n < SECTIONS.len()).then_some(n);
    }
    SECTIONS.iter().position(|s| bare(s.name).starts_with(&want))
}

/// How many of the design's parameters this install can put a number against — proved or not.
#[must_use]
pub fn located() -> usize {
    SECTIONS.iter().flat_map(|s| s.params.iter()).filter(|p| p.source != Source::NotLocated).count()
}

/// How many of those are candidates rather than claims.
///
/// Reported separately and drawn separately. "31 of 47 have a value" and "seven of the 31 are
/// guesses" are different facts, and a screen that showed only the first would be doing the thing
/// this one exists to prevent.
#[must_use]
pub fn candidates() -> usize {
    SECTIONS
        .iter()
        .flat_map(|s| s.params.iter())
        .filter(|p| p.source != Source::NotLocated && p.claim == Claim::Candidate)
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
///
/// It is also what makes the screen *live*. [`Car::with`] folds the pending edits into a copy, and
/// every number on screen is computed from that copy — so raising the limiter moves the torque rows'
/// rpm labels, and raising a stock torque point moves all three upgrade columns under it, in the
/// same frame and without anything being written.
#[derive(Clone, Debug, PartialEq)]
pub struct Car {
    name: String,
    mass_kg: f32,
    rear_drive: f32,
    rpm: [f32; 3],
    torque_nm: [f32; 9],
    /// The four upgrade tables, in N·m. Index 0 duplicates index 3 in the file and is not drawn;
    /// it is carried so that a write to `L3` can keep it in step.
    gain_nm: [[f32; 9]; 4],
    /// The engine speed each torque point sits at, taken from
    /// [`gizmo_nfs::CarHandling::torque_rpm`] rather than worked out here. The formula is short
    /// enough to have been inlined into [`row_label`] and that is exactly why it should not be: it
    /// is a claim about what the file means, so it belongs beside the bytes it is a claim about,
    /// where `ug2` reads the same one.
    torque_rpm: [f32; 9],
    /// Per corner, front-left first — the record's own order.
    tyre_width_mm: [f32; 4],
    steer_ratio: f32,
    /// One per upgrade level: final drive, gear count, and the forward ratios.
    gearbox: [(f32, usize, [f32; 6]); 4],
    /// The lanes with a candidate meaning: steer lock, CoG height, brake bias, and the two axles'
    /// spring and damper. Kept together and drawn marked — see [`Claim`].
    unproven: gizmo_nfs::Unproven,
}

impl Car {
    /// Take the view from a parsed record.
    fn of(c: &gizmo_nfs::CarTypeInfo) -> Self {
        let h = &c.handling;
        let mut gearbox = [(0.0, 0, [0.0; 6]); 4];
        for (slot, g) in gearbox.iter_mut().zip(h.gearbox.iter()) {
            *slot = (g.final_drive, g.count, g.forward);
        }
        let mut tyre_width_mm = [0.0; 4];
        for (mm, m) in tyre_width_mm.iter_mut().zip(h.tyre_width_m.iter()) {
            *mm = m * 1000.0;
        }
        Self {
            steer_ratio: h.steer_ratio,
            name: c.name.clone(),
            mass_kg: c.mass_kg,
            rear_drive: h.rear_drive,
            rpm: [h.engine.idle_rpm, h.engine.red_line_rpm, h.engine.limiter_rpm],
            torque_nm: h.torque_nm,
            gain_nm: h.torque_gain_nm,
            torque_rpm: h.torque_rpm(),
            tyre_width_mm,
            gearbox,
            unproven: h.unproven,
        }
    }

    /// The same car with the pending edits folded in — what a save would produce.
    #[must_use]
    fn with(mut self, edits: &BTreeMap<CarField, f32>) -> Self {
        use CarField as F;
        for (&field, &v) in edits {
            match field {
                F::MassKg => self.mass_kg = v,
                F::IdleRpm => self.rpm[0] = v,
                F::RedLineRpm => self.rpm[1] = v,
                F::LimiterRpm => self.rpm[2] = v,
                F::RearDrive => self.rear_drive = v,
                F::SteerRatio => self.steer_ratio = v,
                F::TorqueNm(i) => {
                    if let Some(t) = self.torque_nm.get_mut(i) {
                        *t = v;
                    }
                }
                F::TorqueGainNm { block, point } => {
                    if let Some(t) = self.gain_nm.get_mut(block).and_then(|b| b.get_mut(point)) {
                        *t = v;
                    }
                }
                F::FinalDrive(level) => {
                    if let Some(g) = self.gearbox.get_mut(level) {
                        g.0 = v;
                    }
                }
                F::GearCount(level) => {
                    if let Some(g) = self.gearbox.get_mut(level) {
                        g.1 = v.round().clamp(0.0, 6.0) as usize;
                    }
                }
                F::Gear { level, gear } => {
                    if let Some(r) =
                        self.gearbox.get_mut(level).and_then(|g| g.2.get_mut(gear.wrapping_sub(1)))
                    {
                        *r = v;
                    }
                }
                F::TyreWidthMm(corner) => {
                    if let Some(w) = self.tyre_width_mm.get_mut(corner) {
                        *w = v;
                    }
                }
                F::CgHeight => self.unproven.cg_height = v,
                F::BrakeBiasFront => self.unproven.brake_bias_f = v,
                F::Spring(axle) => {
                    if let Some(x) = self.unproven.spring.get_mut(axle) {
                        *x = v;
                    }
                }
                F::Damper(axle) => {
                    if let Some(x) = self.unproven.damper.get_mut(axle) {
                        *x = v;
                    }
                }
                // Reverse and the wheel radii have no row here; `ug2 tune` reaches them.
                _ => {}
            }
        }
        // The axis is arithmetic over two of the fields above, so it is re-derived rather than kept:
        // an edited limiter that left the labels alone would be the screen disagreeing with itself.
        let step = (self.rpm[2] - self.rpm[0]) / 8.0;
        for (i, rpm) in self.torque_rpm.iter_mut().enumerate() {
            *rpm = self.rpm[0] + i as f32 * step;
        }
        self
    }

    /// The curve at one upgrade level: stock, or stock plus that level's gain table.
    fn torque_at(&self, level: usize) -> [f32; 9] {
        let mut out = self.torque_nm;
        if level == 0 {
            return out;
        }
        for (t, g) in out.iter_mut().zip(self.gain_nm[level.min(3)].iter()) {
            *t += g;
        }
        out
    }

    /// Peak power at one level, in kilowatts, and the rpm it falls at — the figure the game's own
    /// dynamometer prints, which is the whole reason it is worth drawing.
    fn peak_kw(&self, level: usize) -> (f32, f32) {
        let torque = self.torque_at(level);
        let mut best = (0.0f32, self.torque_rpm[0]);
        for (i, &t) in torque.iter().enumerate() {
            let rpm = self.torque_rpm[i];
            let kw = t * rpm * std::f32::consts::TAU / 60.0 / 1000.0;
            if kw > best.0 {
                best = (kw, rpm);
            }
        }
        best
    }
}

/// A number the table can change, and where it goes.
#[derive(Clone, Debug, PartialEq)]
struct Num {
    /// The value as it reads on screen.
    value: f32,
    /// The lane it writes.
    field: CarField,
    /// A second lane kept in step with the first. The top upgrade's torque table is stored **twice**
    /// — `+0x530` duplicates `+0x5F0` in all 46 records — and writing one without the other would
    /// leave the file holding a disagreement it has never held.
    mirror: Option<CarField>,
    /// Taken off the number on screen to get the lane's own. The upgrade torque cells read
    /// `stock + gain` and the lane holds the gain.
    minus: f32,
    decimals: usize,
    speed: f64,
    lo: f32,
    hi: f32,
    /// A word drawn in place of the bare number, for a lane whose value has a name.
    word: bool,
}

impl Num {
    /// The value to store for a number typed on screen.
    fn stored(&self, shown: f32) -> f32 {
        shown - self.minus
    }
}

/// What one cell of the table holds.
#[derive(Clone, Debug, PartialEq)]
enum Cell {
    /// Nothing here answers it — the row has no source, or the record stores this one once and the
    /// column is an upgrade.
    Empty,
    /// Something to read and not to change.
    Fixed(String),
    /// A number, and the lane it writes.
    Num(Num),
}

impl Cell {
    /// What the cell reads as. `None` for [`Cell::Empty`].
    ///
    /// Only the tests call it — the interface draws each variant its own way — and that is the
    /// point: it is how a cell's *value* is asserted without standing up an `egui` context, so the
    /// table's arithmetic is tested rather than its painting.
    #[cfg(test)]
    fn text(&self) -> Option<String> {
        match self {
            Cell::Empty => None,
            Cell::Fixed(s) => Some(s.clone()),
            Cell::Num(n) if n.word => Some(format!("{:.2} {}", n.value, drive_word(n.value))),
            Cell::Num(n) => Some(format!("{:.*}", n.decimals, n.value)),
        }
    }
}

/// Front, all or rear, from the rear-drive fraction. The fraction partitions the playable cars
/// exactly by their real drivetrains, so the thresholds only have to separate the three cases.
fn drive_word(rear_drive: f32) -> &'static str {
    match rear_drive {
        r if r <= 0.01 => "FWD",
        r if r >= 0.99 => "RWD",
        _ => "AWD",
    }
}

/// A row's name, which for the torque points is the rpm they sit at.
///
/// The design labels its torque rows by rpm and this screen could not, because the axis was thought
/// to be absent; it is not, so they do. Without a car there is no axis either — the rpm is that
/// car's own idle and limiter — so with nothing open the rows keep the file's point number rather
/// than showing a number that would be somebody's guess.
fn row_label(param: &Param, car: Option<&Car>) -> String {
    match (param.source, car) {
        (Source::Torque(i), Some(c)) => match c.torque_rpm.get(i) {
            Some(rpm) => format!("{rpm:.0} rpm"),
            None => param.label.to_string(),
        },
        _ => param.label.to_string(),
    }
}

/// The value for one cell, and the lane behind it.
///
/// `level` is the design's upgrade column, and two sections use all four of them: the record carries
/// four transmission blocks and four torque tables. Everything else it stores once, so it is shown
/// in `STOCK` and left blank under the upgrades rather than repeated four times — a repeated number
/// reads as "this upgrade changes nothing", which is a claim the file does not make.
fn cell(param: &Param, car: Option<&Car>, level: usize) -> Cell {
    let Some(car) = car else { return Cell::Empty };
    let Some(&(final_drive, count, forward)) = car.gearbox.get(level) else { return Cell::Empty };
    let num = |value: f32, field: CarField, decimals: usize, speed: f64, lo: f32, hi: f32| {
        Cell::Num(Num { value, field, mirror: None, minus: 0.0, decimals, speed, lo, hi, word: false })
    };
    let stock_only = |c: Cell| if level == 0 { c } else { Cell::Empty };
    match param.source {
        Source::CarName => stock_only(Cell::Fixed(car.name.clone())),
        Source::MassKg => stock_only(num(car.mass_kg, CarField::MassKg, 0, 1.0, 100.0, 20_000.0)),
        Source::DriveType => stock_only(Cell::Num(Num {
            value: car.rear_drive,
            field: CarField::RearDrive,
            mirror: None,
            minus: 0.0,
            decimals: 2,
            speed: 0.01,
            lo: 0.0,
            hi: 1.0,
            word: true,
        })),
        Source::Rpm(which) => {
            let (i, field) = match which {
                Rpm::Idle => (0, CarField::IdleRpm),
                Rpm::RedLine => (1, CarField::RedLineRpm),
                Rpm::Limiter => (2, CarField::LimiterRpm),
            };
            stock_only(num(car.rpm[i], field, 0, 10.0, 0.0, 30_000.0))
        }
        Source::Torque(i) => {
            let Some(&base) = car.torque_nm.get(i) else { return Cell::Empty };
            if level == 0 {
                return num(base, CarField::TorqueNm(i), 0, 1.0, 0.0, 5_000.0);
            }
            let gain = car.gain_nm.get(level).map_or(0.0, |t| t[i]);
            Cell::Num(Num {
                value: base + gain,
                field: CarField::TorqueGainNm { block: level, point: i },
                // Index 0 is index 3's twin in every record; a write to L3 keeps it so.
                mirror: (level == 3).then_some(CarField::TorqueGainNm { block: 0, point: i }),
                minus: base,
                decimals: 0,
                speed: 1.0,
                lo: base,
                hi: base + 5_000.0,
                word: false,
            })
        }
        Source::TyreWidthFront => stock_only(Cell::Num(Num {
            value: car.tyre_width_mm[0],
            field: CarField::TyreWidthMm(0),
            mirror: Some(CarField::TyreWidthMm(1)),
            minus: 0.0,
            decimals: 0,
            speed: 1.0,
            lo: 50.0,
            hi: 500.0,
            word: false,
        })),
        Source::TyreWidthRear => stock_only(Cell::Num(Num {
            value: car.tyre_width_mm[2],
            field: CarField::TyreWidthMm(2),
            mirror: Some(CarField::TyreWidthMm(3)),
            minus: 0.0,
            decimals: 0,
            speed: 1.0,
            lo: 50.0,
            hi: 500.0,
            word: false,
        })),
        Source::SteerRatio => {
            stock_only(num(car.steer_ratio, CarField::SteerRatio, 2, 0.01, 0.0, 10.0))
        }
        Source::CgHeight => {
            stock_only(num(car.unproven.cg_height, CarField::CgHeight, 3, 0.005, 0.0, 5.0))
        }
        Source::BrakeBias => stock_only(num(
            car.unproven.brake_bias_f,
            CarField::BrakeBiasFront,
            3,
            0.005,
            0.0,
            1.0,
        )),
        Source::Spring(axle) => match car.unproven.spring.get(axle) {
            Some(&v) => stock_only(num(v, CarField::Spring(axle), 3, 0.005, 0.0, 20.0)),
            None => Cell::Empty,
        },
        Source::Damper(axle) => match car.unproven.damper.get(axle) {
            Some(&v) => stock_only(num(v, CarField::Damper(axle), 3, 0.005, 0.0, 20.0)),
            None => Cell::Empty,
        },
        Source::GearCount => num(count as f32, CarField::GearCount(level), 0, 0.05, 1.0, 6.0),
        Source::FinalDrive => num(final_drive, CarField::FinalDrive(level), 3, 0.005, 0.1, 20.0),
        Source::Gear(n) => match forward.get(n - 1) {
            // A gear the box does not have stays blank rather than showing the zero behind it.
            // Raising `gear_count` is what brings it into being, which is what the file says too.
            Some(&r) if n <= count => {
                num(r, CarField::Gear { level, gear: n }, 3, 0.005, 0.05, 20.0)
            }
            _ => Cell::Empty,
        },
        Source::NotLocated => Cell::Empty,
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
    let asked = app.cars.is_some();
    let bundle = app.car_bundle.clone();
    let saved = describe_save(app, t);
    // Cloned `Arc`, not borrowed: the state is taken out of the app below and the draw needs both.
    let cars = app.cars.clone().unwrap_or_default();
    let opened = app.car_opened.clone();

    // The state is *taken* rather than copied: it now holds a map of pending edits per car, so a
    // copy per frame would be a copy of every number the user has touched. It goes back below,
    // before anything is asked of the app again — which is also why Save is a flag returned from
    // the draw rather than a call made inside it.
    let mut state = std::mem::take(&mut app.carp);
    let mut save = false;
    let mut revert = false;
    {
        // Open on the car the file is, or on the first record the bundle holds — never on nothing
        // when there is something to show.
        if let Some(name) = opened.as_deref().or_else(|| cars.first().map(|c| c.name.as_str())) {
            state.default_to(name);
        }
        let record = cars.iter().find(|c| c.name == state.car);
        let file_car = record.map(Car::of);
        let live = state.edits.get(&state.car).cloned().unwrap_or_default();
        let car = file_car.clone().map(|c| c.with(&live));
        let dirty = dirty_count(file_car.as_ref(), &live);
        header(
            ui,
            t,
            d,
            &mut state,
            &cars,
            dirty,
            bundle.as_deref(),
            saved.as_deref(),
            &mut save,
            &mut revert,
        );
        let full = ui.available_rect_before_wrap();
        ui.horizontal_top(|ui| {
            ui.set_min_height(full.height());
            sections_panel(ui, t, d, &mut state, car.as_ref());
            widget::rule_v(ui, full.height(), token::DIVIDER);
            table_panel(ui, t, d, &mut state, car.as_ref(), asked);
        });
    }
    app.carp = state;
    if revert {
        app.carp.clear_edits();
    }
    if save {
        app.save_handling();
    }
}

/// How many pending edits actually differ from the file. An edit that has been dragged back to
/// where it started is not a change, and a Save button counting it would be lying about its own
/// work.
fn dirty_count(file: Option<&Car>, edits: &BTreeMap<CarField, f32>) -> usize {
    let Some(file) = file else { return 0 };
    edits
        .iter()
        .filter(|(&field, &value)| {
            stored_value(file, field).is_none_or(|was| (was - value).abs() > f32::EPSILON * 8.0)
        })
        .count()
}

/// What the file holds in one lane, in the units the pending map keeps.
fn stored_value(car: &Car, field: CarField) -> Option<f32> {
    use CarField as F;
    Some(match field {
        F::MassKg => car.mass_kg,
        F::IdleRpm => car.rpm[0],
        F::RedLineRpm => car.rpm[1],
        F::LimiterRpm => car.rpm[2],
        F::RearDrive => car.rear_drive,
        F::SteerRatio => car.steer_ratio,
        F::TorqueNm(i) => *car.torque_nm.get(i)?,
        F::TorqueGainNm { block, point } => *car.gain_nm.get(block)?.get(point)?,
        F::FinalDrive(level) => car.gearbox.get(level)?.0,
        F::GearCount(level) => car.gearbox.get(level)?.1 as f32,
        F::Gear { level, gear } => *car.gearbox.get(level)?.2.get(gear.checked_sub(1)?)?,
        F::TyreWidthMm(corner) => *car.tyre_width_mm.get(corner)?,
        F::CgHeight => car.unproven.cg_height,
        F::BrakeBiasFront => car.unproven.brake_bias_f,
        F::Spring(axle) => *car.unproven.spring.get(axle)?,
        F::Damper(axle) => *car.unproven.damper.get(axle)?,
        _ => return None,
    })
}

/// The line under the Save button: what the last write did, in the interface's own language.
fn describe_save(app: &PryHub, t: &crate::i18n::Strings) -> Option<String> {
    match app.carp_saved.as_ref()? {
        Ok(done) if done.changed == 0 => Some(t.cp_saved_nothing.to_owned()),
        Ok(done) => Some(format!("{} · {} {}", done.car, done.changed, t.cp_saved_lanes)),
        Err(e) => Some(format!("{} — {e}", t.cp_save_failed)),
    }
}

/// The car list, as a dropdown over every record in the bundle.
///
/// A `ComboBox` rather than another list panel: the screen already spends its left column on
/// sections and its right on the table, and 46 rows would have to take space from one of them to
/// show something a person picks once and then forgets about.
///
/// A car with something pending is marked in the list, because the edits survive a switch and a
/// picker that hid that would be inviting someone to lose them by not scrolling far enough.
fn car_picker(
    ui: &mut Ui,
    t: &crate::i18n::Strings,
    d: theme::Density,
    state: &mut State,
    cars: &[gizmo_nfs::CarTypeInfo],
) {
    ui.label(
        RichText::new(t.cp_car).font(theme::font::body(d.small_size())).color(theme::muted(55)),
    );
    if cars.is_empty() {
        ui.label(
            RichText::new("—").font(theme::font::mono(d.mono_size())).color(theme::muted(35)),
        );
        return;
    }
    let shown = if state.car.is_empty() { "—".to_owned() } else { state.car.clone() };
    // The colour is given explicitly, as every other label on this screen gives it. Left to the
    // default it came out `RGB(233, 232, 232)` against a `RGB(243, 242, 242)` background — the car
    // name was on screen and invisible, and a screenshot read at a glance would have passed it.
    egui::ComboBox::from_id_salt("carp-car")
        .selected_text(
            RichText::new(shown)
                .font(theme::font::mono(d.mono_size()))
                .color(token::TEXT)
                .strong(),
        )
        .width(150.0)
        .show_ui(ui, |ui| {
            ui.set_max_height(360.0);
            for car in cars {
                let pending = state.edits.get(&car.name).is_some_and(|e| !e.is_empty());
                let label = if pending {
                    format!("{}  ●", car.name)
                } else {
                    car.name.clone()
                };
                let ink = if pending { token::ACCENT } else { token::TEXT };
                let mut selected = state.car == car.name;
                if ui
                    .selectable_label(
                        selected,
                        RichText::new(label).font(theme::font::mono(d.mono_size())).color(ink),
                    )
                    .clicked()
                {
                    selected = true;
                    state.car = car.name.clone();
                }
                let _ = selected;
            }
        });
}

/// The row the design puts above everything: what is being read, the level switch, and the writing.
#[allow(clippy::too_many_arguments)]
fn header(
    ui: &mut Ui,
    t: &crate::i18n::Strings,
    d: theme::Density,
    state: &mut State,
    cars: &[gizmo_nfs::CarTypeInfo],
    dirty: usize,
    bundle: Option<&std::path::Path>,
    saved: Option<&str>,
    save: &mut bool,
    revert: &mut bool,
) {
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
                widget::rule_v(ui, 20.0, token::DIVIDER);
                // The car picker. All 46 records come out of one file, so browsing them is a click
                // rather than opening another `GEOMETRY.BIN` — which is what it used to be, and
                // which made comparing two cars a restart.
                car_picker(ui, t, d, state, cars);
                widget::rule_v(ui, 20.0, token::DIVIDER);
                ui.label(
                    RichText::new(t.cp_levels)
                        .font(theme::font::body(d.small_size()))
                        .color(theme::muted(55)),
                );
                let mut level = state.level;
                let items: Vec<(usize, &str)> =
                    LEVELS.iter().enumerate().map(|(i, n)| (i, *n)).collect();
                if widget::segmented(ui, egui::Id::new("carp-level"), &mut level, &items, widget::Seg::Small)
                {
                    state.level = level;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Enabled exactly when there is something to write and somewhere to write it.
                    // A Save that is live with no install behind it would fail in the worker, which
                    // is a slower and worse way to say the same thing.
                    let can = dirty > 0 && bundle.is_some();
                    ui.add_enabled_ui(can, |ui| {
                        *save |= widget::button_primary(ui, t.cp_save).clicked();
                    });
                    ui.add_enabled_ui(dirty > 0, |ui| {
                        *revert |= widget::button_secondary(ui, t.cp_revert).clicked();
                    });
                    let (text, tone) = match dirty {
                        0 => (t.cp_no_changes.to_owned(), theme::muted(45)),
                        n => (format!("{n} {}", t.cp_changed), token::ACCENT),
                    };
                    ui.label(RichText::new(text).font(theme::font::mono(d.small_size())).color(tone));
                    // Edits on cars other than this one are kept and are not what Save writes, so
                    // they are said out loud rather than left to be discovered.
                    let others = state.others_pending();
                    if others > 0 {
                        ui.label(
                            RichText::new(format!("+{others} {}", t.cp_other_cars))
                                .font(theme::font::mono(d.small_size()))
                                .color(theme::muted(50)),
                        )
                        .on_hover_text(t.cp_other_cars_note);
                    }
                });
            });
            // The file the button writes, named before it is pressed — and what the last press did.
            ui.horizontal(|ui| {
                match bundle {
                    Some(path) => {
                        widget::tag(ui, t.cp_writes_to, widget::Tone::Neutral);
                        ui.label(
                            RichText::new(path.display().to_string())
                                .font(theme::font::mono(d.small_size() - 1.0))
                                .color(theme::muted(50)),
                        )
                        .on_hover_text(t.cp_backup_note);
                    }
                    None => {
                        ui.label(
                            RichText::new(t.cp_no_install_write)
                                .font(theme::font::body(d.small_size()))
                                .color(theme::muted(45)),
                        );
                    }
                }
                if let Some(saved) = saved {
                    ui.add_space(token::SPACE_2);
                    widget::tag(ui, saved, widget::Tone::Accent);
                }
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

/// The left column: the nine sections, the live power readout, and the note that says what this
/// screen is.
fn sections_panel(
    ui: &mut Ui,
    t: &crate::i18n::Strings,
    d: theme::Density,
    state: &mut State,
    car: Option<&Car>,
) {
    ui.vertical(|ui| {
        ui.set_width(d.tree_width());
        widget::caption_strip(ui, t.cp_sections, |ui| {
            ui.label(
                RichText::new(format!("{} · {}p", SECTIONS.len(), total()))
                    .font(theme::font::mono(d.small_size()))
                    .color(theme::muted(45)),
            );
        });
        let reserve = if car.is_some() { 190.0 } else { 96.0 };
        egui::ScrollArea::vertical()
            .id_salt("carp-sections")
            .max_height((ui.available_height() - reserve).max(60.0))
            .show(ui, |ui| {
                for (i, section) in SECTIONS.iter().enumerate() {
                    let on = i == state.section;
                    let filled =
                        section.params.iter().filter(|p| p.source != Source::NotLocated).count();
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
            });
        if let Some(car) = car {
            power_panel(ui, t, d, car, state.level);
        }
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

/// The design's live torque curve, and what the four levels come to in kilowatts.
///
/// Kilowatts because that is what the game's own dynamometer reads in, which makes the number
/// checkable rather than decorative: anyone with the install can put a car on the dyno and hold it
/// up against this. The curve is drawn for the selected level over the stock one, so a gain table
/// being edited is visible as the gap between two lines rather than as nine numbers changing.
fn power_panel(ui: &mut Ui, t: &crate::i18n::Strings, d: theme::Density, car: &Car, level: usize) {
    ui.add_space(token::SPACE_2);
    widget::caption_strip(ui, t.cp_power, |ui| {
        let (kw, rpm) = car.peak_kw(level);
        ui.label(
            RichText::new(format!("{kw:.1} kW @ {rpm:.0}"))
                .font(theme::font::mono(d.small_size()))
                .color(token::ACCENT),
        );
    });
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width() - 8.0, 64.0),
        egui::Sense::hover(),
    );
    let painter = ui.painter();
    painter.rect_filled(rect, 2.0, token::SURFACE);
    let stock = car.torque_at(0);
    let shown = car.torque_at(level);
    let top = shown.iter().chain(stock.iter()).copied().fold(1.0f32, f32::max);
    let plot = |curve: &[f32; 9]| -> Vec<egui::Pos2> {
        curve
            .iter()
            .enumerate()
            .map(|(i, &t)| {
                let x = rect.left() + 4.0 + (rect.width() - 8.0) * i as f32 / 8.0;
                let y = rect.bottom() - 4.0 - (rect.height() - 8.0) * (t / top);
                egui::pos2(x, y)
            })
            .collect()
    };
    if level > 0 {
        painter.add(egui::Shape::line(plot(&stock), egui::Stroke::new(1.0_f32, theme::muted(30))));
    }
    painter.add(egui::Shape::line(plot(&shown), egui::Stroke::new(1.5_f32, token::ACCENT)));
    // Every level's peak, so the ladder is one glance rather than four clicks.
    for (l, name) in LEVELS.iter().enumerate() {
        let (kw, _) = car.peak_kw(l);
        ui.horizontal(|ui| {
            ui.add_space(token::SPACE_2);
            let on = l == level;
            ui.label(
                RichText::new(format!("{name:<6}"))
                    .font(theme::font::mono(d.small_size() - 1.0))
                    .color(if on { token::ACCENT } else { theme::muted(40) }),
            );
            ui.label(
                RichText::new(format!("{kw:>6.1} kW"))
                    .font(theme::font::mono(d.small_size() - 1.0))
                    .color(if on { token::TEXT } else { theme::muted(45) }),
            );
        });
    }
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
    let selected = state.car.clone();
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
            ui.label(
                RichText::new(format!("· {} {}", candidates(), t.cp_candidates))
                    .font(theme::font::mono(d.small_size()))
                    .color(token::ACCENT_2_700),
            )
            .on_hover_text(t.cp_candidate);
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
        let _ = &state.car;
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
                .min_col_width(64.0)
                .show(ui, |ui| {
                    ui.label("");
                    ui.label("");
                    for (i, name) in LEVELS.iter().enumerate() {
                        let ink = if i == state.level { token::ACCENT } else { theme::muted(45) };
                        ui.label(theme::tracked(name, 0.1, theme::font::mono(9.5), ink));
                    }
                    ui.end_row();

                    for param in section.params {
                        ui.label(
                            RichText::new(row_label(param, car))
                                .font(theme::font::mono(d.mono_size()))
                                .color(token::TEXT),
                        );
                        ui.label(
                            RichText::new(param.unit)
                                .font(theme::font::mono(d.small_size() - 1.5))
                                .color(theme::muted(42)),
                        );
                        for level in 0..LEVELS.len() {
                            let cell = cell(param, car, level);
                            let into = state.edits.entry(selected.clone()).or_default();
                            draw_cell(ui, t, d, &cell, param.claim, into);
                        }
                        ui.end_row();
                    }
                });
        });
    });
}

/// One cell: a dash, a word, or something to drag.
///
/// A candidate is drawn in the second accent rather than in the text colour, and says so when
/// hovered. That is the whole visible difference between "this is what the file holds" and "this is
/// what somebody thinks the file holds", and it has to be visible without hovering — a mark that
/// only appears on hover is a mark for the person who already suspected.
fn draw_cell(
    ui: &mut Ui,
    t: &crate::i18n::Strings,
    d: theme::Density,
    cell: &Cell,
    claim: Claim,
    edits: &mut BTreeMap<CarField, f32>,
) {
    match cell {
        Cell::Empty => {
            ui.label(
                RichText::new("—").font(theme::font::mono(d.mono_size())).color(theme::muted(30)),
            )
            .on_hover_text(t.cp_not_located);
        }
        Cell::Fixed(text) => {
            ui.label(
                RichText::new(text)
                    .font(theme::font::mono(d.mono_size()))
                    .color(token::TEXT)
                    .strong(),
            );
        }
        Cell::Num(num) => {
            let mut value = num.value;
            let drag = egui::DragValue::new(&mut value)
                .speed(num.speed)
                .range(num.lo..=num.hi)
                .fixed_decimals(num.decimals);
            // The drive fraction is a number whose value has a name, and the name is the thing
            // anybody reads. Both are shown: `1.00 RWD` is draggable and still says what it means.
            // Two things this has to get right, and the first draft got neither.
            //
            // The colour is `override_text_color`, not the widget's `fg_stroke`: this theme sets
            // the override, and an override beats a widget stroke, so painting the stroke changed
            // nothing at all. It looked plausible on screen and a pixel said otherwise — the
            // candidate cell and the proved one beside it both came back `RGB(32, 30, 29)`, which
            // is `token::TEXT` twice. Same lesson as the black panel this module already has a
            // paragraph about, and the same cheap check caught it.
            //
            // And it is inside a `scope`, because a `Grid` adds every cell to **one** `Ui`. A style
            // set in place would not end with the cell; it would tint every remaining cell of the
            // row, which is worse than no mark at all — a mark that spreads is a mark that lies.
            let resp = ui
                .scope(|ui| {
                    if claim == Claim::Candidate {
                        ui.visuals_mut().override_text_color = Some(token::ACCENT_2_700);
                    }
                    if num.word {
                        ui.add(drag.custom_formatter(|v, _| {
                            format!("{v:.2} {}", drive_word(v as f32))
                        }))
                    } else {
                        ui.add(drag)
                    }
                })
                .inner;
            let resp = if claim == Claim::Candidate {
                resp.on_hover_text(t.cp_candidate)
            } else {
                resp
            };
            if resp.changed() {
                let stored = num.stored(value);
                edits.insert(num.field, stored);
                if let Some(mirror) = num.mirror {
                    edits.insert(mirror, stored);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The design's table, counted. If a section is ever dropped to make the screen look fuller,
    /// this is what says so.
    #[test]
    fn the_screen_carries_the_designs_whole_table() {
        // Ten sections where the design draws nine. The tenth is `[::SUSPENSION]`, and it is here
        // because the *game* names it: `LANGUAGES/English.bin` lists front and rear springs, shocks
        // and sway bars among its ten tuning sliders. Adding a section is a departure and is meant
        // to be seen as one, which is what this line is for.
        assert_eq!(SECTIONS.len(), 10, "the design's nine, plus the one the game names");
        assert_eq!(SECTIONS[9].name, "[::SUSPENSION]");
        // 47 rows: the design's 39, plus one because its torque section generates eight rows from
        // its own `rpmSteps` where the file holds nine points; plus one because its single
        // tyre-width row is two here (12 of the 46 records give the rear axle its own width); plus
        // the six of the new section.
        assert_eq!(total(), 47, "the design's rows, at the file's own resolution");
        assert_eq!(SECTIONS[3].params.len(), 9, "the file's nine torque points");
    }

    /// Which rows are **candidates** rather than claims, by name.
    ///
    /// The list is spelt out for the same reason the proved one below it is: a row moving from
    /// candidate to proved is a claim about the game, and it should not be possible to make it
    /// without editing a test that says so out loud. Every entry's evidence, its rival readings and
    /// the experiment that would settle it live in `gizmo_nfs::Unproven`.
    #[test]
    fn every_candidate_is_named_as_one() {
        let named: Vec<&str> = SECTIONS
            .iter()
            .flat_map(|s| s.params.iter())
            .filter(|p| p.source != Source::NotLocated && p.claim == Claim::Candidate)
            .map(|p| p.label)
            .collect();
        assert_eq!(
            named,
            vec![
                "cg_height",     // +0x388 — SUVs top, hatchbacks bottom, traffic all exactly 0.300
                "brake_bias_f",  // +0x38C — a front fraction, FWD 0.564 against RWD 0.531
                "spring_f", "damper_f", "spring_r", "damper_r", // +0x1E0 / +0x200
            ]
        );
        assert_eq!(candidates(), 6);
        assert_eq!(located(), 32, "26 proved and 6 candidates");

        // A candidate is not a gap: it has a lane, and the cell is editable — because the only
        // thing that settles one is changing it and driving.
        let car = sample();
        let cg = &SECTIONS[1].params[2];
        assert_eq!(cg.claim, Claim::Candidate);
        let Cell::Num(num) = cell(cg, Some(&car), 0) else { panic!("editable") };
        assert_eq!(num.field, CarField::CgHeight);
        assert!(num.field.is_unproven(), "and the parser agrees it is one");
        // …and it is stored once, so it stays under STOCK.
        assert_eq!(cell(cg, Some(&car), 1), Cell::Empty);

        // `steer_lock` is a dash again, and that is a **result**: `+0x284` was wired in here, set
        // from 37° to 12° on a 240SX, installed and driven, and the car steered exactly as before.
        // The row going back to empty is what the experiment bought.
        let steer_lock = &SECTIONS[8].params[0];
        assert_eq!(steer_lock.label, "steer_lock");
        assert_eq!(steer_lock.source, Source::NotLocated, "tested, refused, and not quietly kept");
    }

    /// Exactly which rows are **proved**. Asserted by name rather than by count, so that promoting
    /// a row out of `Candidate` has to be done in the open, with this list edited to say so — which
    /// is a stronger guard than the one this test used to be, when there was nothing between
    /// "claimed" and "absent" and a guess could only enter by being dressed as a claim.
    #[test]
    fn only_what_globalb_answers_is_claimed() {
        let named: Vec<&str> = SECTIONS
            .iter()
            .flat_map(|s| s.params.iter())
            .filter(|p| p.source != Source::NotLocated && p.claim == Claim::Proved)
            .map(|p| p.label)
            .collect();
        assert_eq!(
            named,
            vec![
                "car_name", "drive_type", "mass", "idle_rpm", "red_line", "max_rpm", "trq_pt_1",
                "trq_pt_2", "trq_pt_3", "trq_pt_4", "trq_pt_5", "trq_pt_6", "trq_pt_7", "trq_pt_8",
                "trq_pt_9", "gear_count", "final_drive", "gear_1", "gear_2", "gear_3", "gear_4",
                "gear_5", "gear_6", "tire_width_f", "tire_width_r", "steer_speed",
            ]
        );
        assert_eq!(located() - candidates(), 26);
        // `AERO` stays wholly empty, and there is now a reason rather than a shrug: the sweep over
        // the 46 records found nothing, and `SPEED2.EXE` holds `aero_drag`, `aero_lift` and
        // `downforce` in one **global** block, so aero may simply not be per-car in this game.
        assert!(
            SECTIONS[6].params.iter().all(|p| p.source == Source::NotLocated),
            "aero is not in this record"
        );
        // `BRAKES` keeps two of its three empty. The bias has a candidate; a brake *force* and a
        // handbrake do not, and are not going to be fitted to whichever nearby lane is the right
        // order of magnitude. No row may claim a source — proved or candidate — without the claim
        // being made in the open by editing a test, which is what `STEERING` made someone do.
        let brakes = &SECTIONS[7];
        assert_eq!(brakes.params[0].source, Source::NotLocated, "brake_force has no lane");
        assert_eq!(brakes.params[2].source, Source::NotLocated, "handbrake has no lane");
        // `STEERING` is half filled, and the halves are not interchangeable: the multiplier is
        // per-car and was confirmed by driving it, the lock angle is a global ±43 and stays a gap.
        let steering = &SECTIONS[8];
        assert_eq!(steering.name, "[::STEERING]");
        // The lock is empty and the multiplier is proved, and the two emptinesses in this file's
        // history are not the same: the first was a sweep that had not looked in the right shape,
        // this one is a candidate that was looked at, driven, and refused.
        assert_eq!(steering.params[0].source, Source::NotLocated, "steer_lock was tested and refused");
        assert_eq!(steering.params[1].source, Source::SteerRatio, "steer_speed reads the multiplier");
        assert_eq!(steering.params[1].claim, Claim::Proved, "and that one was driven");
    }

    /// With no record nothing is filled. With one, the two sections the file stores four times vary
    /// across all four upgrade columns and everything else is shown once, under `STOCK`.
    #[test]
    fn upgrade_columns_show_only_what_varies() {
        let mass = &SECTIONS[1].params[0];
        assert_eq!(cell(mass, None, 0), Cell::Empty, "no record, no value");

        let car = sample();
        assert_eq!(cell(mass, Some(&car), 0).text().as_deref(), Some("1220"));
        for level in 1..LEVELS.len() {
            assert_eq!(
                cell(mass, Some(&car), level),
                Cell::Empty,
                "mass is stored once, not per level"
            );
        }

        // The gearbox is one of the two sections the file stores four times.
        let final_drive = &SECTIONS[4].params[1];
        assert_eq!(cell(final_drive, Some(&car), 0).text().as_deref(), Some("4.000"));
        assert_eq!(cell(final_drive, Some(&car), 3).text().as_deref(), Some("3.500"));

        // A sixth gear the stock box does not have stays empty at STOCK and fills at L3.
        let gear6 = &SECTIONS[4].params[7];
        assert_eq!(cell(gear6, Some(&car), 0), Cell::Empty, "the stock five-speed has no sixth");
        assert_eq!(cell(gear6, Some(&car), 3).text().as_deref(), Some("0.800"));

        // And a row this game does not store stays empty even where a record exists.
        let drag = &SECTIONS[6].params[0];
        assert_eq!(cell(drag, Some(&car), 0), Cell::Empty, "aero is not in the file");
    }

    /// The torque section is the other one that fills across the columns, and the number it shows
    /// under an upgrade is the curve **plus** that level's gain — while the lane it writes is the
    /// gain alone.
    #[test]
    fn the_upgrade_torque_columns_show_the_sum_and_write_the_gain() {
        let car = sample();
        let peak = &SECTIONS[3].params[5]; // trq_pt_6, the 240SX's 216 N·m
        assert_eq!(cell(peak, Some(&car), 0).text().as_deref(), Some("216"));
        assert_eq!(cell(peak, Some(&car), 1).text().as_deref(), Some("234"), "216 + 34 % of 54");
        assert_eq!(cell(peak, Some(&car), 3).text().as_deref(), Some("270"), "216 + 54");

        let Cell::Num(num) = cell(peak, Some(&car), 3) else { panic!("an editable cell") };
        assert_eq!(num.field, CarField::TorqueGainNm { block: 3, point: 5 });
        // Typing 300 into the L3 column stores the gain, not the sum.
        assert!((num.stored(300.0) - 84.0).abs() < 0.01);
        // …and the file's duplicate of that table is kept in step.
        assert_eq!(num.mirror, Some(CarField::TorqueGainNm { block: 0, point: 5 }));

        // STOCK writes the curve itself, with nothing taken off.
        let Cell::Num(stock) = cell(peak, Some(&car), 0) else { panic!("editable") };
        assert_eq!(stock.field, CarField::TorqueNm(5));
        assert_eq!(stock.mirror, None);
        assert!((stock.stored(300.0) - 300.0).abs() < 0.01);
    }

    /// An edit shows up everywhere it should in the same frame, which is what "live" has to mean:
    /// the limiter moves the rpm labels, and a stock torque point moves all three upgrade columns.
    #[test]
    fn an_edit_is_folded_back_in_before_anything_is_drawn() {
        let car = sample();
        let mut edits = BTreeMap::new();

        edits.insert(CarField::LimiterRpm, 9000.0);
        let live = car.clone().with(&edits);
        assert_eq!(live.torque_rpm[8], 9000.0, "the axis ends at the limiter");
        assert_eq!(live.torque_rpm[1], 1825.0, "and steps by an eighth of the new span");
        assert_eq!(row_label(&SECTIONS[3].params[8], Some(&live)), "9000 rpm");

        // Raising the stock curve raises every upgrade column under it, because a gain is a gain.
        edits.insert(CarField::TorqueNm(5), 300.0);
        let live = car.clone().with(&edits);
        let peak = &SECTIONS[3].params[5];
        assert_eq!(cell(peak, Some(&live), 0).text().as_deref(), Some("300"));
        assert_eq!(cell(peak, Some(&live), 3).text().as_deref(), Some("354"), "300 + the same 54");

        // Nothing was written: the car this came from still reads what the file said.
        assert_eq!(car.torque_nm[5], 216.0);
        assert_eq!(car.rpm[2], 7000.0);
    }

    /// A value dragged back to where it started is not a change, and the Save button must not say
    /// it is.
    #[test]
    fn only_a_real_difference_counts_as_dirty() {
        let car = sample();
        let mut edits = BTreeMap::new();
        assert_eq!(dirty_count(Some(&car), &edits), 0);
        edits.insert(CarField::IdleRpm, 800.0);
        assert_eq!(dirty_count(Some(&car), &edits), 0, "the same number is not an edit");
        edits.insert(CarField::IdleRpm, 900.0);
        assert_eq!(dirty_count(Some(&car), &edits), 1);
        edits.insert(CarField::MassKg, 1100.0);
        assert_eq!(dirty_count(Some(&car), &edits), 2);
        // With no record open there is nothing to differ from.
        assert_eq!(dirty_count(None, &edits), 0);
    }

    /// Edits belong to one car, are **kept** when the user looks at another, and Save writes only
    /// the one on screen.
    ///
    /// This is a reversal, and a deliberate one. The rule used to be that switching cars *dropped*
    /// the edits, which was right when a switch meant opening a different `GEOMETRY.BIN` — nobody
    /// does that by accident. Now that the screen has a picker over all 46 records, a switch is a
    /// click, and throwing away work on a click is the wrong trade. What has to stay true either way
    /// is the thing this test really guards: a pending `idle_rpm` for one car must never be applied
    /// to another.
    #[test]
    fn edits_are_kept_per_car_and_never_cross() {
        let mut state = State::default();
        state.default_to("240SX");
        assert_eq!(state.selected(), Some("240SX"));
        state.edits.entry("240SX".into()).or_default().insert(CarField::IdleRpm, 900.0);
        assert_eq!(state.pending(), vec![(CarField::IdleRpm, 900.0)]);

        // Another car sees none of it, and gets its own.
        state.car = "SUPRA".into();
        assert!(state.pending().is_empty(), "no car inherits another's edits");
        assert_eq!(state.others_pending(), 1, "and the 240SX's are still there, and said so");
        state.edits.entry("SUPRA".into()).or_default().insert(CarField::MassKg, 1400.0);
        assert_eq!(state.pending(), vec![(CarField::MassKg, 1400.0)]);

        // Going back finds them where they were left.
        state.car = "240SX".into();
        assert_eq!(state.pending(), vec![(CarField::IdleRpm, 900.0)]);

        // Revert — and a save — clear only the car they were for.
        state.clear_edits();
        assert!(state.pending().is_empty());
        state.car = "SUPRA".into();
        assert_eq!(state.pending(), vec![(CarField::MassKg, 1400.0)], "the other car is untouched");

        // And `default_to` never overrides a choice already made.
        state.default_to("GTO");
        assert_eq!(state.selected(), Some("SUPRA"));
    }

    /// Rear-drive fraction reads as the three words the design's row expects, and is editable.
    #[test]
    fn drive_type_is_named_from_the_fraction() {
        let mut car = sample();
        let row = &SECTIONS[0].params[2];
        car.rear_drive = 0.0;
        assert_eq!(cell(row, Some(&car), 0).text().as_deref(), Some("0.00 FWD"));
        car.rear_drive = 0.5;
        assert_eq!(cell(row, Some(&car), 0).text().as_deref(), Some("0.50 AWD"));
        car.rear_drive = 1.0;
        assert_eq!(cell(row, Some(&car), 0).text().as_deref(), Some("1.00 RWD"));
        let Cell::Num(num) = cell(row, Some(&car), 0) else { panic!("editable") };
        assert_eq!(num.field, CarField::RearDrive);
    }

    /// The two tyre rows write the axle they name, and each keeps its own pair together.
    #[test]
    fn the_tyre_rows_write_one_axle_each() {
        let car = sample();
        let front = &SECTIONS[5].params[3];
        let rear = &SECTIONS[5].params[4];
        assert_eq!(cell(front, Some(&car), 0).text().as_deref(), Some("205"));
        assert_eq!(cell(rear, Some(&car), 0).text().as_deref(), Some("225"));
        let Cell::Num(f) = cell(front, Some(&car), 0) else { panic!("editable") };
        let Cell::Num(r) = cell(rear, Some(&car), 0) else { panic!("editable") };
        assert_eq!((f.field, f.mirror), (CarField::TyreWidthMm(0), Some(CarField::TyreWidthMm(1))));
        assert_eq!((r.field, r.mirror), (CarField::TyreWidthMm(2), Some(CarField::TyreWidthMm(3))));
    }

    /// `--section` finds a section the ways someone would actually type it, and refuses the rest.
    #[test]
    fn a_section_is_found_by_name() {
        // The three spellings of one row, all landing on it.
        for q in ["torque", "TORQUE_CURVE", "[::TORQUE_CURVE]", "Torque_Cur"] {
            assert_eq!(section_index(q), Some(3), "{q}");
        }
        assert_eq!(section_index("vehicle"), Some(0));
        assert_eq!(section_index("steering"), Some(8));
        // A prefix that is ambiguous takes the first, which is the order the screen draws them in.
        assert_eq!(section_index("t"), Some(3), "[::TORQUE_CURVE] comes before [::TIRES]");

        // An index still works, and is bounded by the table rather than by a literal.
        assert_eq!(section_index("0"), Some(0));
        assert_eq!(section_index(&(SECTIONS.len() - 1).to_string()), Some(SECTIONS.len() - 1));
        assert_eq!(section_index(&SECTIONS.len().to_string()), None, "one past the end");

        // And a miss is a miss — the caller warns rather than quietly showing section zero.
        assert_eq!(section_index("gearboxes"), None, "a prefix of the query is not a match");
        assert_eq!(section_index("nope"), None);
        assert_eq!(section_index(""), None);
        assert_eq!(section_index("[::]"), None);
    }

    /// The torque rows carry engine speeds, and only when a car is open.
    ///
    /// The nine values are written out rather than recomputed from `rpm`, so this says what the
    /// labels *should* read for a 240SX instead of re-running the screen's own arithmetic against
    /// itself. Whether the axis is the right one is settled next door, in `gizmo_nfs::globalb`.
    #[test]
    fn torque_rows_are_labelled_by_rpm() {
        let torque = SECTIONS[3].params;
        let car = sample();
        let labels: Vec<String> = torque.iter().map(|p| row_label(p, Some(&car))).collect();
        assert_eq!(
            labels,
            [
                "800 rpm", "1575 rpm", "2350 rpm", "3125 rpm", "3900 rpm", "4675 rpm", "5450 rpm",
                "6225 rpm", "7000 rpm"
            ]
        );

        // With nothing open there is no axis, so the rows keep the design's own key. An earlier
        // draft would have printed `NaN rpm` here, which is worse than saying nothing.
        assert_eq!(row_label(&torque[0], None), "trq_pt_1");
        // And every other row is its label whether or not a car is open.
        let mass = &SECTIONS[1].params[0];
        assert_eq!(row_label(mass, Some(&car)), "mass");
        assert_eq!(row_label(mass, None), "mass");
    }

    /// The power readout is the figure the game's dynamometer prints — 115.8 kW for a stock 240SX —
    /// and it climbs with the level rather than staying put.
    #[test]
    fn the_power_readout_matches_the_dynamometer() {
        let car = sample();
        let (kw, rpm) = car.peak_kw(0);
        assert!((kw - 115.86).abs() < 0.01, "{kw} kW against the dyno's 115.8");
        assert_eq!(rpm, 5450.0);
        let mut last = kw;
        for level in 1..4 {
            let (kw, _) = car.peak_kw(level);
            assert!(kw > last, "level {level} is {kw} kW, no more than the level below");
            last = kw;
        }
        assert!((car.peak_kw(3).0 - 144.8).abs() < 0.2, "{} kW", car.peak_kw(3).0);
    }

    /// `gizmo_nfs::Unproven` is `#[non_exhaustive]` — the parser correctly refusing to be built from
    /// outside — so a test that wants one takes it from a synthetic record through the parser.
    fn unproven(
        cg_height: f32,
        brake_bias: f32,
        spring: [f32; 2],
        damper: [f32; 2],
    ) -> gizmo_nfs::Unproven {
        let mut b = vec![0u8; 0x890 + 16];
        b[..5].copy_from_slice(b"TESTX");
        b[0x20..0x25].copy_from_slice(b"TESTX");
        let path = b"CARS\\TESTX\\GEOMETRY.BIN";
        b[0x40..0x40 + path.len()].copy_from_slice(path);
        let mut put = |o: usize, v: f32| b[o..o + 4].copy_from_slice(&v.to_le_bytes());
        put(0x388, cg_height);
        put(0x38c, brake_bias);
        put(0x1ec, spring[0]);
        put(0x20c, spring[1]);
        put(0x1f0, damper[0]);
        put(0x210, damper[1]);
        gizmo_nfs::globalb::find_car(&b, "TESTX").expect("the synthetic record parses").handling.unproven
    }

    /// A stand-in shaped like the 240SX's record: a stock five-speed that gains a sixth by L3, and
    /// the record's own 34 % / 68 % / 100 % ladder of a 25 % maximum.
    fn sample() -> Car {
        let five = (4.0, 5, [3.321, 1.902, 1.308, 1.0, 0.9, 0.0]);
        let six = (3.5, 6, [3.321, 1.902, 1.308, 1.09, 0.92, 0.8]);
        let torque = [140.0, 150.0, 160.0, 180.0, 200.0, 216.0, 203.0, 170.0, 150.0];
        let scale = |k: f32| {
            let mut out = [0.0f32; 9];
            for (o, t) in out.iter_mut().zip(torque.iter()) {
                *o = t * k;
            }
            out
        };
        Car {
            name: "240SX".into(),
            mass_kg: 1220.0,
            rear_drive: 1.0,
            rpm: [800.0, 6500.0, 7000.0],
            torque_nm: torque,
            gain_nm: [scale(0.25), scale(0.085), scale(0.17), scale(0.25)],
            torque_rpm: [
                800.0, 1575.0, 2350.0, 3125.0, 3900.0, 4675.0, 5450.0, 6225.0, 7000.0,
            ],
            tyre_width_mm: [205.0, 205.0, 225.0, 225.0],
            steer_ratio: 1.1,
            gearbox: [five, five, five, six],
            // The 240SX's own candidate lanes, as the record holds them.
            unproven: unproven(0.5, 0.535, [1.70, 1.67], [1.40, 1.40]),
        }
    }
}
