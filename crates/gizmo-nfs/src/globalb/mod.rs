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
// Field offsets within one transmission block.
const G_FINAL_DRIVE: usize = 0x08;
const G_REAR_DRIVE: usize = 0x10; // 0 = FWD, 1 = RWD; only the stock block is read
const G_COUNT: usize = 0x18;
const G_RATIOS: usize = 0x20; // reverse, neutral, then up to six forward

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
    /// It is deliberately named for what the experiment showed rather than for what a modding tool
    /// calls it. The candidate word was `SteeringRatio`; the evidence is "the car stopped turning",
    /// and that supports a multiplier on steering response and not the precise quantity a name
    /// implies.
    pub steer_ratio: f32,
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
    /// It also settles the one loose end the first reading left. 115.86 kW displayed as `115.8`,
    /// which looked like the axis being 0.06 out; the Mustang's 223.64 displays as `223.6`, and
    /// **truncation fits both readouts where rounding fits only one**. The gap was the readout
    /// cutting a digit, not the arithmetic. (Only the 240SX says so — the Mustang's figure shows
    /// the same either way.)
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
        gearbox,
        rear_drive: le_f32(b, rec + GEARBOX_AT[0] + G_REAR_DRIVE)?,
        body_m: [
            le_f32(b, rec + OFF_BODY)?,
            le_f32(b, rec + OFF_BODY + 4)?,
            le_f32(b, rec + OFF_BODY + 8)?,
        ],
        tyre_width_m,
        steer_ratio: le_f32(b, rec + OFF_STEER)?,
    })
}

/// Parse every `CarTypeInfo` record from a decompressed `GLOBALB.BUN`.
///
/// Records are found by their `CARS\<name>\GEOMETRY.BIN` path signature (at record offset
/// `0x40`) and validated by the doubly-stored name, so the scan tolerates the surrounding
/// database chunks without decoding them.
#[must_use]
pub fn parse_cartypeinfos(globalb: &[u8]) -> Vec<CarTypeInfo> {
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
            out.push(info);
        }
    }
    out
}

/// Find one car's record by collection name (case-sensitive, e.g. `"240SX"`).
#[must_use]
pub fn find_car(globalb: &[u8], name: &str) -> Option<CarTypeInfo> {
    parse_cartypeinfos(globalb).into_iter().find(|c| c.name == name)
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

        // And the readout **truncates**: 223.638 shows as 223.6 either way, but the 240SX's
        // 115.8567 shows as 115.8 and would show 115.9 if the game rounded. One digit, and it is
        // the difference between "the axis is 0.06 out" and "the axis is exact".
        let trunc = |kw: f32| (kw * 10.0).floor() / 10.0;
        assert_eq!(trunc(peak.kw()), 223.6);
        let s14 = engine_record(
            800.0,
            6500.0,
            7000.0,
            [0.140, 0.150, 0.160, 0.180, 0.200, 0.216, 0.203, 0.170, 0.150],
        );
        assert_eq!(trunc(s14.peak_power().kw()), 115.8, "as the game showed it");
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
