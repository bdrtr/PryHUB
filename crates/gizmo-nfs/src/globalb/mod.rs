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
/// and, just as importantly, what is not. Aero, brakes, steering, tyre grip and the torque curve's
/// rpm axis are **not in this file**; they are absent rather than unread, and this struct does not
/// pretend otherwise by carrying zeroed fields for them.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CarHandling {
    /// The rpm limits.
    pub engine: Engine,
    /// Nine points of the torque curve in **N·m**, stored in the file as kN·m the same way mass is
    /// stored in tonnes. It rises to an interior peak and falls again in all 46 records. **The rpm
    /// axis is not stored** — only these nine magnitudes — so anything that plots it is choosing an
    /// axis, not reading one.
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
