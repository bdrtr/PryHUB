//! A car assembled against the install it came from: every wheel in its arch, nothing left on the
//! floor. Synthetic parts can test the arithmetic, but only the real files can say whether the
//! record's mounts and the meshes agree.
//!
//! Skipped unless `NFSU2_ROOT` is set, like `golden_assets`.

use gizmo_nfs::parts::{group_of, rig_car, Grp};
use gizmo_nfs::{select_stock_car, NfsMeshPart};
use std::path::PathBuf;

fn root() -> Option<PathBuf> {
    std::env::var_os("NFSU2_ROOT").map(PathBuf::from)
}

/// The bounds of `parts`, in car space. They are baked or as the file places them.
fn bounds(parts: &[NfsMeshPart]) -> ([f32; 3], [f32; 3]) {
    let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
    for part in parts {
        let c = gizmo_nfs::placement::part_centroid(part);
        let apply = gizmo_nfs::placement::should_place(&part.transform, &c);
        for &p in &part.positions {
            let p = gizmo_nfs::placement::place_point(&part.transform, p, apply);
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
    }
    (lo, hi)
}

#[test]
fn every_recorded_car_stands_on_four_wheels() {
    let Some(root) = root() else {
        eprintln!("NFSU2_ROOT unset — skipping rig test");
        return;
    };
    let bundle = gizmo_nfs::globalb::install::find(&root).expect("the install has a bundle");
    let globalb = gizmo_nfs::globalb::install::read(&bundle).expect("the bundle reads");
    let (mut cars, mut with_wheels) = (0, 0);
    for entry in std::fs::read_dir(root.join("CARS")).expect("CARS").flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(info) = gizmo_nfs::globalb::find_car(&globalb, &name) else { continue };
        let Ok(bytes) = std::fs::read(entry.path().join("GEOMETRY.BIN")) else { continue };
        let Ok(parts) = gizmo_nfs::geometry::parse_geometry(&bytes) else { continue };
        let stock = select_stock_car(&parts);
        if stock.is_empty() {
            continue;
        }
        cars += 1;
        let out = rig_car(&stock, &stock, &info.wheels);

        // Nothing the rig is responsible for is left in the middle of the floor. A wheel or a brake
        // that is still there is the bug this exists for.
        for part in out.iter().filter(|p| group_of(&p.name) == Grp::Wheel || p.name.contains("_BRAK")) {
            if part.name.contains("BRAKELIGHT") {
                continue;
            }
            let (lo, hi) = bounds(std::slice::from_ref(part));
            let middle = (lo[1] + hi[1]) * 0.5;
            assert!(middle.abs() > 0.3, "{name}: {} sits on the centre line at y={middle}", part.name);
        }

        let wheels: Vec<&NfsMeshPart> = out.iter().filter(|p| group_of(&p.name) == Grp::Wheel).collect();
        if wheels.is_empty() {
            continue;
        }
        with_wheels += 1;
        assert_eq!(wheels.len(), 4, "{name}");

        // Each wheel inside the car's own length, and no further out than a wheel's width past its
        // body. A mount read wrong, a flip the wrong way round or the wrong axis all fail this.
        let body: Vec<NfsMeshPart> =
            out.iter().filter(|p| group_of(&p.name) != Grp::Wheel && !p.name.contains("_BRAK")).cloned().collect();
        let (blo, bhi) = bounds(&body);
        for wheel in wheels {
            let (lo, hi) = bounds(std::slice::from_ref(&wheel.clone()));
            assert!(lo[0] > blo[0] && hi[0] < bhi[0], "{name}: {} past the car's ends", wheel.name);
            // The size the record states, within what the meshes' tyres add. A wheel scaled by its
            // own matrix fails this: `BUS` comes out at 2.5 times its record.
            let radius = (hi[2] - lo[2]) * 0.5;
            let stated = info.wheels[0].radius;
            assert!((radius - stated).abs() < stated * 0.25, "{name}: radius {radius} against {stated}");
            assert!(lo[1] > blo[1] - 0.2 && hi[1] < bhi[1] + 0.2, "{name}: {} [{}, {}] outside body [{}, {}]",
                wheel.name, lo[1], hi[1], blo[1], bhi[1]);
        }
    }
    eprintln!("{cars} recorded cars rigged, {with_wheels} with a wheel part");
    assert!(cars >= 40, "only {cars} recorded cars found");
    assert_eq!(with_wheels, cars, "a recorded car with nothing to roll on");
}
