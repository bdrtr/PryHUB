//! Parsing NFSU2's global car database (`GLOBAL/GLOBALB.BUN`).
//!
//! A per-car `GEOMETRY.BIN` carries only *one* wheel mesh; the game reads where to place it at
//! the four corners — plus the wheel radius, mass and much else — from a `CarTypeInfo` record in
//! the global bundle. Each record is `0x890` bytes and begins with the car's collection name, so
//! records are located by their `CARS\<name>\GEOMETRY.BIN` path signature and read by fixed
//! field offsets. Layout is from NFSTools/GlobalLib (`Support.Underground2` CarTypeInfo
//! disassembler); validated against real files (e.g. the 240SX's track = 1.64 m matches its
//! known body width).
//!
//! Axes (car space): **XValue = longitudinal** (fore/aft, + front − rear), **YValue = lateral**
//! (track, + left − right), **RideHeight = vertical**, **Diameter = wheel radius (metres)**.

pub mod carparts;
pub mod edit;
pub mod install;

const REC_SIZE: usize = 0x890;
const OFF_NAME2: usize = 0x20;
const OFF_PATH: usize = 0x40;
const OFF_MANUFACTURER: usize = 0xC0;
const OFF_WHEELS: usize = 0x120;
const WHEEL_STRIDE: usize = 0x30;
const OFF_MASS: usize = 0x220;

// Field offsets within one wheel entry.
const W_FORE_AFT: usize = 0x00; // XValue
const W_RIDE_HEIGHT: usize = 0x08;
const W_RADIUS: usize = 0x10; // Diameter (actually the radius, in metres)
const W_TYRE_WIDTH: usize = 0x14; // metres; 17 distinct values, 0.165–0.315
const W_LATERAL: usize = 0x1C; // YValue

// Handling fields, all measured against a real install's 46 records — see `CarHandling`.
const OFF_BODY: usize = 0x224; // length, width, height (m)
const OFF_RPM: usize = 0x300; // idle, red line, limiter
const OFF_TORQUE: usize = 0x310; // 9 × f32, kN·m
/// Steering response, as a multiplier. 1.00–1.25 across the install, and **confirmed in the game**:
/// see [`CarHandling::steer_ratio`].
const OFF_STEER: usize = 0x380;
/// The four transmission blocks: stock, then the three upgrade levels.
const GEARBOX_AT: [usize; 4] = [0x2C0, 0x460, 0x4A0, 0x4E0];
/// The four nine-point torque tables that sit past the transmissions — see
/// [`CarHandling::torque_gain_nm`] for what they are and how that was established.
const TORQUE_GAIN_AT: [usize; 4] = [0x530, 0x570, 0x5B0, 0x5F0];
// Field offsets within one transmission block.
const G_FINAL_DRIVE: usize = 0x08;
const G_REAR_DRIVE: usize = 0x10; // 0 = FWD, 1 = RWD; only the stock block is read
const G_COUNT: usize = 0x18;
const G_RATIOS: usize = 0x20; // reverse, neutral, then up to six forward

// The tuning lanes, read off `SPEED2.EXE`'s own slider-range and physics code — see `Chassis` for
// the evidence. Every one is a pair, front then rear, except the brakes.
/// The two axle blocks, eight lanes each: rise, spring, bump, rebound, sway bar, then the travel
/// pair (compression stop, extension stop). The eighth is not read by the force code.
const SUSPENSION_AT: [usize; 2] = [0x280, 0x2A0];
const S_RISE: usize = 0x00;
const S_SPRING: usize = 0x04;
const S_SHOCK: usize = 0x08;
const S_SWAY: usize = 0x10;
const S_TRAVEL: usize = 0x14;
/// The two tyre blocks: slip angle, load, lateral scale, then the grip pair at `+0x0C`/`+0x10`.
const GRIP_AT: [usize; 2] = [0x1E0, 0x200];
const G_SLIP: usize = 0x00;
const G_LOAD: usize = 0x04;
const G_LATERAL: usize = 0x08;
const G_GRIP: usize = 0x0C;
const OFF_DOWNFORCE: usize = 0x374;
const OFF_RIDE_DROP: usize = 0x714;
/// Brake torque, then handbrake torque — one of each for the whole car.
const OFF_BRAKE: usize = 0x384;

// Lanes with a *candidate* meaning and no proof. See `Unproven`, which is where each one's
// evidence, its rival readings and the experiment that would settle it are written down.
const U_BRAKE_BIAS: usize = 0x38C;

/// One wheel's mount, in NFSU2 car space (metres).
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct WheelSpec {
    /// Longitudinal (fore/aft) offset from the car origin: `+` toward the front, `−` the rear.
    pub fore_aft: f32,
    /// Lateral (track) offset: `+` is the left side, `−` the right.
    pub lateral: f32,
    /// Vertical height of the wheel centre.
    pub ride_height: f32,
    /// Wheel radius in metres.
    pub radius: f32,
}

/// The engine's three rpm limits.
///
/// Locked on every one of a real install's 46 records: strictly increasing 46/46, every value an
/// exact multiple of 50, `idle` only ever 800, 850 or 1000, and `limiter − red_line` exactly 500 for
/// the 31 playable cars and exactly 1000 for the 15 traffic ones — a split that falls out of the
/// arithmetic rather than being imposed on it.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Engine {
    /// Idle speed.
    pub idle_rpm: f32,
    /// Where the tachometer turns red.
    pub red_line_rpm: f32,
    /// Where fuel is cut. Always above [`Self::red_line_rpm`].
    pub limiter_rpm: f32,
}

/// One transmission, at one upgrade level.
///
/// The file stores eight ratio slots: reverse (always negative), neutral (always exactly `0.0`),
/// then [`Self::count`] forward gears, then zeros. All four invariants hold across 184 blocks
/// (46 cars × 4 levels), as does "the forward gears strictly descend".
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Gearbox {
    /// Final-drive ratio. Stored twice in the block and equal in all 184.
    pub final_drive: f32,
    /// Reverse ratio, negative as the file stores it.
    pub reverse: f32,
    /// Forward ratios, highest first. Only the first [`Self::count`] are meaningful.
    pub forward: [f32; 6],
    /// How many of [`Self::forward`] the car has: 3–6.
    pub count: usize,
}

impl Gearbox {
    /// The forward ratios the car actually has.
    #[must_use]
    pub fn gears(&self) -> &[f32] {
        &self.forward[..self.count.min(self.forward.len())]
    }
}

/// What a car's physics record says about how it drives.
///
/// Read from the same `CarTypeInfo` record as the rest — see the module note for what is in there
/// and, just as importantly, what is not. Aero, brakes and tyre grip are **not in this file**; they
/// are absent rather than unread, and this struct does not pretend otherwise by carrying zeroed
/// fields for them.
///
/// The torque curve's rpm axis is a third thing again: not stored, and not lost either. It is
/// *derived* from two fields that are — see [`Self::torque_rpm`], which is a method rather than a
/// field for exactly that reason.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CarHandling {
    /// The rpm limits.
    pub engine: Engine,
    /// Nine points of the torque curve in **N·m**, stored in the file as kN·m the same way mass is
    /// stored in tonnes. It rises to an interior peak and falls again in all 46 records.
    ///
    /// The rpm they sit at is **not stored beside them** and is not missing either: see
    /// [`CarHandling::torque_rpm`].
    pub torque_nm: [f32; 9],
    /// Four more nine-point torque tables, in the same **N·m** as [`Self::torque_nm`], which the
    /// record keeps past its transmissions at `+0x530`, `+0x570`, `+0x5B0` and `+0x5F0`.
    ///
    /// **What the file says, over all 46 records.** Take each table's best-fit multiple of the car's
    /// own stock curve. Then in every one of the 31 playable cars, without exception:
    /// `k[1] / k[3]` is `0.340` and `k[2] / k[3]` is `0.680`, and table 0 is table 3 **byte for
    /// byte**. The 15 traffic vehicles have no torque curve at all and these are zero there too. A
    /// graduated 34 % / 68 % / 100 % series is what an upgrade ladder looks like and is not what a
    /// coincidence looks like, three decimal places wide across 31 cars.
    ///
    /// **They are stored curves, not a stored scalar.** For most cars the shape is the stock one
    /// scaled, which is why the multiple fits at all — but A3's worst point is 26 % off its own
    /// best fit and SKYLINE's is 51 % off, so the game cannot be reconstructing these from one
    /// number per car. That also settles a warning left in `ug2 poke`: the table at `+0x530` is a
    /// quarter of the 240SX's curve, and reading that one car as the pattern would have named a
    /// scalar where there are four tables.
    ///
    /// **Read as a gain rather than a replacement**, because the sizes leave nothing else: a 240SX's
    /// largest is 25 % of its own curve, an ESCALADE's 5 %. A car whose curve was *replaced* by
    /// table 3 would lose three quarters of its engine on its last upgrade. Added, a fully built
    /// 240SX makes 1.25× its stock torque, which is the size of gain NFSU2's engine packages are
    /// sold as.
    ///
    /// **What is not proved here is which upgrade drives which table.** That the ladder exists is a
    /// fact about the file; that index `n` is the game's engine level `n` is a reading of it, and
    /// the experiment that settles it is the one `ug2 poke` was built for — change one, install,
    /// and look at the dynamometer with the package fitted. [`Self::torque_at`] applies the reading
    /// so that it can be checked rather than assumed.
    pub torque_gain_nm: [[f32; 9]; 4],
    /// Stock, then the three upgrade levels, in the order the game's tuning screen offers them.
    pub gearbox: [Gearbox; 4],
    /// Fraction of drive to the rear axle: `0.0` front-wheel drive, `1.0` rear, between the two
    /// all-wheel. Partitions the playable cars exactly by their real drivetrains.
    pub rear_drive: f32,
    /// Body box in metres — length, width, height.
    ///
    /// Proved rather than guessed: the 4×4 inertia tensor beside it is the closed form for a uniform
    /// cuboid, `diag = m/12 · (W²+H², L²+H², L²+W²)`, to a worst relative error of 7.6e-08 over all
    /// 46 records. Nothing but the right L, W, H and mass reproduces that.
    pub body_m: [f32; 3],
    /// Tyre width per corner in metres, in the same order as [`CarTypeInfo::wheels`].
    pub tyre_width_m: [f32; 4],
    /// How much the car turns for a given input, as a multiplier — 1.00 to 1.25 across the install,
    /// 1.10 for a 240SX.
    ///
    /// **This one was settled by driving.** An earlier sweep concluded steering was not in this
    /// game's files, and it was looking for the right thing in the wrong units: the only steering
    /// *angles* are a global ±43 identical in all 46 records, so nothing angle-shaped could be
    /// per-car. This is not an angle. Set to 0.05 on a 240SX and installed, the car barely turns at
    /// all — which is what a steering multiplier does and what no other subsystem does.
    ///
    /// The name was held at arm's length for a while, because the candidate word `SteeringRatio`
    /// came from a modding tool and the evidence was only "the car stopped turning" — which supports
    /// a multiplier on steering response, not the precise quantity that name implies. **The game
    /// settles it in its own words.** `LANGUAGES/English.bin` carries the tuning screen's help text:
    ///
    /// > Steering ratio determines how quickly the wheels of car respond to steering input. Use the
    /// > slider to adjust the sensitivity of your steering.
    ///
    /// So "steering ratio" is this game's own term for response sensitivity rather than for a
    /// geometric ratio, which is exactly what the driving showed. Name and meaning both come from
    /// the game now, and neither is borrowed.
    ///
    /// That text also places this field: it is one of the game's **tuning sliders**, and it was the
    /// first of them found in this record. The rest were found later and by a different route —
    /// reading the code that sets each slider's range — and are in [`Chassis`]. That code agrees
    /// with the drive: slider `8` takes its range from this lane, with `+0x6CC` as its upper end.
    pub steer_ratio: f32,
    /// Suspension, tyres, downforce, ride height and brakes — the lanes the game's own slider and
    /// physics code locate in this record.
    pub chassis: Chassis,
    /// Lanes that read plausibly and are not proved. Behind a type whose name says so.
    pub unproven: Unproven,
}

/// The tuning sliders' lanes, located by **the game's code** rather than by sweeping the file.
///
/// # How these were found
///
/// `SPEED2.EXE` (v1.2) keeps the `CarTypeInfo` array behind a global pointer and sets every Dyno
/// tuning slider's range in one function: `switch (slider)` over ids `0..=0x18`, each case reading
/// named lanes of the car's record and widening a `[min, max]` with them. The slider ids are named by
/// `LANGUAGES/English.bin`, whose tuning labels sit in id order — and the cases the code makes
/// identifiable on its own pin that order down: `8` reads `+0x380`, the steering ratio already
/// settled by driving; `9` reads `+0x2C8` and `+0x4E8`, the stock and third-level **final drive**;
/// `10..=15` are converted as gear ratios; `0x17`/`0x18` loop over the nine-point ECU and turbo
/// torque tables. What sits between is then read rather than guessed:
///
/// | slider | lanes (front, rear) | upper end |
/// |---|---|---|
/// | `0`/`4` springs | `+0x284`, `+0x2A4` | `+0x6D4`, `+0x6F4` |
/// | `1`/`5` shocks | `+0x288`/`+0x28C`, `+0x2A8`/`+0x2AC` | `+0x6D8`/`+0x6DC`, `+0x6F8`/`+0x6FC` |
/// | `2`/`6` sway bars | `+0x290`, `+0x2B0` | `+0x6E0`, `+0x700` |
/// | `3`/`7` ride height | `−(+0x714)`, `−(+0x718)`, and `0` | — |
/// | `0x13`/`0x14` tyre grip | `+0x1EC`/`+0x1F0`, `+0x20C`/`+0x210` | `+0x65C`/`+0x660`, `+0x67C`/`+0x680` |
/// | `0x15`/`0x16` downforce | `+0x374`, `+0x378` | plus two upgrade lanes each |
///
/// Ids `0..=3` are one axle and `4..=7` the other: the default-value function links them in exactly
/// those pairs, and the front is read as the lower id because the labels say *Front* first.
///
/// The upper-end lanes are where the slider stops, and the game shifts that range by the
/// performance parts fitted, so they are not exposed here as "the upgraded value".
///
/// # What the physics does with them
///
/// The slider code says which lane a slider owns; the **physics** says what the number is. Both are
/// read from the same executable, and the second settles what the first left open — which shock lane
/// is bump, which grip lane is the peak — and adds the lanes no slider touches.
///
/// **Units: tonnes, metres, seconds, kilonewtons** — the same "kilo" convention as mass and torque.
/// A spring of `37` is 37 kN/m. The physics does not read the record itself: it copies `+0x110` to
/// `+0x3C0` and lerps the fitted performance parts into the copy, toward targets in `+0x650..+0x718`
/// — the same block the slider ranges end in.
///
/// *Suspension* (one object per wheel, force each frame), with `x` the compression in metres from the
/// static position and `v` its rate:
///
/// ```text
/// x clamped to travel; below the extension stop the wheel is in the air and the force is 0
/// F = (max(x,0)·rise + 1)·spring·x  +  (v > 0 ? bump : rebound)·v
///   + the axle's share of the car's weight  +  sway_bar·(x − x of the other wheel on the axle)
/// F = max(F, 0)
/// ```
///
/// The weight share is added separately, so the spring does not hold the car up — it resists
/// leaving the static position. A 240SX's front reads as a damping ratio of about 0.8.
///
/// *Tyre.* Lateral force comes from a table in the executable, indexed by slip angle over
/// [`Self::tyre_slip_deg`] and by `0.8 × load / `[`Self::tyre_load`], scaled by
/// [`Self::tyre_lateral`]. Longitudinal force is the wheel torque over its radius — there is no
/// longitudinal slip curve. A wheel that slides (contact speed off by more than 0.5 m/s, or locked)
/// gives the sliding grip along the slip direction instead, and every wheel is held inside a
/// friction circle of the peak grip times its load.
///
/// The functions, in v1.2: suspension `0x5A8790`, tyre force `0x5A8280`, friction circle
/// `0x59D600`, lateral table `0x59D510`, wheel lock `0x59D400`, the physics copy `0x5B4DC0`.
///
/// # What this overturned
///
/// * `+0x284` was the "angle" that refused to be a steering lock. It is the **front spring**. A
///   HUMMER's 60 against a 106's 34 is a stiffer spring under a heavier car, and the traffic
///   vehicles' flat 100 is a default.
/// * The two lanes at `+0x1EC`/`+0x1F0` were read as the front spring and front damper. They are
///   **one** quantity, the front **tyre grip**: when the slider shows one number the code averages
///   them (`× 0.5`), which is meaningless across a spring and a damper.
/// * `+0x388` was the strongest candidate for a centre-of-gravity height. It is the **handbrake
///   torque**. Raising it to 1.40 and driving left cornering unchanged — which is what a handbrake
///   does when nobody pulls it.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Chassis {
    /// How much stiffer the spring gets as it compresses, front then rear, per metre: the spring
    /// force is `(1 + rise·x)·spring·x`. Around 5–6.
    pub spring_rise: [f32; 2],
    /// Spring stiffness, front then rear, in kN/m. 30–60 over the playable cars; every traffic
    /// vehicle 100.
    pub spring: [f32; 2],
    /// Damping, front then rear, as `[bump, rebound]` in kN·s/m — bump while the wheel compresses,
    /// rebound while it extends. Around 4–7.5. The tuning screen averages the pair.
    pub shock: [[f32; 2]; 2],
    /// Sway-bar stiffness, front then rear, in kN/m of difference between the axle's two wheels.
    /// 0 on every traffic vehicle.
    pub sway_bar: [f32; 2],
    /// Suspension travel, front then rear, as `[compression stop, extension stop]` in metres from the
    /// static position. The second is negative (a 240SX `0.16, −0.16`); past it the wheel is off the
    /// ground.
    pub travel_m: [[f32; 2]; 2],
    /// The slip angle the lateral table is laid over, front then rear, in degrees. The table reaches
    /// its last column here and stays there. 12 in every record.
    pub tyre_slip_deg: [f32; 2],
    /// The load the lateral table's load axis is divided by, front then rear, in kN. 1 in every
    /// record.
    pub tyre_load: [f32; 2],
    /// A multiplier on the lateral table, front then rear. 1 in every record.
    pub tyre_lateral: [f32; 2],
    /// Tyre grip, front then rear, as `[peak, sliding]` friction coefficients. The peak bounds every
    /// wheel's friction circle; the sliding one is the force a sliding wheel gives. The peak is the
    /// larger in every one of the 46 records (a 240SX 1.70 against 1.40).
    pub tyre_grip: [[f32; 2]; 2],
    /// Downforce, front then rear. Around 0.14–0.18; units not established.
    pub downforce: [f32; 2],
    /// How far the ride-height slider can lower the car, front then rear, in metres (0.02–0.05).
    /// The slider runs from minus this to zero. 0 on every traffic vehicle.
    pub ride_drop_m: [f32; 2],
    /// Brake torque per wheel at full pedal, in kN·m once the game's fixed `× 4` is applied — the
    /// stored number is what this holds. 0.47–0.75.
    pub brake: f32,
    /// Handbrake torque per wheel at full lever, in kN·m once the game's fixed `× 10` is applied —
    /// again the stored number.
    pub handbrake: f32,
}

/// Lanes that read plausibly and are **not proved**.
///
/// Everything else this module exposes is either measured across the install, settled by driving,
/// or located by the game's own code. What is left here was not, and it is a separate struct rather
/// than another field of [`CarHandling`] for exactly that reason: a consumer has to reach past a
/// type whose name says "unproven" to show one, so an unproven number cannot be printed beside a
/// proved one by accident.
///
/// **Four have already left, promoted.** `+0x284` (once a refused steering lock) and the two
/// "spring"/"damper" pairs were located by `SPEED2.EXE`'s slider-range code, and are now
/// [`Chassis::spring`] and [`Chassis::tyre_grip`]. `+0x388`, the centre-of-gravity candidate, was
/// read in the tyre code as the **handbrake** torque and is now [`Chassis::handbrake`]. That is the
/// point of this struct: a candidate that survives contact is promoted, and one that does not is
/// renamed rather than quietly printed beside a measured one.
///
/// [NFSU2Forge](https://github.com/justlucasgomes/NFSU2Forge) reads this same record with offsets
/// relative to the manufacturer string, which is `+0xC0` here, so its claims translate directly.
/// Where it disagreed with this crate the disagreements were checkable and resolved against it: it
/// calls `+0x380` a brake force, which is the steering multiplier; it calls a wheel-entry lane front
/// and rear *grip*, which reads `+0.730, −0.730, −0.730, +0.730` across the four corners and so is a
/// mirrored coordinate; and its reverse ratio is the first lane of the *next* transmission block.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Unproven {
    /// `+0x38C`, the third of a triple whose first two are [`Chassis::brake`] and
    /// [`Chassis::handbrake`].
    ///
    /// **For:** 0.47–0.60 in all 46 records — a *front fraction* just over half — and it orders by
    /// drivetrain: FWD 0.564, RWD 0.531, AWD 0.530. Front-heavy cars getting more front brake is
    /// what a bias does. The game's own text agrees on the concept: "Brake bias controls how much
    /// braking the front tires do verses the rears."
    ///
    /// **Against:** it was set to 0.02 on a real 240SX, installed and driven, and braking was
    /// unchanged. That is the reason this is still not claimed — but it is a weak reason, because a
    /// bias moves force between axles and does not change **stopping distance**, so the experiment
    /// very likely watched the wrong thing.
    ///
    /// **Neighbours, and a part.** It sits after the brake and handbrake torques, and the brake
    /// upgrade moves it together with them (a 240SX 0.535 → 0.49). But the brake-bias slider's range
    /// is a fixed `0..1` rather than this lane, and the per-wheel brake code reads only a pedal it is
    /// handed — where the pedal is split between axles has not been read.
    ///
    /// **Settles it:** the code that splits the pedal, or set it to 0.02 again and brake hard
    /// mid-corner. A car that spins is a bias.
    pub brake_bias_f: f32,
}

/// Radians per second per rpm — `2π/60`, the conversion torque needs to become power.
const RAD_PER_RPM: f32 = std::f32::consts::TAU / 60.0;

/// Watts in one mechanical horsepower (550 ft·lbf/s), to the precision an `f32` can hold.
const W_PER_HP: f32 = 745.699_9;

/// The number of *intervals* the torque curve's nine points span. See [`CarHandling::torque_rpm`].
const TORQUE_INTERVALS: f32 = 8.0;

/// A point on a car's power curve: where it is, and how much it is.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PowerPoint {
    /// Engine speed, in rpm.
    pub rpm: f32,
    /// Power in watts: `torque × rpm × 2π/60`.
    pub watts: f32,
}

impl PowerPoint {
    /// The same figure in kilowatts, which is the unit this game's dynamometer reads in.
    #[must_use]
    pub fn kw(&self) -> f32 {
        self.watts / 1000.0
    }

    /// …and in mechanical horsepower, which is the unit its car-select screen reads in.
    #[must_use]
    pub fn hp(&self) -> f32 {
        self.watts / W_PER_HP
    }
}

impl CarHandling {
    /// The rpm each of [`Self::torque_nm`]'s nine points sits at: **idle to limiter in eight equal
    /// steps**.
    ///
    /// This is a method and not a field because the file does not contain it. The nine values are
    /// nowhere in the 8 MB bundle — not as `f32`, `u32`, `i32` or `u16` — and neither the 240SX's
    /// step (775) nor its peak-power speed (5450) appears anywhere in its own 2,192-byte record. An
    /// earlier sweep of every 4-aligned lane of all 46 records looked for a nine-wide increasing
    /// run and found none, and that sweep was right: the axis is not a run of stored numbers, it is
    /// arithmetic over two fields that *are* stored.
    ///
    /// **Locked by driving, on two cars.** The game's own dynamometer reads a stock 240SX at
    /// 115.8 kW and a stock Mustang GT at 223.6 kW. This axis reproduces both; the three other
    /// readings anyone would try reproduce neither, so the pair picks one candidate out of four
    /// rather than merely failing to contradict it:
    ///
    /// | axis | 240SX (game: 115.8) | Mustang GT (game: 223.6) |
    /// |---|---|---|
    /// | **idle → limiter, 8 steps** | **115.86 @ 5450** | **223.64 @ 5788** |
    /// | idle → red line, 8 steps | 107.88 @ 5075 | 206.66 @ 5350 |
    /// | 0 → limiter, 8 steps | 111.61 @ 5250 | 219.84 @ 5688 |
    /// | idle → limiter, 9 steps | 104.87 @ 4933 | 202.24 @ 5233 |
    ///
    /// The second car is **not the first one twice**. Its span is 800 → 6500 where the 240SX's is
    /// 800 → 7000, so the step is 712.5 rather than 775 and its peak falls on index 7 rather than
    /// index 6. A formula that happened to fit one span at one index had to fit a different span at
    /// a different index to survive that.
    ///
    /// One loose end is **open, and was briefly written down here as closed**. The 240SX computes
    /// 115.8567 and was read off the game as `115.8`; the Mustang computes 223.638 and reads
    /// `223.6`. Truncation fits both and rounding fits only the second, so this said the readout
    /// truncates. Then `LANGUAGES/English.bin` turned up the game's own format string for that
    /// line — `%$3.1f %s^@%$d rpm` — and `%.1f` **rounds**, which would print `115.9`. So either
    /// the first reading was a digit out or that format is not the one behind it. Unresolved, and
    /// one look at the 240SX's dyno decides it.
    ///
    /// The axis does not rest on it either way: the Mustang reads `223.6` under both conventions,
    /// and the three rival axes are 3.8 to 21 kW away from it.
    ///
    /// What this is *not* is 46 confirmations. Two cars were driven; the other 44 are this formula
    /// applied to their own two numbers, and they are unchecked against a dynamometer.
    #[must_use]
    pub fn torque_rpm(&self) -> [f32; 9] {
        let idle = self.engine.idle_rpm;
        let step = (self.engine.limiter_rpm - idle) / TORQUE_INTERVALS;
        let mut out = [0.0f32; 9];
        for (i, rpm) in out.iter_mut().enumerate() {
            *rpm = idle + i as f32 * step;
        }
        out
    }

    /// The curve as `(rpm, N·m)` pairs — [`Self::torque_rpm`] against [`Self::torque_nm`].
    #[must_use]
    pub fn torque_curve(&self) -> [(f32, f32); 9] {
        let rpm = self.torque_rpm();
        let mut out = [(0.0, 0.0); 9];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = (rpm[i], self.torque_nm[i]);
        }
        out
    }

    /// The torque curve at engine upgrade `level`: `0` stock, `1`–`3` with the matching table of
    /// [`Self::torque_gain_nm`] added.
    ///
    /// The addition is the *reading* set out on that field, applied in one place so that a drive can
    /// disagree with it. Level 0 is the stock curve unchanged, which is the one case that is not a
    /// reading of anything. Anything above 3 is clamped to 3 rather than indexing off the end.
    #[must_use]
    pub fn torque_at(&self, level: usize) -> [f32; 9] {
        let mut out = self.torque_nm;
        if level == 0 {
            return out;
        }
        let gain = self.torque_gain_nm[level.min(3)];
        for (t, g) in out.iter_mut().zip(gain.iter()) {
            *t += g;
        }
        out
    }

    /// [`Self::peak_power`] for the curve at engine upgrade `level`.
    #[must_use]
    pub fn peak_power_at(&self, level: usize) -> PowerPoint {
        let rpm = self.torque_rpm();
        let torque = self.torque_at(level);
        let mut best = PowerPoint { rpm: rpm[0], watts: torque[0] * rpm[0] * RAD_PER_RPM };
        for i in 1..9 {
            let watts = torque[i] * rpm[i] * RAD_PER_RPM;
            if watts > best.watts {
                best = PowerPoint { rpm: rpm[i], watts };
            }
        }
        best
    }

    /// The highest-power point **of the nine**, which is what confirmed the axis.
    ///
    /// "Of the nine" is a stated convention rather than a discovered one. If the game reads the
    /// curve as nine samples, this is its peak; if it interpolates linearly between them, power is
    /// piecewise quadratic and its maximum can fall *between* two points — which, measured over
    /// this install, it would for 8 of the 46 cars. The 240SX is not one of them: on both segments
    /// either side of point 6 the parabola's own vertex lies outside the segment, so the peak sits
    /// on the node whichever way the game evaluates it. The one dynamometer reading available
    /// therefore cannot tell the two conventions apart, and this reports the one the file's own
    /// nine points support.
    #[must_use]
    pub fn peak_power(&self) -> PowerPoint {
        let curve = self.torque_curve();
        // Seeded from the first point rather than from zero, so a car whose curve is all zeroes —
        // or a record holding a NaN — still names a point instead of an rpm nothing sits at.
        let (rpm, torque) = curve[0];
        let mut best = PowerPoint { rpm, watts: torque * rpm * RAD_PER_RPM };
        for &(rpm, torque) in curve.iter().skip(1) {
            let watts = torque * rpm * RAD_PER_RPM;
            if watts > best.watts {
                best = PowerPoint { rpm, watts };
            }
        }
        best
    }
}

/// The subset of a `CarTypeInfo` record needed to place and size a car: its name, the four
/// wheel mounts (front-left, front-right, rear-right, rear-left — file order), mass, and what the
/// record says about how the car drives.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct CarTypeInfo {
    /// Collection name, e.g. `"MUSTANGGT"` (matches the `CARS/<name>/` folder).
    pub name: String,
    /// Manufacturer/display name.
    pub manufacturer: String,
    /// Front-left, front-right, rear-right, rear-left.
    pub wheels: [WheelSpec; 4],
    /// Mass in kilograms.
    pub mass_kg: f32,
    /// Engine limits, torque curve, gearboxes and the body box.
    pub handling: CarHandling,
}

#[inline]
fn le_f32(b: &[u8], off: usize) -> Option<f32> {
    b.get(off..off + 4).map(|s| f32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

#[inline]
fn le_u32(b: &[u8], off: usize) -> Option<u32> {
    b.get(off..off + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

/// Read a fixed-width, NUL-terminated ASCII field.
fn cstr(b: &[u8], off: usize, max: usize) -> String {
    let s = b.get(off..(off + max).min(b.len())).unwrap_or(&[]);
    let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    String::from_utf8_lossy(&s[..end]).into_owned()
}

/// A plausible car collection name: non-empty, short, and only `A–Z 0–9 _`.
fn is_car_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 13
        && s.bytes().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_')
}

fn read_wheel(b: &[u8], base: usize) -> Option<WheelSpec> {
    Some(WheelSpec {
        fore_aft: le_f32(b, base + W_FORE_AFT)?,
        lateral: le_f32(b, base + W_LATERAL)?,
        ride_height: le_f32(b, base + W_RIDE_HEIGHT)?,
        radius: le_f32(b, base + W_RADIUS)?,
    })
}

/// Read the `CarTypeInfo` record beginning at `rec`, or `None` if it doesn't validate.
fn read_record(b: &[u8], rec: usize) -> Option<CarTypeInfo> {
    if rec + REC_SIZE > b.len() {
        return None;
    }
    let name = cstr(b, rec, 0x10);
    // The name is stored twice (0x00 and 0x20); require both to agree — a strong record check.
    if !is_car_name(&name) || cstr(b, rec + OFF_NAME2, 0x10) != name {
        return None;
    }
    let mut wheels = [WheelSpec { fore_aft: 0.0, lateral: 0.0, ride_height: 0.0, radius: 0.0 }; 4];
    for (i, w) in wheels.iter_mut().enumerate() {
        *w = read_wheel(b, rec + OFF_WHEELS + i * WHEEL_STRIDE)?;
    }
    Some(CarTypeInfo {
        name,
        manufacturer: cstr(b, rec + OFF_MANUFACTURER, 0x10),
        wheels,
        // Stored as mass × 1/1000 (a Mustang reads 1.560 → 1560 kg).
        mass_kg: le_f32(b, rec + OFF_MASS)? * 1000.0,
        handling: read_handling(b, rec)?,
    })
}

/// Read one 64-byte transmission block.
fn read_gearbox(b: &[u8], base: usize) -> Option<Gearbox> {
    let count = le_u32(b, base + G_COUNT)? as usize;
    let mut forward = [0.0f32; 6];
    for (i, slot) in forward.iter_mut().enumerate() {
        // Ratio slot 0 is reverse and slot 1 is neutral, so forward gear 1 is slot 2.
        *slot = le_f32(b, base + G_RATIOS + (2 + i) * 4)?;
    }
    Some(Gearbox {
        final_drive: le_f32(b, base + G_FINAL_DRIVE)?,
        reverse: le_f32(b, base + G_RATIOS)?,
        forward,
        count: count.min(forward.len()),
    })
}

/// Read the handling fields of the record beginning at `rec`.
fn read_handling(b: &[u8], rec: usize) -> Option<CarHandling> {
    let mut torque_nm = [0.0f32; 9];
    for (i, t) in torque_nm.iter_mut().enumerate() {
        // Stored in kN·m, the same "kilo" convention mass uses.
        *t = le_f32(b, rec + OFF_TORQUE + i * 4)? * 1000.0;
    }
    let mut gearbox = [read_gearbox(b, rec + GEARBOX_AT[0])?; 4];
    for (i, g) in gearbox.iter_mut().enumerate() {
        *g = read_gearbox(b, rec + GEARBOX_AT[i])?;
    }
    let mut torque_gain_nm = [[0.0f32; 9]; 4];
    for (block, table) in torque_gain_nm.iter_mut().enumerate() {
        for (i, t) in table.iter_mut().enumerate() {
            // Same kilo convention as the stock curve beside them.
            *t = le_f32(b, rec + TORQUE_GAIN_AT[block] + i * 4)? * 1000.0;
        }
    }
    let mut tyre_width_m = [0.0f32; 4];
    for (i, w) in tyre_width_m.iter_mut().enumerate() {
        *w = le_f32(b, rec + OFF_WHEELS + i * WHEEL_STRIDE + W_TYRE_WIDTH)?;
    }
    Some(CarHandling {
        engine: Engine {
            idle_rpm: le_f32(b, rec + OFF_RPM)?,
            red_line_rpm: le_f32(b, rec + OFF_RPM + 4)?,
            limiter_rpm: le_f32(b, rec + OFF_RPM + 8)?,
        },
        torque_nm,
        torque_gain_nm,
        gearbox,
        rear_drive: le_f32(b, rec + GEARBOX_AT[0] + G_REAR_DRIVE)?,
        body_m: [
            le_f32(b, rec + OFF_BODY)?,
            le_f32(b, rec + OFF_BODY + 4)?,
            le_f32(b, rec + OFF_BODY + 8)?,
        ],
        tyre_width_m,
        steer_ratio: le_f32(b, rec + OFF_STEER)?,
        chassis: read_chassis(b, rec)?,
        unproven: Unproven { brake_bias_f: le_f32(b, rec + U_BRAKE_BIAS)? },
    })
}

/// Read the tuning-slider lanes of the record beginning at `rec`. See [`Chassis`].
fn read_chassis(b: &[u8], rec: usize) -> Option<Chassis> {
    let f = |at: usize| le_f32(b, rec + at);
    let axle = |at: [usize; 2], lane: usize| Some([f(at[0] + lane)?, f(at[1] + lane)?]);
    let pair = |at: usize| Some([f(at)?, f(at + 4)?]);
    Some(Chassis {
        spring_rise: axle(SUSPENSION_AT, S_RISE)?,
        spring: axle(SUSPENSION_AT, S_SPRING)?,
        shock: [pair(SUSPENSION_AT[0] + S_SHOCK)?, pair(SUSPENSION_AT[1] + S_SHOCK)?],
        sway_bar: axle(SUSPENSION_AT, S_SWAY)?,
        travel_m: [pair(SUSPENSION_AT[0] + S_TRAVEL)?, pair(SUSPENSION_AT[1] + S_TRAVEL)?],
        tyre_slip_deg: axle(GRIP_AT, G_SLIP)?,
        tyre_load: axle(GRIP_AT, G_LOAD)?,
        tyre_lateral: axle(GRIP_AT, G_LATERAL)?,
        tyre_grip: [pair(GRIP_AT[0] + G_GRIP)?, pair(GRIP_AT[1] + G_GRIP)?],
        downforce: pair(OFF_DOWNFORCE)?,
        ride_drop_m: pair(OFF_RIDE_DROP)?,
        brake: f(OFF_BRAKE)?,
        handbrake: f(OFF_BRAKE + 4)?,
    })
}

/// Parse every `CarTypeInfo` record from a decompressed `GLOBALB.BUN`.
///
/// Records are found by their `CARS\<name>\GEOMETRY.BIN` path signature (at record offset
/// `0x40`) and validated by the doubly-stored name, so the scan tolerates the surrounding
/// database chunks without decoding them.
#[must_use]
pub fn parse_cartypeinfos(globalb: &[u8]) -> Vec<CarTypeInfo> {
    located_cartypeinfos(globalb).into_iter().map(|(_, info)| info).collect()
}

/// Every record, with the byte offset it begins at.
///
/// The offsets exist for the write path in [`edit`], which has to put a number back exactly where it
/// read one. They are deliberately produced by the **same scan** the reader uses rather than by the
/// `0x00034600` chunk's own arithmetic: two ways of locating the same record are two chances to
/// disagree, and a disagreement here writes into the car next door.
#[must_use]
pub fn located_cartypeinfos(globalb: &[u8]) -> Vec<(usize, CarTypeInfo)> {
    let mut out = Vec::new();
    let mut seen_end = 0usize; // avoid overlapping matches
    let needle = b"CARS";
    let mut i = 0usize;
    while i + OFF_PATH < globalb.len() {
        let Some(rel) = globalb[i..].windows(needle.len()).position(|w| w == needle) else {
            break;
        };
        let path_pos = i + rel;
        i = path_pos + 1;
        if path_pos < OFF_PATH {
            continue;
        }
        let rec = path_pos - OFF_PATH;
        if rec < seen_end {
            continue;
        }
        // Confirm this is the GEOMETRY path field, not an incidental "CARS".
        let path = cstr(globalb, path_pos, 0x30);
        if !path.contains("GEOMETRY") {
            continue;
        }
        if let Some(info) = read_record(globalb, rec) {
            seen_end = rec + REC_SIZE;
            out.push((rec, info));
        }
    }
    out
}

/// Find one car's record by collection name (case-sensitive, e.g. `"240SX"`).
#[must_use]
pub fn find_car(globalb: &[u8], name: &str) -> Option<CarTypeInfo> {
    parse_cartypeinfos(globalb).into_iter().find(|c| c.name == name)
}

/// Where one car's record begins, by collection name.
#[must_use]
pub fn find_record(globalb: &[u8], name: &str) -> Option<usize> {
    located_cartypeinfos(globalb).into_iter().find(|(_, c)| c.name == name).map(|(at, _)| at)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic 0x890 record with a name, path, and one known wheel, exercising the reader
    /// without shipping any copyrighted game data.
    #[test]
    fn reads_a_synthetic_record() {
        let mut b = vec![0u8; REC_SIZE + 16];
        b[..5].copy_from_slice(b"TESTX");
        b[OFF_NAME2..OFF_NAME2 + 5].copy_from_slice(b"TESTX");
        let path = b"CARS\\TESTX\\GEOMETRY.BIN";
        b[OFF_PATH..OFF_PATH + path.len()].copy_from_slice(path);
        let put = |b: &mut [u8], o: usize, v: f32| b[o..o + 4].copy_from_slice(&v.to_le_bytes());
        // Front-left wheel.
        put(&mut b, OFF_WHEELS + W_FORE_AFT, 1.44);
        put(&mut b, OFF_WHEELS + W_LATERAL, 0.86);
        put(&mut b, OFF_WHEELS + W_RIDE_HEIGHT, 0.17);
        put(&mut b, OFF_WHEELS + W_RADIUS, 0.34);
        put(&mut b, OFF_MASS, 1.56);

        let cars = parse_cartypeinfos(&b);
        assert_eq!(cars.len(), 1);
        let c = &cars[0];
        assert_eq!(c.name, "TESTX");
        assert!((c.mass_kg - 1560.0).abs() < 1e-3);
        assert!((c.wheels[0].fore_aft - 1.44).abs() < 1e-4);
        assert!((c.wheels[0].lateral - 0.86).abs() < 1e-4);
        assert!((c.wheels[0].radius - 0.34).abs() < 1e-4);
        assert_eq!(find_car(&b, "TESTX").as_ref(), Some(c));
        assert!(find_car(&b, "NOPE").is_none());
    }

    /// Build a record carrying just an engine: the three rpm limits, and nine torque magnitudes in
    /// the **kN·m** the file stores them in.
    fn engine_record(idle: f32, red: f32, limiter: f32, torque_knm: [f32; 9]) -> CarHandling {
        let mut b = vec![0u8; REC_SIZE + 16];
        b[..5].copy_from_slice(b"TESTX");
        b[OFF_NAME2..OFF_NAME2 + 5].copy_from_slice(b"TESTX");
        let path = b"CARS\\TESTX\\GEOMETRY.BIN";
        b[OFF_PATH..OFF_PATH + path.len()].copy_from_slice(path);
        let put = |b: &mut [u8], o: usize, v: f32| b[o..o + 4].copy_from_slice(&v.to_le_bytes());
        put(&mut b, OFF_RPM, idle);
        put(&mut b, OFF_RPM + 4, red);
        put(&mut b, OFF_RPM + 8, limiter);
        for (i, t) in torque_knm.into_iter().enumerate() {
            put(&mut b, OFF_TORQUE + i * 4, t);
        }
        find_car(&b, "TESTX").expect("the synthetic record parses").handling
    }

    /// A record shaped like the 240SX's engine, read back through the parser and then asked for the
    /// axis the file does not store.
    ///
    /// The numbers are the ones the dynamometer check was run on — idle 800, limiter 7000 and the
    /// nine magnitudes that peak at 216 N·m — so this is that check, minus the driving.
    #[test]
    fn the_torque_axis_is_idle_to_limiter_in_eight_steps() {
        let h = engine_record(
            800.0,
            6500.0,
            7000.0,
            [0.140, 0.150, 0.160, 0.180, 0.200, 0.216, 0.203, 0.170, 0.150],
        );
        assert_eq!(
            h.torque_rpm(),
            [800.0, 1575.0, 2350.0, 3125.0, 3900.0, 4675.0, 5450.0, 6225.0, 7000.0]
        );
        // The two ends are the two fields it is built from, exactly — the step divides by 8, which
        // in binary floating point is an exponent shift and loses nothing.
        let axis = h.torque_rpm();
        assert_eq!(axis[0], h.engine.idle_rpm, "the curve starts at idle");
        assert_eq!(axis[8], h.engine.limiter_rpm, "and ends at the limiter");
        assert!(axis.windows(2).all(|w| w[0] < w[1]), "strictly increasing");

        // Peak *torque* is at index 5 (4675 rpm) and peak *power* at index 6 (5450). That the two
        // fall apart is the whole reason one dynamometer reading could pick an axis out of four —
        // if they coincided, every candidate would put the peak on the torque peak and only the
        // rpm printed beside it would differ.
        let peak_torque =
            h.torque_nm.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).unwrap().0;
        assert_eq!(peak_torque, 5, "the curve's largest magnitude is its sixth point");
        let peak = h.peak_power();
        assert_eq!(peak.rpm, 5450.0, "one point past that, where torque is already falling");
        assert!((peak.kw() - 115.86).abs() < 0.01, "{} kW against the dyno's 115.8", peak.kw());
        assert!((peak.hp() - 155.4).abs() < 0.1, "{} hp", peak.hp());

        let curve = h.torque_curve();
        assert_eq!(curve[0], (800.0, 140.0));
        assert_eq!(curve[8].0, 7000.0);
        assert!((curve[5].1 - 216.0).abs() < 0.01, "kN·m came back as N·m");
    }

    /// The second car that was driven, and the reason it is worth a test of its own.
    ///
    /// A Mustang GT's span is 800 → 6500 where the 240SX's is 800 → 7000, so the step is 712.5
    /// rather than 775, and its peak power falls on index **7** rather than index 6. Had the axis
    /// been fitted to the first car it would have had to survive a different span at a different
    /// point, and the game's dynamometer reads exactly what this computes: 223.6 kW.
    #[test]
    fn a_second_span_lands_on_its_own_dynamometer_reading() {
        let h = engine_record(
            800.0,
            6000.0,
            6500.0,
            [0.250, 0.280, 0.320, 0.350, 0.390, 0.425, 0.390, 0.369, 0.320],
        );
        let axis = h.torque_rpm();
        assert_eq!(axis[1] - axis[0], 712.5, "a different step from the 240SX's 775");
        assert_eq!(axis[0], 800.0);
        assert_eq!(axis[8], 6500.0);

        let peak = h.peak_power();
        assert_eq!(peak.rpm, 5787.5, "index 7, where the 240SX peaks at index 6");
        assert!((peak.kw() - 223.638).abs() < 0.01, "{} kW against the dyno's 223.6", peak.kw());

        // What is asserted is the computed figure, not how the game prints it. An earlier version
        // of this test asserted truncation, on the strength of the 240SX reading 115.8 where
        // 115.8567 rounds to 115.9 — and then the game's own format string for that line turned up
        // in `LANGUAGES/English.bin` as `%$3.1f`, which rounds. The printing convention is open;
        // both cars agree with the axis regardless, since 223.638 prints 223.6 either way.
        assert!((peak.kw() - 223.638).abs() < 0.01, "either convention shows 223.6");
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_cartypeinfos(&[]).is_empty());
        assert!(parse_cartypeinfos(&[0u8; 32]).is_empty());
        // "CARS" present but no valid record around it.
        let mut b = vec![0u8; 0x40];
        b.extend_from_slice(b"CARS but not a real path");
        assert!(parse_cartypeinfos(&b).is_empty());
    }
}
