//! Putting a number back into a car's handling record — the far end of the reader next door.
//!
//! # Why this is a lane and not a struct
//!
//! The obvious shape for a write path is "hand me a [`CarTypeInfo`] and I will serialise it", and it
//! is the wrong one here. A record is 2,192 bytes and this crate reads about 130 of them; a
//! serialiser would have to decide what to do with the other 2,060, and the only correct answer —
//! leave them exactly as they were — is what an in-place lane write does by construction. So an edit
//! is a [`Field`] and a number, the same four-byte write `ug2 poke` makes, with the difference that
//! the lane is named rather than typed as a hex offset.
//!
//! That also keeps the one property worth having: **nothing moves**. The bundle carries an embedded
//! `0x80134000` geometry stream whose alignment [`crate::repack`] would re-derive into padding the
//! file never had — rebuilding `GLOBALB.BUN` with no edits at all returns 8,008,120 bytes for an
//! 8,008,064-byte input. A write that changes no lengths needs no theory of the layout it is writing
//! into, and this one refuses to run if the length changed anyway.
//!
//! # What a caller still has to know
//!
//! **The game reads `GLOBAL/GlobalB.lzc`.** `GLOBALB.BUN` sits beside it holding the same 46 records
//! and editing it does nothing at all — established by experiment, not by reading: tripling a 240SX's
//! mass in the `.BUN` was driven and felt like nothing, and the same edit to the same lane of the
//! `.lzc` produced a car that could barely get off the line. The `.lzc` is **JDLZ-compressed** in a
//! pristine install and decompresses byte-for-byte to the `.BUN`, so a caller decompresses, edits,
//! and compresses back. (An earlier note in this repository said the `.lzc` was not compressed. It
//! was describing a file that had already been replaced with a decompressed one.)
//!
//! Nothing here writes a file. It edits a buffer, and says no rather than half-doing it.

use super::{
    read_record, CarTypeInfo, GEARBOX_AT, G_COUNT, G_FINAL_DRIVE, G_RATIOS, G_REAR_DRIVE, OFF_MASS,
    OFF_RPM, OFF_STEER, OFF_TORQUE, OFF_WHEELS, REC_SIZE, TORQUE_GAIN_AT, U_ANGLE_284, U_BRAKE_BIAS,
    U_CG_HEIGHT, U_DAMPER, U_SPRING, U_SUSPENSION_AT, WHEEL_STRIDE, W_RADIUS, W_TYRE_WIDTH,
};
use crate::error::{NfsError, NfsResult};

/// How the number a person reads relates to the number the record stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Store {
    /// Stored as it reads.
    Float,
    /// Stored a thousand times smaller. The record's own "kilo" convention: a 1,220 kg car is
    /// `1.22`, a 216 N·m torque point is `0.216`, a 205 mm tyre is `0.205`.
    Kilo,
    /// Stored as an unsigned integer.
    Count,
}

/// One editable number in a `CarTypeInfo` record, named rather than typed as an offset.
///
/// Every variant here is a lane this crate already **reads**, which is the rule that decides
/// membership: a field nobody can show a value for is a field nobody can sensibly edit, and the
/// 222 lanes whose meaning is unknown stay the business of `ug2 poke`, where writing one is an
/// experiment rather than a setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Field {
    /// Kerb mass, in kilograms.
    MassKg,
    /// Idle speed, rpm.
    IdleRpm,
    /// Where the tachometer turns red, rpm.
    RedLineRpm,
    /// Where fuel is cut, rpm. Also the top of the torque curve's own axis — see
    /// [`super::CarHandling::torque_rpm`], so moving it moves every torque point's engine speed.
    LimiterRpm,
    /// Stock torque curve point `0..9`, in N·m.
    TorqueNm(usize),
    /// A point of one of the four upgrade torque tables, in N·m. See
    /// [`super::CarHandling::torque_gain_nm`].
    TorqueGainNm {
        /// Which table, `0..4`.
        block: usize,
        /// Which of its nine points.
        point: usize,
    },
    /// Fraction of drive to the rear axle. Only the stock transmission block's is read.
    RearDrive,
    /// Steering response multiplier.
    SteerRatio,
    /// Final drive of transmission `level` (`0` stock, `1..4` the upgrades).
    FinalDrive(usize),
    /// Reverse ratio of transmission `level`, negative as the file stores it.
    Reverse(usize),
    /// A forward ratio.
    Gear {
        /// Transmission level, `0..4`.
        level: usize,
        /// Which forward gear, **1-based**, `1..=6`.
        gear: usize,
    },
    /// How many forward gears transmission `level` has.
    GearCount(usize),
    /// Tyre width at corner `0..4`, in millimetres.
    TyreWidthMm(usize),
    /// Wheel radius at corner `0..4`, in metres.
    WheelRadiusM(usize),
    /// The angle-shaped lane at `+0x284`, named for its offset because its one proposed name was
    /// tested and refused. See [`super::Unproven::angle_284_deg`].
    Angle284,
    /// Candidate: centre-of-gravity height. See [`super::Unproven::cg_height`].
    CgHeight,
    /// Candidate: front brake bias. See [`super::Unproven::brake_bias_f`].
    BrakeBiasFront,
    /// Candidate: spring rate of axle `0` (front) or `1` (rear).
    Spring(usize),
    /// Candidate: damping rate of axle `0` (front) or `1` (rear).
    Damper(usize),
}

impl Field {
    /// Byte offset of this field's lane within the 2,192-byte record, or `None` when an index is out
    /// of range — which is the only way to build a `Field` that does not name one.
    #[must_use]
    pub fn lane(self) -> Option<usize> {
        let gearbox = |level: usize| GEARBOX_AT.get(level).copied();
        let at = match self {
            Self::MassKg => OFF_MASS,
            Self::IdleRpm => OFF_RPM,
            Self::RedLineRpm => OFF_RPM + 4,
            Self::LimiterRpm => OFF_RPM + 8,
            Self::TorqueNm(point) if point < 9 => OFF_TORQUE + point * 4,
            Self::TorqueGainNm { block, point } if point < 9 => {
                TORQUE_GAIN_AT.get(block).copied()? + point * 4
            }
            Self::RearDrive => gearbox(0)? + G_REAR_DRIVE,
            Self::SteerRatio => OFF_STEER,
            Self::FinalDrive(level) => gearbox(level)? + G_FINAL_DRIVE,
            Self::Reverse(level) => gearbox(level)? + G_RATIOS,
            // Ratio slot 0 is reverse and slot 1 is neutral, so forward gear 1 is slot 2.
            Self::Gear { level, gear } if (1..=6).contains(&gear) => {
                gearbox(level)? + G_RATIOS + (1 + gear) * 4
            }
            Self::GearCount(level) => gearbox(level)? + G_COUNT,
            Self::TyreWidthMm(corner) if corner < 4 => {
                OFF_WHEELS + corner * WHEEL_STRIDE + W_TYRE_WIDTH
            }
            Self::WheelRadiusM(corner) if corner < 4 => OFF_WHEELS + corner * WHEEL_STRIDE + W_RADIUS,
            Self::Angle284 => U_ANGLE_284,
            Self::CgHeight => U_CG_HEIGHT,
            Self::BrakeBiasFront => U_BRAKE_BIAS,
            Self::Spring(axle) => U_SUSPENSION_AT.get(axle).copied()? + U_SPRING,
            Self::Damper(axle) => U_SUSPENSION_AT.get(axle).copied()? + U_DAMPER,
            _ => return None,
        };
        (at + 4 <= REC_SIZE).then_some(at)
    }

    /// How this field is stored.
    #[must_use]
    pub fn store(self) -> Store {
        match self {
            Self::MassKg
            | Self::TorqueNm(_)
            | Self::TorqueGainNm { .. }
            | Self::TyreWidthMm(_) => Store::Kilo,
            Self::GearCount(_) => Store::Count,
            _ => Store::Float,
        }
    }

    /// The number to put in the file for a value as a person reads it.
    #[must_use]
    pub fn stored(self, shown: f32) -> f32 {
        match self.store() {
            Store::Kilo => shown / 1000.0,
            Store::Float => shown,
            Store::Count => shown.round().clamp(0.0, 6.0),
        }
    }

    /// The inverse: what a stored number reads as.
    #[must_use]
    pub fn shown(self, stored: f32) -> f32 {
        match self.store() {
            Store::Kilo => stored * 1000.0,
            Store::Float | Store::Count => stored,
        }
    }

    /// The name this field is called on the command line, e.g. `gear2@3`.
    ///
    /// Round-trips through [`Self::parse`], which is asserted rather than assumed — a name that does
    /// not read back is a name that silently edits the wrong lane.
    #[must_use]
    pub fn key(self) -> String {
        match self {
            Self::MassKg => "mass".into(),
            Self::IdleRpm => "idle_rpm".into(),
            Self::RedLineRpm => "red_line".into(),
            Self::LimiterRpm => "limiter".into(),
            Self::TorqueNm(point) => format!("torque{}", point + 1),
            Self::TorqueGainNm { block, point } => format!("gain{}_{}", block, point + 1),
            Self::RearDrive => "rear_drive".into(),
            Self::SteerRatio => "steer_ratio".into(),
            Self::FinalDrive(level) => format!("final_drive@{level}"),
            Self::Reverse(level) => format!("reverse@{level}"),
            Self::Gear { level, gear } => format!("gear{gear}@{level}"),
            Self::GearCount(level) => format!("gear_count@{level}"),
            Self::TyreWidthMm(corner) => format!("tyre_width{corner}"),
            Self::WheelRadiusM(corner) => format!("wheel_radius{corner}"),
            Self::Angle284 => "angle284?".into(),
            Self::CgHeight => "cg_height?".into(),
            Self::BrakeBiasFront => "brake_bias?".into(),
            Self::Spring(axle) => format!("spring{axle}?"),
            Self::Damper(axle) => format!("damper{axle}?"),
        }
    }

    /// Read a name back. `None` for anything that does not name a lane.
    ///
    /// A bare `gear3` means the stock transmission's third gear rather than being refused, because
    /// `@0` is noise to type and stock is what someone editing one gear almost always means. Every
    /// other level has to be said.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        let name = name.trim().to_ascii_lowercase();
        let (head, level) = match name.split_once('@') {
            Some((head, level)) => (head, level.parse::<usize>().ok()?),
            None => (name.as_str(), 0),
        };
        let after = |prefix: &str| head.strip_prefix(prefix).and_then(|n| n.parse::<usize>().ok());
        let field = match head {
            "mass" => Self::MassKg,
            "idle_rpm" => Self::IdleRpm,
            "red_line" => Self::RedLineRpm,
            "limiter" => Self::LimiterRpm,
            "rear_drive" => Self::RearDrive,
            "steer_ratio" => Self::SteerRatio,
            "final_drive" => Self::FinalDrive(level),
            // The question mark is part of the name, and deliberately awkward to type: these are
            // candidates, and a command line that spells one should say so.
            "angle284?" => Self::Angle284,
            "cg_height?" => Self::CgHeight,
            "brake_bias?" => Self::BrakeBiasFront,
            "reverse" => Self::Reverse(level),
            "gear_count" => Self::GearCount(level),
            _ => {
                if let Some(point) = after("torque") {
                    Self::TorqueNm(point.checked_sub(1)?)
                } else if let Some(gear) = after("gear") {
                    Self::Gear { level, gear }
                } else if let Some(corner) = after("tyre_width") {
                    Self::TyreWidthMm(corner)
                } else if let Some(corner) = after("wheel_radius") {
                    Self::WheelRadiusM(corner)
                } else if let Some(axle) = head.strip_suffix('?').and_then(|h| {
                    h.strip_prefix("spring").map(|n| (n, 0)).or_else(|| h.strip_prefix("damper").map(|n| (n, 1)))
                }) {
                    let n: usize = axle.0.parse().ok()?;
                    if axle.1 == 0 { Self::Spring(n) } else { Self::Damper(n) }
                } else {
                    let (block, point) = head.strip_prefix("gain")?.split_once('_')?;
                    Self::TorqueGainNm {
                        block: block.parse().ok()?,
                        point: point.parse::<usize>().ok()?.checked_sub(1)?,
                    }
                }
            }
        };
        field.lane().map(|_| field)
    }

    /// Whether this field's meaning is a candidate rather than a claim.
    ///
    /// A caller that shows a value has to be able to ask, so that an unproven lane is never drawn
    /// like a proved one. The naming carries it too — [`Self::key`] ends these in `?`.
    #[must_use]
    pub fn is_unproven(self) -> bool {
        matches!(
            self,
            Self::Angle284
                | Self::CgHeight
                | Self::BrakeBiasFront
                | Self::Spring(_)
                | Self::Damper(_)
        )
    }

    /// Every field of a record, in the order a table would list them. For a caller that wants to
    /// enumerate rather than name.
    #[must_use]
    pub fn all() -> Vec<Self> {
        let mut out = vec![Self::MassKg, Self::IdleRpm, Self::RedLineRpm, Self::LimiterRpm];
        out.extend((0..9).map(Self::TorqueNm));
        out.extend(
            (0..4).flat_map(|block| (0..9).map(move |point| Self::TorqueGainNm { block, point })),
        );
        out.push(Self::RearDrive);
        out.push(Self::SteerRatio);
        for level in 0..4 {
            out.push(Self::FinalDrive(level));
            out.push(Self::GearCount(level));
            out.push(Self::Reverse(level));
            out.extend((1..=6).map(move |gear| Self::Gear { level, gear }));
        }
        out.extend((0..4).map(Self::TyreWidthMm));
        out.extend((0..4).map(Self::WheelRadiusM));
        // Last, and named with a `?`, so that a listing reads proved-then-candidate rather than
        // mixing the two.
        out.extend([Self::Angle284, Self::CgHeight, Self::BrakeBiasFront]);
        out.extend((0..2).map(Self::Spring));
        out.extend((0..2).map(Self::Damper));
        out
    }
}

/// Read one field out of a record that has already been located.
#[must_use]
pub fn get(record: &[u8], field: Field) -> Option<f32> {
    let at = field.lane()?;
    let bytes: [u8; 4] = record.get(at..at + 4)?.try_into().ok()?;
    Some(match field.store() {
        Store::Count => u32::from_le_bytes(bytes) as f32,
        _ => field.shown(f32::from_le_bytes(bytes)),
    })
}

/// One edit: which lane, and the value as a person reads it.
pub type Edit = (Field, f32);

/// Apply a set of edits to one car's record, in place, and say how many lanes changed.
///
/// **The set is refused whole.** Every lane is resolved and every value checked before any of them
/// is written, so a set holding one bad field leaves the buffer exactly as it was rather than
/// half-edited. That is the same rule the texture write path keeps, for the same reason: this is
/// somebody's game.
///
/// Afterwards the record is read back **through the parser** — not merely re-read as bytes — so a
/// write that lands somewhere it should not is caught here rather than by a car that will not load.
pub fn apply(bundle: &mut [u8], car: &str, edits: &[Edit]) -> NfsResult<usize> {
    let rec = super::find_record(bundle, car)
        .ok_or_else(|| NfsError::NoSuchCar { name: car.to_owned() })?;

    // Resolve everything first. A lane that does not exist, or a value that is not a number, stops
    // the whole set before a byte moves.
    let mut writes: Vec<(usize, [u8; 4])> = Vec::with_capacity(edits.len());
    for &(field, value) in edits {
        let at = field.lane().ok_or(NfsError::BufferSizeMismatch {
            detail: "an edit named a lane outside the record",
        })?;
        if !value.is_finite() {
            return Err(NfsError::BufferSizeMismatch {
                detail: "an edit asked for a value that is not a finite number",
            });
        }
        let stored = field.stored(value);
        let bytes = match field.store() {
            Store::Count => (stored as u32).to_le_bytes(),
            _ => stored.to_le_bytes(),
        };
        let lane = rec + at;
        if lane + 4 > bundle.len() {
            return Err(NfsError::UnexpectedEof {
                offset: lane,
                needed: 4,
                remaining: bundle.len().saturating_sub(lane),
            });
        }
        writes.push((lane, bytes));
    }

    let was = bundle.len();
    let mut changed = 0usize;
    for (lane, bytes) in writes {
        let slot = &mut bundle[lane..lane + 4];
        if slot != bytes {
            slot.copy_from_slice(&bytes);
            changed += 1;
        }
    }
    if bundle.len() != was {
        return Err(NfsError::BufferSizeMismatch {
            detail: "an in-place write changed the bundle's length",
        });
    }

    read_record(bundle, rec).ok_or(NfsError::BufferSizeMismatch {
        detail: "the edited record no longer parses",
    })?;
    Ok(changed)
}

/// Every difference between a car's record and the values wanted for it, as edits.
///
/// The interface holds what the user typed; this works out what of it is actually a change, so a
/// Save that alters nothing writes nothing and says so.
#[must_use]
pub fn diff(record: &[u8], wanted: &[Edit]) -> Vec<Edit> {
    wanted
        .iter()
        .copied()
        .filter(|&(field, value)| {
            get(record, field).is_none_or(|was| field.stored(was) != field.stored(value))
        })
        .collect()
}

/// The whole of a car's handling, as the edits that would reproduce it.
///
/// What Revert needs: the numbers a record held when it was opened, in the form Save takes.
#[must_use]
pub fn snapshot(info: &CarTypeInfo) -> Vec<Edit> {
    let h = &info.handling;
    let mut out = vec![
        (Field::MassKg, info.mass_kg),
        (Field::IdleRpm, h.engine.idle_rpm),
        (Field::RedLineRpm, h.engine.red_line_rpm),
        (Field::LimiterRpm, h.engine.limiter_rpm),
        (Field::RearDrive, h.rear_drive),
        (Field::SteerRatio, h.steer_ratio),
    ];
    out.extend(h.torque_nm.iter().enumerate().map(|(i, &t)| (Field::TorqueNm(i), t)));
    for (block, table) in h.torque_gain_nm.iter().enumerate() {
        out.extend(
            table
                .iter()
                .enumerate()
                .map(move |(point, &t)| (Field::TorqueGainNm { block, point }, t)),
        );
    }
    for (level, g) in h.gearbox.iter().enumerate() {
        out.push((Field::FinalDrive(level), g.final_drive));
        out.push((Field::Reverse(level), g.reverse));
        out.push((Field::GearCount(level), g.count as f32));
        out.extend((1..=6).map(move |gear| (Field::Gear { level, gear }, g.forward[gear - 1])));
    }
    out.extend(
        h.tyre_width_m.iter().enumerate().map(|(i, &w)| (Field::TyreWidthMm(i), w * 1000.0)),
    );
    out.extend(info.wheels.iter().enumerate().map(|(i, w)| (Field::WheelRadiusM(i), w.radius)));
    let u = &h.unproven;
    out.push((Field::Angle284, u.angle_284_deg));
    out.push((Field::CgHeight, u.cg_height));
    out.push((Field::BrakeBiasFront, u.brake_bias_f));
    out.extend(u.spring.iter().enumerate().map(|(i, &v)| (Field::Spring(i), v)));
    out.extend(u.damper.iter().enumerate().map(|(i, &v)| (Field::Damper(i), v)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::globalb::{find_car, REC_SIZE};

    /// A synthetic bundle holding one record, shaped the way the reader insists on.
    fn bundle() -> Vec<u8> {
        let mut b = vec![0u8; REC_SIZE + 64];
        b[..5].copy_from_slice(b"TESTX");
        b[0x20..0x25].copy_from_slice(b"TESTX");
        let path = b"CARS\\TESTX\\GEOMETRY.BIN";
        b[0x40..0x40 + path.len()].copy_from_slice(path);
        let put = |b: &mut [u8], o: usize, v: f32| b[o..o + 4].copy_from_slice(&v.to_le_bytes());
        put(&mut b, OFF_MASS, 1.22);
        put(&mut b, OFF_RPM, 800.0);
        put(&mut b, OFF_RPM + 4, 6500.0);
        put(&mut b, OFF_RPM + 8, 7000.0);
        for (i, t) in [0.140, 0.150, 0.160, 0.180, 0.200, 0.216, 0.203, 0.170, 0.150]
            .into_iter()
            .enumerate()
        {
            put(&mut b, OFF_TORQUE + i * 4, t);
        }
        // The four gain tables at their measured 34 % / 68 % / 100 % of a 25 % maximum.
        for (block, k) in [0.25f32, 0.085, 0.17, 0.25].into_iter().enumerate() {
            for i in 0..9 {
                let base = f32::from_le_bytes(
                    b[OFF_TORQUE + i * 4..OFF_TORQUE + i * 4 + 4].try_into().unwrap(),
                );
                put(&mut b, TORQUE_GAIN_AT[block] + i * 4, base * k);
            }
        }
        for (level, &at) in GEARBOX_AT.iter().enumerate() {
            put(&mut b, at + G_FINAL_DRIVE, 4.0 - level as f32 * 0.1);
            put(&mut b, at + G_RATIOS, -3.657);
            b[at + G_COUNT..at + G_COUNT + 4].copy_from_slice(&5u32.to_le_bytes());
            for (i, r) in [3.321, 1.902, 1.308, 1.0, 0.9, 0.0].into_iter().enumerate() {
                put(&mut b, at + G_RATIOS + (2 + i) * 4, r);
            }
        }
        put(&mut b, GEARBOX_AT[0] + G_REAR_DRIVE, 1.0);
        put(&mut b, OFF_STEER, 1.1);
        for corner in 0..4 {
            put(&mut b, OFF_WHEELS + corner * WHEEL_STRIDE + W_TYRE_WIDTH, 0.205);
            put(&mut b, OFF_WHEELS + corner * WHEEL_STRIDE + W_RADIUS, 0.34);
        }
        b
    }

    /// Every name reads back as the field that produced it. A key that does not round-trip is a
    /// command line that edits a lane nobody asked for.
    #[test]
    fn every_field_survives_its_own_name() {
        for field in Field::all() {
            let key = field.key();
            assert_eq!(Field::parse(&key), Some(field), "{key}");
        }
        // The shorthand a person actually types.
        assert_eq!(Field::parse("gear3"), Some(Field::Gear { level: 0, gear: 3 }));
        assert_eq!(Field::parse("  IDLE_RPM "), Some(Field::IdleRpm));
        assert_eq!(Field::parse("torque9"), Some(Field::TorqueNm(8)));
        assert_eq!(Field::parse("gain3_9"), Some(Field::TorqueGainNm { block: 3, point: 8 }));
        // And a name that would land outside the record is not a field.
        assert_eq!(Field::parse("torque0"), None, "the points are 1-based when named");
        assert_eq!(Field::parse("torque10"), None);
        assert_eq!(Field::parse("gear7"), None);
        assert_eq!(Field::parse("gear1@4"), None, "there are four transmissions, 0..3");
        assert_eq!(Field::parse("tyre_width4"), None);
        assert_eq!(Field::parse("nonsense"), None);
        assert_eq!(Field::parse(""), None);
    }

    /// Distinct lanes for distinct fields — the property a table of offsets can quietly break by
    /// getting one stride wrong.
    #[test]
    fn no_two_fields_share_a_lane() {
        let mut seen = std::collections::HashMap::new();
        for field in Field::all() {
            let lane = field.lane().expect("every enumerated field names a lane");
            assert!(lane + 4 <= REC_SIZE, "{} is outside the record", field.key());
            if let Some(other) = seen.insert(lane, field) {
                panic!("{} and {} both write {lane:#05x}", field.key(), other.key());
            }
        }
    }

    /// A write goes in, reads back through the parser, and does not disturb its neighbours.
    #[test]
    fn an_edit_lands_and_reads_back() {
        let mut b = bundle();
        let before = find_car(&b, "TESTX").expect("the synthetic record parses");

        let changed = apply(
            &mut b,
            "TESTX",
            &[
                (Field::IdleRpm, 900.0),
                (Field::TorqueNm(5), 260.0),
                (Field::Gear { level: 3, gear: 6 }, 0.8),
                (Field::GearCount(3), 6.0),
                (Field::MassKg, 1100.0),
            ],
        )
        .expect("the set applies");
        assert_eq!(changed, 5);

        let after = find_car(&b, "TESTX").expect("and still parses");
        assert_eq!(after.handling.engine.idle_rpm, 900.0);
        assert!((after.handling.torque_nm[5] - 260.0).abs() < 0.01, "N·m went in as kN·m");
        assert!((after.mass_kg - 1100.0).abs() < 0.01, "kg went in as tonnes");
        assert_eq!(after.handling.gearbox[3].count, 6);
        assert!((after.handling.gearbox[3].forward[5] - 0.8).abs() < 1e-4);

        // Everything not named is untouched.
        assert_eq!(after.handling.engine.limiter_rpm, before.handling.engine.limiter_rpm);
        assert_eq!(after.handling.torque_nm[4], before.handling.torque_nm[4]);
        assert_eq!(after.handling.gearbox[0].forward, before.handling.gearbox[0].forward);
        assert_eq!(after.wheels, before.wheels);
    }

    /// A set with one bad field writes none of itself.
    #[test]
    fn a_bad_edit_refuses_the_whole_set() {
        let mut b = bundle();
        let before = b.clone();
        let err = apply(
            &mut b,
            "TESTX",
            &[(Field::IdleRpm, 900.0), (Field::TorqueNm(99), 1.0)],
        );
        assert!(err.is_err(), "a lane outside the record is refused");
        assert_eq!(b, before, "and nothing was written");

        assert!(apply(&mut b, "TESTX", &[(Field::IdleRpm, f32::NAN)]).is_err());
        assert_eq!(b, before, "a NaN is refused too");

        // And a car that is not there is named in the error rather than silently doing nothing.
        let err = apply(&mut b, "NOPE", &[(Field::IdleRpm, 900.0)]).expect_err("no such car");
        assert!(format!("{err}").contains("NOPE"), "{err}");
        assert_eq!(b, before);
    }

    /// A snapshot of a record, applied back to it, changes nothing.
    #[test]
    fn a_snapshot_is_a_no_op() {
        let mut b = bundle();
        let info = find_car(&b, "TESTX").expect("parses");
        let edits = snapshot(&info);
        let rec = &b[super::super::find_record(&b, "TESTX").expect("located")..];
        assert!(diff(rec, &edits).is_empty(), "nothing in a snapshot differs from its own record");
        assert_eq!(apply(&mut b, "TESTX", &edits).expect("applies"), 0, "and none of it writes");
        assert_eq!(find_car(&b, "TESTX").as_ref(), Some(&info));
    }

    /// The gain tables read back in N·m and add up the way [`super::super::CarHandling::torque_at`]
    /// says they do.
    #[test]
    fn the_gain_tables_read_as_a_ladder() {
        let b = bundle();
        let h = find_car(&b, "TESTX").expect("parses").handling;
        assert_eq!(h.torque_gain_nm[0], h.torque_gain_nm[3], "table 0 duplicates table 3");
        // 34 % and 68 % of the top table, which is the ratio every playable car in a real install
        // holds to three decimal places.
        for i in 0..9 {
            let top = h.torque_gain_nm[3][i];
            assert!((h.torque_gain_nm[1][i] - top * 0.34).abs() < 0.05, "point {i}");
            assert!((h.torque_gain_nm[2][i] - top * 0.68).abs() < 0.05, "point {i}");
        }
        // Stock is the curve untouched; level 3 is a quarter more of it.
        assert_eq!(h.torque_at(0), h.torque_nm);
        assert!((h.torque_at(3)[5] - 216.0 * 1.25).abs() < 0.5);
        assert!(h.peak_power_at(3).kw() > h.peak_power().kw());
        // Past the end clamps rather than panicking.
        assert_eq!(h.torque_at(9), h.torque_at(3));
    }
}
