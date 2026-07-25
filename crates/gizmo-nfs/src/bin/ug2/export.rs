//! `ug2 export` — a car (or one configuration of it) as glTF and/or OBJ + MTL + PNG.
//!
//! Both formats by default: the `.glb` is one self-contained file with its textures inside it,
//! the OBJ is what the older tools around this game read. Neither is a conversion of the other —
//! they are two renderings of the same [`MaterialPlan`].
//!
//! Point it at a `CARS/` folder and it exports every car in it, each into its own subdirectory.
//! A car that fails is reported and the batch carries on — one unreadable model out of forty is
//! not a reason to have exported nothing — but the command still exits non-zero, so a script is
//! never told a partial run succeeded.
//!
//! A folder runs on several threads, because exporting is pure CPU work over independent cars. The
//! output stays in **car order** regardless: each car's lines are collected and printed as soon as
//! every car before it has finished, so a run is reproducible and a diff of two runs is empty. That
//! costs a little latency on the first line and buys output a script can rely on.

use crate::paths::{Car, Result};
use gizmo_nfs::export::{self, MaterialPlan};
use gizmo_nfs::parts::{group_of, select_car, CarConfig, Grp};
use gizmo_nfs::{NfsMeshPart, NfsTexture};
use std::fmt::Write as _;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Threads a folder export uses when `--jobs` is not given.
///
/// Capped rather than "all of them": each worker holds a car's file plus its parsed geometry, which
/// is tens of megabytes, and this project's dev machine has 13 GB. Eight is well inside that and
/// already saturates the disk.
fn default_jobs() -> usize {
    std::thread::available_parallelism().map_or(4, |n| n.get().min(8))
}

pub fn run(
    path: &Path,
    out: &Path,
    config: CarConfig,
    all: bool,
    want_textures: bool,
    format: &str,
    jobs: Option<usize>,
) -> Result<()> {
    let cars = Car::resolve_all(path)?;
    // One car keeps the old shape: its files go straight into `out`, not into `out/<NAME>/`.
    if let [only] = &cars[..] {
        let report = one(only, out, &config, all, want_textures, format)?;
        outln!("{}", report.trim_end());
        return Ok(());
    };

    // ── A folder of cars: each into its own subdirectory, named after the car ──
    let workers = jobs.unwrap_or_else(default_jobs).clamp(1, cars.len());
    let next = AtomicUsize::new(0);
    let mut done: Vec<Option<Result<String>>> = vec![None; cars.len()];
    std::thread::scope(|scope| {
        let (results, collected) = std::sync::mpsc::channel::<(usize, Result<String>)>();
        for _ in 0..workers {
            let results = results.clone();
            let next = &next;
            let cars = &cars;
            let config = &config;
            scope.spawn(move || {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(car) = cars.get(i) else { break };
                    let report =
                        one(car, &out.join(&car.name), config, all, want_textures, format);
                    if results.send((i, report)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(results);
        // Printed in car order, not completion order: a run has to be reproducible. Whatever has
        // arrived and has no unfinished car before it goes out immediately, so this still streams.
        let mut printed = 0usize;
        for (i, report) in collected {
            done[i] = Some(report);
            while printed < done.len() {
                match &done[printed] {
                    Some(Ok(text)) => outln!("{}", text.trim_end()),
                    Some(Err(e)) => eprintln!("ug2: {e}"),
                    None => break,
                }
                printed += 1;
            }
        }
    });

    let failed: Vec<String> = cars
        .iter()
        .zip(&done)
        .filter(|(_, report)| matches!(report, Some(Err(_)) | None))
        .map(|(car, _)| car.name.clone())
        .collect();
    outln!(
        "{}/{} cars written into {} on {workers} threads",
        cars.len() - failed.len(),
        cars.len(),
        out.display()
    );
    if !failed.is_empty() {
        outln!("  failed: {}", failed.join(", "));
        return Err(format!("{} of {} cars failed", failed.len(), cars.len()));
    }
    Ok(())
}

/// One car into one directory, returning the lines it would have printed.
///
/// Returning rather than printing is what lets a folder run on several threads and still come out
/// in car order.
fn one(
    car: &Car,
    out: &Path,
    config: &CarConfig,
    all: bool,
    want_textures: bool,
    format: &str,
) -> Result<String> {
    let parts = car.parts()?;
    let selected: Vec<&NfsMeshPart> = if all {
        parts.iter().collect()
    } else {
        // Skip the parts that are never drawn (engine bay, underbody, livery decals): they
        // would import as geometry buried inside the body.
        select_car(&parts, config).into_iter().filter(|p| group_of(&p.name) != Grp::Skip).collect()
    };
    if selected.is_empty() {
        let siblings = car.siblings();
        if siblings.is_empty() {
            return Err(format!("{}: nothing to export", car.name));
        }
        return Err(format!(
            "{}: its GEOMETRY.BIN holds no parts — this directory is a set, try one of: {}",
            car.name,
            siblings.join(", ")
        ));
    }
    let tpk = want_textures.then(|| car.textures()).flatten();

    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let (want_obj, want_glb) = (format != "glb", format != "obj");
    let mtl_name = format!("{}.mtl", car.name);
    let obj_name = format!("{}.obj", car.name);
    let glb_name = format!("{}.glb", car.name);

    // ── Materials: one per (texture, shader) pair a run resolves to ──
    let plan = MaterialPlan::build(&selected, tpk.as_ref());

    if want_glb {
        let glb = export::write_glb(&selected, tpk.as_ref()).map_err(|e| format!("{glb_name}: {e}"))?;
        write(&out.join(&glb_name), &glb)?;
    }

    let mut written = 0usize;
    if want_obj {
        let obj_text = export::write_obj(&selected, &mtl_name, |p, run| plan.name_for(p, run));
        let mtl_text = export::write_mtl(&plan.materials);
        write(&out.join(&obj_name), obj_text.as_bytes())?;
        write(&out.join(&mtl_name), mtl_text.as_bytes())?;

        // Only the OBJ needs the images beside it; the `.glb` carries its own.
        if let Some(tpk) = &tpk {
            let dir = out.join("tex");
            std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            for hash in &plan.textures {
                if let Some(t) = tpk.texture(*hash) {
                    write_png(&dir.join(export::png_name(t)), t)?;
                    written += 1;
                }
            }
        }
    }

    let tris: usize = selected.iter().map(|p| p.triangle_count()).sum();
    let mut report = String::new();
    let _ = writeln!(
        report,
        "{}: {} parts, {tris} triangles, {} materials, {} textures",
        car.name,
        selected.len(),
        plan.materials.len(),
        plan.textures.len()
    );
    if want_glb {
        let _ = writeln!(report, "  {}", out.join(&glb_name).display());
    }
    if want_obj {
        let _ = writeln!(report, "  {}", out.join(&obj_name).display());
        let _ = writeln!(report, "  {}", out.join(&mtl_name).display());
    }
    if written > 0 {
        let _ = writeln!(report, "  {}/*.png", out.join("tex").display());
    }
    Ok(report)
}

fn write_png(path: &Path, t: &NfsTexture) -> Result<()> {
    let bytes = export::png_bytes(t).map_err(|e| format!("{}: {e}", path.display()))?;
    write(path, &bytes)
}

fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}
