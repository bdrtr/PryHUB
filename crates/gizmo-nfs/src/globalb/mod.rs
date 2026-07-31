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

// Lanes with a *candidate* meaning and no proof. See `Unproven`, which is where each one's
// evidence, its rival readings and the experiment that would settle it are written down.
const U_ANGLE_284: usize = 0x284;
const U_CG_HEIGHT: usize = 0x388;
const U_BRAKE_BIAS: usize = 0x38C;
/// Two eight-lane blocks, read as front then rear.
const U_SUSPENSION_AT: [usize; 2] = [0x1E0, 0x200];
const U_SPRING: usize = 0x0C;
const U_DAMPER: usize = 0x10;

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
    /// That text also places this field: it is one of **ten tuning sliders** the game names, and it
    /// is the first of them found in this record. The others are worth looking for nearby — and
    /// looking for them by name is what produced [`Unproven`], which is where the rest of that list
    /// has got to.
    pub steer_ratio: f32,
    /// Lanes that read plausibly and are not proved. Behind a type whose name says so.
    pub unproven: Unproven,
}

/// Lanes that read plausibly and are **not proved**.
///
/// Everything else this module exposes is either measured across the install or was settled by
/// driving. These five were not, and they are a separate struct rather than five more fields of
/// [`CarHandling`] for exactly that reason: a consumer has to reach past a type whose name says
/// "unproven" to show one, so an unproven number cannot be printed beside a proved one by accident.
///
/// **One has already left.** `+0x284` was read as a steering lock, and the experiment refused it —
/// see [`Self::angle_284_deg`], which keeps the lane and the refutation rather than deleting both.
/// That is the point of this struct: a candidate that survives contact is promoted, a candidate that
/// does not leaves a record of having been tried, and neither outcome is a number quietly printed
/// beside a measured one.
///
/// **They exist because the game says they should.** `LANGUAGES/English.bin` carries the tuning
/// screen's help text, and it names ten sliders: front and rear springs, front and rear shocks,
/// front and rear sway bars, steering ratio, tyre grip, downforce, ride height, brake bias, gear
/// ratios and final drive. Only two of those — the steering ratio and the gearboxes — are proved
/// here. The rest have to be somewhere, and looking for a *named* thing is a different search from
/// sweeping 222 unread lanes for something interesting.
///
/// **Two of them were found by someone else, and the cross-check cut both ways.**
/// [NFSU2Forge](https://github.com/justlucasgomes/NFSU2Forge) reads this same record with offsets
/// relative to the manufacturer string, which is `+0xC0` here, so its claims translate directly.
/// Where it agrees with this crate — mass, the rpm limits, the torque curve — two independent reads
/// landing on one lane is worth having. Where it does not, the disagreements were checkable and
/// three of them resolve against it: it calls `+0x380` a brake force, which is the steering
/// multiplier settled by driving; it calls a wheel-entry lane front and rear *grip*, which reads
/// `+0.730, −0.730, −0.730, +0.730` across the four corners and so is a mirrored coordinate rather
/// than a grip, and whose "rear" is the front-right wheel; and its reverse ratio is the first lane
/// of the *next* transmission block. What survived that is below.
///
/// None of these is claimed. Each one names what would settle it, which is always the same
/// experiment: change it with `ug2 tune`, install, drive.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Unproven {
    /// `+0x284`, in what look like degrees. Named for its offset because the one name anybody has
    /// proposed for it has been **tested and refused**.
    ///
    /// It was read as a steering lock, and that reading is now dead three times over:
    ///
    /// * The 240SX's was set from 37 to **12** — a third of the angle — installed, and driven. The
    ///   car steered exactly as before.
    /// * The 14 traffic vehicles all read exactly **100.0** while a BUS reads **75**. A taxi does
    ///   not out-lock a bus.
    /// * A HUMMER reads 60 where a PEUGOT 106 reads 27, which is the wrong way round: small cars
    ///   have more lock, not less.
    ///
    /// The null is worth more than a null usually is here, because it was run against a **positive
    /// control** in the same session. This crate had never actually confirmed that the game reads
    /// the plain `.lzc` it writes — that was inherited from an older note which turned out to be
    /// describing its own output. So the 240SX's mass was tripled to 3,660 kg alongside this edit,
    /// installed together, and driven: the car was unmistakably heavy. The channel carries, the car
    /// was the right one, and the steering did not move. Compare the earlier brake-bias null, which
    /// has none of that behind it and is why [`Self::brake_bias_f`] is still a candidate.
    ///
    /// What remains true is that the lane is **not nothing**: 27 to 60 over the playable cars, 15
    /// distinct values, and a second angle-shaped lane four on at `+0x290` (20–45) that moves with
    /// it. Something reads these. It is not the player's steering.
    pub angle_284_deg: f32,
    /// `+0x388` — the middle of a triple of fractions at `+0x384`, `+0x388`, `+0x38C`.
    ///
    /// **For:** the strongest ordering of any lane here. 0.39 (COROLLA) to 0.67 (HUMMER) over the
    /// playable cars, and every traffic vehicle is exactly 0.300 — a per-class default. Sorted, it
    /// puts the SUVs at the top and the hatchbacks at the bottom, which is what a centre-of-gravity
    /// height does and what very little else does.
    ///
    /// **Against:** a 240SX's was raised from 0.50 to **1.40** — nearly roof height — installed
    /// and driven, and cornering felt unchanged. The drive carried its own control this time: the
    /// rev limiter was cut to 3,200 in the same write, and the car audibly stopped at 3,200, so
    /// the file did reach the game and the null is not a plumbing null.
    ///
    /// **Why the row stays a candidate anyway, and this is the honest reading rather than the
    /// convenient one.** "Cornering felt unchanged" is a statement about what NFSU2's handling
    /// model *does with* a centre of gravity, not about what the lane is. This game does not let a
    /// car roll over in ordinary driving at all, so the most obvious consequence of a high centre
    /// of gravity is one it may simply never express. That is the same shape of error as the
    /// brake-bias null next door, which watched stopping distance when a bias moves force between
    /// axles — a real measurement of the wrong observable. Set against it, the file evidence is
    /// the strongest of any lane here.
    ///
    /// **What would settle it now:** something that shows weight *transfer* rather than roll —
    /// nose-dive under hard braking, squat under acceleration, or how a drift breaks away — with
    /// the value driven to both extremes rather than raised once.
    pub cg_height: f32,
    /// `+0x38C`, the third of that triple.
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
    /// **Settles it:** set it to 0.02 again and brake hard mid-corner. A car that spins is a bias.
    pub brake_bias_f: f32,
    /// `+0x1EC` and `+0x20C` — the fourth lane of two eight-lane blocks at `+0x1E0` and `+0x200`,
    /// read as front then rear.
    ///
    /// **For:** the game names "Front Springs" and "Rear Springs" as two of its ten sliders, so
    /// there are two of these to find. The blocks are identical in shape (`12.0, 1.0, 1.0, x, y,
    /// 0, 0, 0`), differ only in the two lanes read here, and 17 and 18 distinct values run 1.43 to
    /// 1.90 with every traffic vehicle above the playable range — a bus at 3.0.
    ///
    /// **Against:** which block is the front axle is an assumption, not a reading. A second, higher
    /// pair sits at `+0x650`/`+0x670` (1.88 / 1.65 where these are 1.70 / 1.40), which may be the
    /// upgraded suspension the game sells — or may be something else entirely.
    ///
    /// **Settles it:** set one to 5.0 and drive over a kerb. Front and rear are told apart by which
    /// end of the car stops absorbing it.
    pub spring: [f32; 2],
    /// `+0x1F0` and `+0x210`, the lane after each spring. The game's "Front Shocks" and "Rear
    /// Shocks". Same evidence and the same doubt as [`Self::spring`], which it sits beside.
    pub damper: [f32; 2],
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
        unproven: Unproven {
            angle_284_deg: le_f32(b, rec + U_ANGLE_284)?,
            cg_height: le_f32(b, rec + U_CG_HEIGHT)?,
            brake_bias_f: le_f32(b, rec + U_BRAKE_BIAS)?,
            spring: [
                le_f32(b, rec + U_SUSPENSION_AT[0] + U_SPRING)?,
                le_f32(b, rec + U_SUSPENSION_AT[1] + U_SPRING)?,
            ],
            damper: [
                le_f32(b, rec + U_SUSPENSION_AT[0] + U_DAMPER)?,
                le_f32(b, rec + U_SUSPENSION_AT[1] + U_DAMPER)?,
            ],
        },
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
