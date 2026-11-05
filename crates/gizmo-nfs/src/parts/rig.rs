//! The parts `GEOMETRY.BIN` models about the origin and never places, put where the game puts them.
//!
//! Selecting a car's parts is not the same as assembling it. A wheel is one mesh with no placement,
//! and so are the brakes and the exhaust tip, and on some cars the spoiler: written out as they stand
//! they all land in the middle of the car's floor, and the car rolls on empty arches. The install
//! does say where they go, just not on the part itself:
//!
//! | part | where it goes | stated by |
//! |---|---|---|
//! | the wheel | four copies, one per mount | `GLOBALB`'s [`WheelSpec`]s |
//! | `FRONT_BRAKE`, `REAR_BRAKE` | inside each wheel of that axle | the wheel's `MARKER_BRAKE`, in the wheel's own space |
//! | the exhaust | one copy per exit | `LEFT_EXHAUST`, `RIGHT_EXHAUST`, on the rear bumper or a widebody's body |
//! | the spoiler | one | `SPOILER`, on `BASE` |
//!
//! Only an *unplaced* brake, exhaust or spoiler is touched, so a file that does place one keeps it.
//!
//! Every copy comes back **baked**: its vertices already in car space and its matrix the identity,
//! so a writer needs no idea that it was ever instanced. The copies get a suffix on the name because
//! both exports key a part's materials and its node by that name.

use super::group::{group_of, Grp};
use crate::globalb::WheelSpec;
use crate::hash::string_hash;
use crate::placement::{part_centroid, place_dir, place_point, should_place};
use crate::types::{NfsMeshPart, IDENTITY};

/// How near the origin a part's placed centroid must be to count as never placed, metres. The
/// unplaced exhausts, brakes and spoilers of an install sit within 0.21 m; a placed one is a metre
/// or more out.
const UNPLACED: f32 = 0.35;

/// The shader of the flat square a wheel carries the size of the tyre's bounding box. The game
/// hides it; drawn, it is a panel standing out past the tyre.
const PLAINNOTHING: u32 = 0x010C_B64A;

/// The name each wheel position is told apart by, in the record's order.
const CORNERS: [&str; 4] = ["FL", "FR", "RR", "RL"];

/// What a part waits to be hung on.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mount {
    Exhaust,
    Spoiler,
    Brake { front: bool },
}

impl Mount {
    /// By stem rather than word, because names arrive truncated (`_FRONT_BRAK`).
    fn of(name: &str) -> Option<Self> {
        if name.contains("BRAKELIGHT") {
            None
        } else if name.contains("_FRONT_BRAK") {
            Some(Self::Brake { front: true })
        } else if name.contains("_REAR_BRAK") {
            Some(Self::Brake { front: false })
        } else if group_of(name) == Grp::Exhaust {
            Some(Self::Exhaust)
        } else if name.contains("_SPOILER") {
            Some(Self::Spoiler)
        } else {
            None
        }
    }
}

/// `selected` assembled: the wheel at all four `mounts`, and the unplaced brakes, exhaust and spoiler
/// where the file's markers say. Everything else passes through in order, untouched.
///
/// Markers and the wheel's frame are looked up in `stock` rather than `selected`, so taking the
/// bumper off a car in the assembly tab does not also take the exhaust that hangs from it. An
/// unplaced part whose marker is nowhere in `stock` is dropped: not drawn at all beats drawn under
/// the floor.
///
/// The wheel mesh is one part however many positions it fills, so every wheel after the first in
/// `selected` is dropped as well.
#[must_use]
pub fn rig_car(selected: &[&NfsMeshPart], stock: &[&NfsMeshPart], mounts: &[WheelSpec; 4]) -> Vec<NfsMeshPart> {
    let frame = stock.iter().find(|p| group_of(&p.name) == Grp::Wheel).copied();
    let mut wheel_done = false;
    let mut out = Vec::with_capacity(selected.len() + 8);
    for &part in selected {
        if group_of(&part.name) == Grp::Wheel {
            if !is_one_wheel(part) {
                out.push(part.clone());
            } else if !wheel_done {
                wheel_done = true;
                out.extend(wheels(part, mounts));
            }
            continue;
        }
        match Mount::of(&part.name).filter(|_| is_unplaced(part)) {
            Some(Mount::Brake { front }) => out.extend(brakes(part, frame, mounts, front)),
            Some(Mount::Exhaust) => {
                let exits = marker_positions(stock, &["LEFT_EXHAUST", "RIGHT_EXHAUST"]);
                for (i, at) in exits.into_iter().enumerate() {
                    // The file's own matrix is dropped: most cars model the pipe along +x with an
                    // identity one and it reads right, while 350Z's stands the pipe on end.
                    let name = format!("{}_{i}", part.name);
                    out.push(bake(part, name, |p| add(p, at), |n| n));
                }
            }
            Some(Mount::Spoiler) => {
                if let Some(&at) = marker_positions(stock, &["SPOILER"]).first() {
                    // A spoiler, unlike the others, keeps the rotation its matrix states.
                    let apply = det_is_usable(part);
                    let origin = [0.0; 3];
                    let held = place_point(&part.transform, origin, apply);
                    out.push(bake(
                        part,
                        part.name.clone(),
                        |p| add(sub(place_point(&part.transform, p, apply), held), at),
                        |n| place_dir(&part.transform, n, apply),
                    ));
                }
            }
            None => out.push(part.clone()),
        }
    }
    out
}

/// Whether a wheel part is a single wheel: a circle about the axle, so centred on `z = 0`.
///
/// Eight traffic vehicles (`BUS`, `PICKUP`, `MINIVAN`, ...) ship something else under the same name,
/// standing on the ground from `z = 0` and spread across the car's centre line. What the game does
/// with it is not known here, so it stays as the file has it rather than being copied four times.
fn is_one_wheel(part: &NfsMeshPart) -> bool {
    placed_centre(part)[2].abs() < 0.05
}

/// The wheel at its four mounts: centred, and with its rim facing out.
fn wheels(part: &NfsMeshPart, mounts: &[WheelSpec; 4]) -> Vec<NfsMeshPart> {
    let part = without_filler(part);
    let apply = should_place(&part.transform, &part_centroid(&part));
    let centre = placed_centre(&part);
    mounts
        .iter()
        .zip(CORNERS)
        .map(|(mount, corner)| {
            let (at, flip) = (mount_point(mount), facing(mount));
            bake(
                &part,
                format!("{}_{corner}", part.name),
                |p| add(flip(sub(place_point(&part.transform, p, apply), centre)), at),
                |n| flip(place_dir(&part.transform, n, apply)),
            )
        })
        .collect()
}

/// One axle's brake inside each of its two wheels.
///
/// The brake's own matrix is dropped: a SKYLINE's rear one turns the disc flat, face up, while the
/// raw geometry is already in the wheel's frame.
fn brakes(part: &NfsMeshPart, frame: Option<&NfsMeshPart>, mounts: &[WheelSpec; 4], front: bool) -> Vec<NfsMeshPart> {
    let part = without_filler(part);
    let centre = frame.map_or([0.0; 3], placed_centre);
    let marker = frame
        .and_then(|w| w.marker("MARKER_BRAKE"))
        .map_or([0.0; 3], crate::NfsMarker::position);
    let axle = if front { [0, 1] } else { [2, 3] };
    axle.iter()
        .map(|&i| {
            let (at, flip) = (mount_point(&mounts[i]), facing(&mounts[i]));
            bake(
                &part,
                format!("{}_{}", part.name, CORNERS[i]),
                |p| add(flip(sub(add(p, marker), centre)), at),
                flip,
            )
        })
        .collect()
}

/// The record's mount as a point in car space.
fn mount_point(m: &WheelSpec) -> [f32; 3] {
    [m.fore_aft, m.lateral, m.ride_height]
}

/// The turn that shows a wheel's rim rather than its back on this side of the car.
///
/// The mesh is modelled for the right flank only: on the left it would show its flat inboard back
/// and stick out of the arch. A half turn about the up axis fixes both, where a reflection would
/// invert the winding and the normals with it.
fn facing(m: &WheelSpec) -> fn([f32; 3]) -> [f32; 3] {
    if m.lateral > 0.0 {
        |[x, y, z]| [-x, -y, z]
    } else {
        |v| v
    }
}

/// The same part with its `PLAINNOTHING` runs gone. A part without a material table has none.
fn without_filler(part: &NfsMeshPart) -> NfsMeshPart {
    let mut out = part.clone();
    if part.materials.iter().all(|m| m.shader.0 != PLAINNOTHING) {
        return out;
    }
    out.indices.clear();
    out.materials.clear();
    for run in part.materials.iter().filter(|m| m.shader.0 != PLAINNOTHING) {
        let Some(slice) = part.indices.get(run.index_offset..run.index_offset + run.index_count) else {
            continue;
        };
        let mut kept = run.clone();
        kept.index_offset = out.indices.len();
        out.indices.extend_from_slice(slice);
        out.materials.push(kept);
    }
    out
}

/// A copy of `part` with `point` and `dir` already applied and nothing left for a writer to apply.
fn bake(
    part: &NfsMeshPart,
    name: String,
    point: impl Fn([f32; 3]) -> [f32; 3],
    dir: impl Fn([f32; 3]) -> [f32; 3],
) -> NfsMeshPart {
    let mut out = part.clone();
    out.name = name;
    out.positions = part.positions.iter().map(|&p| point(p)).collect();
    out.normals = part.normals.iter().map(|&n| dir(n)).collect();
    out.transform = IDENTITY;
    // Their frames were in the old space, and every marker has already been used by now.
    out.markers.clear();
    let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
    for p in &out.positions {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    if !out.positions.is_empty() {
        out.bbox_min = lo;
        out.bbox_max = hi;
    }
    out
}

/// Whether the file left `part` at the origin.
fn is_unplaced(part: &NfsMeshPart) -> bool {
    if part.positions.is_empty() {
        return false;
    }
    let apply = should_place(&part.transform, &part_centroid(part));
    let mut sum = [0.0f32; 3];
    for &p in &part.positions {
        sum = add(sum, place_point(&part.transform, p, apply));
    }
    let n = part.positions.len() as f32;
    (sum[0] / n).hypot(sum[1] / n).hypot(sum[2] / n) < UNPLACED
}

/// The middle of a part's bounds, where its own matrix puts it.
fn placed_centre(part: &NfsMeshPart) -> [f32; 3] {
    let apply = should_place(&part.transform, &part_centroid(part));
    let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
    for &p in &part.positions {
        let p = place_point(&part.transform, p, apply);
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    if part.positions.is_empty() {
        return [0.0; 3];
    }
    [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5]
}

/// Every marker named in `names` across `parts`, in car space, duplicates dropped: a bumper's LODs
/// carry the same markers.
fn marker_positions(parts: &[&NfsMeshPart], names: &[&str]) -> Vec<[f32; 3]> {
    let mut out: Vec<[f32; 3]> = Vec::new();
    for part in parts {
        let apply = should_place(&part.transform, &part_centroid(part));
        for name in names {
            let hash = string_hash(name);
            for marker in part.markers.iter().filter(|m| m.hash.0 == hash) {
                let at = place_point(&part.transform, marker.position(), apply);
                let d = |o: &[f32; 3]| (o[0] - at[0]).hypot(o[1] - at[1]).hypot(o[2] - at[2]);
                if !out.iter().any(|o| d(o) < 1e-3) {
                    out.push(at);
                }
            }
        }
    }
    out
}

/// Whether the part's matrix is a pose at all. A collapsed one is ignored, as [`should_place`] does.
fn det_is_usable(part: &NfsMeshPart) -> bool {
    crate::placement::det3(&part.transform) > 1e-6
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AssetHash, NfsMarker, NfsMaterialRange};

    fn part(name: &str, positions: Vec<[f32; 3]>) -> NfsMeshPart {
        NfsMeshPart {
            name: name.to_owned(),
            normals: vec![[0.0, 1.0, 0.0]; positions.len()],
            positions,
            transform: IDENTITY,
            ..NfsMeshPart::default()
        }
    }

    /// A wheel in the shape the install ships: 0.3 in radius and 0.2 wide, on the +y side of zero.
    fn wheel() -> NfsMeshPart {
        part("CAR_KIT00_FRONT_WHEEL_A", vec![[-0.3, 0.0, -0.3], [0.3, 0.2, 0.3]])
    }

    fn mounts() -> [WheelSpec; 4] {
        let spec = |fore_aft, lateral| WheelSpec { fore_aft, lateral, ride_height: 0.1, radius: 0.3 };
        [spec(1.4, 0.8), spec(1.4, -0.8), spec(-1.1, -0.8), spec(-1.1, 0.8)]
    }

    fn named<'a>(parts: &'a [NfsMeshPart], name: &str) -> &'a NfsMeshPart {
        parts.iter().find(|p| p.name == name).unwrap_or_else(|| panic!("no part named {name}"))
    }

    #[test]
    fn one_wheel_becomes_four_centred_on_their_mounts() {
        let w = wheel();
        let out = rig_car(&[&w], &[&w], &mounts());
        assert_eq!(out.len(), 4);
        for (corner, mount) in CORNERS.iter().zip(mounts()) {
            let copy = named(&out, &format!("CAR_KIT00_FRONT_WHEEL_A_{corner}"));
            assert_eq!(copy.transform, IDENTITY, "a copy leaves nothing for a writer to apply");
            let centre = placed_centre(copy);
            for (got, want) in centre.iter().zip(mount_point(&mount)) {
                assert!((got - want).abs() < 1e-5, "{corner}: {centre:?}");
            }
        }
    }

    /// A half turn, and not a reflection: the wheel's outer face ends up outboard on both sides.
    #[test]
    fn the_rim_faces_out_on_both_flanks() {
        let w = part("CAR_FRONT_WHEEL", vec![[-0.3, 0.0, -0.3], [0.3, 0.2, 0.3], [0.0, 0.0, 0.0]]);
        let out = rig_car(&[&w], &[&w], &mounts());
        // Vertex 2 is on the rim's face, 0.1 from the wheel's middle.
        let rim = |name: &str| named(&out, name).positions[2][1];
        assert!((rim("CAR_FRONT_WHEEL_FR") - -0.9).abs() < 1e-5, "right: {}", rim("CAR_FRONT_WHEEL_FR"));
        assert!((rim("CAR_FRONT_WHEEL_FL") - 0.9).abs() < 1e-5, "left: {}", rim("CAR_FRONT_WHEEL_FL"));
    }

    #[test]
    fn a_second_wheel_part_does_not_make_eight() {
        let (front, rear) = (wheel(), part("CAR_KIT00_REAR_WHEE", vec![[-0.1; 3], [0.1; 3]]));
        let out = rig_car(&[&front, &rear], &[&front, &rear], &mounts());
        assert_eq!(out.len(), 4);
    }

    #[test]
    fn each_axle_gets_its_own_brake_at_the_wheels_marker() {
        let mut w = wheel();
        // The wheel's centre is y = 0.1, so a marker at y = 0.15 is 0.05 outboard of it.
        w.markers.push(NfsMarker::new(AssetHash(string_hash("MARKER_BRAKE")), {
            let mut m = IDENTITY;
            m[3][1] = 0.15;
            m
        }));
        let (front, rear) =
            (part("CAR_KIT00_FRONT_BRAKE_A", vec![[0.0; 3]]), part("CAR_KIT00_REAR_BRAK", vec![[0.0; 3]]));
        let out = rig_car(&[&front, &rear], &[&w, &front, &rear], &mounts());
        let names: Vec<_> = out.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            ["CAR_KIT00_FRONT_BRAKE_A_FL", "CAR_KIT00_FRONT_BRAKE_A_FR", "CAR_KIT00_REAR_BRAK_RR", "CAR_KIT00_REAR_BRAK_RL"],
        );
        let at = |i: usize| out[i].positions[0];
        assert!((at(1)[1] - -0.75).abs() < 1e-5, "front right: {:?}", at(1));
        assert!((at(0)[1] - 0.75).abs() < 1e-5, "front left: {:?}", at(0));
        assert!((at(2)[0] - -1.1).abs() < 1e-5, "the rear axle's mount: {:?}", at(2));
    }

    #[test]
    fn the_filler_square_does_not_come_along() {
        let mut w = wheel();
        w.indices = vec![0, 1, 0, 1, 0, 1];
        let run = |shader, index_offset| NfsMaterialRange {
            hash: AssetHash(1),
            shader: AssetHash(shader),
            index_offset,
            index_count: 3,
        };
        w.materials = vec![run(PLAINNOTHING, 0), run(7, 3)];
        let out = rig_car(&[&w], &[&w], &mounts());
        assert_eq!(out[0].indices.len(), 3);
        assert_eq!(out[0].materials.len(), 1);
        assert_eq!(out[0].materials[0].index_offset, 0, "the kept run moves to where it now starts");
    }

    #[test]
    fn an_exhaust_hangs_once_per_exit_and_not_at_all_without_one() {
        let exhaust = part("CAR_KIT00_EXHAUST_A", vec![[0.0; 3]]);
        let marked = |x: f32, name: &str| {
            let mut m = IDENTITY;
            m[3][0] = x;
            NfsMarker::new(AssetHash(string_hash(name)), m)
        };
        let mut bumper = part("CAR_KIT00_REAR_BUMPER_A", vec![[-2.0, 0.0, 0.0]]);
        bumper.markers = vec![marked(-2.0, "LEFT_EXHAUST"), marked(-2.1, "RIGHT_EXHAUST")];
        let out = rig_car(&[&exhaust], &[&exhaust, &bumper], &mounts());
        assert_eq!(out.len(), 2);
        assert!((out[1].positions[0][0] - -2.1).abs() < 1e-5);

        assert!(rig_car(&[&exhaust], &[&exhaust], &mounts()).is_empty());
    }

    /// The shape eight traffic vehicles ship as `TIRE_FRONT`: on the ground and across the centre.
    #[test]
    fn a_tire_part_that_is_not_one_wheel_is_left_as_filed() {
        let t = part("BUS_TIRE_FRONT_A", vec![[-0.9, -1.3, 0.0], [0.9, 1.3, 0.8]]);
        let out = rig_car(&[&t], &[&t], &mounts());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].positions, t.positions);
    }

    /// A file that places a spoiler itself is right and is left alone.
    #[test]
    fn a_part_the_file_placed_keeps_its_place() {
        let mut spoiler = part("CAR_KIT00_SPOILER_A", vec![[0.0; 3]]);
        spoiler.transform[3][0] = -2.0;
        let body = part("CAR_KIT00_BODY_A", vec![[1.0, 0.0, 0.0]]);
        let out = rig_car(&[&spoiler, &body], &[&spoiler, &body], &mounts());
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].transform, spoiler.transform);
        assert_eq!(out[1].name, "CAR_KIT00_BODY_A");
    }
}
