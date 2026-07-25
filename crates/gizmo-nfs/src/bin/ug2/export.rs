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
    let selected = chosen(car, &parts, config, all)?;
    let tpk = want_textures.then(|| car.textures()).flatten();

    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let plan = MaterialPlan::build(&selected, tpk.as_ref());
    let written = write_files(car, out, &selected, tpk.as_ref(), &plan, format)?;
    Ok(report(car, out, &selected, &plan, &written))
}

/// Which parts to write: everything in the file, or the configured car with the never-drawn parts
/// left out.
fn chosen<'a>(
    car: &Car,
    parts: &'a [NfsMeshPart],
    config: &CarConfig,
    all: bool,
) -> Result<Vec<&'a NfsMeshPart>> {
    let selected: Vec<&NfsMeshPart> = if all {
        parts.iter().collect()
    } else {
        // Skip the parts that are never drawn (engine bay, underbody, livery decals): they
        // would import as geometry buried inside the body.
        select_car(parts, config).into_iter().filter(|p| group_of(&p.name) != Grp::Skip).collect()
    };
    if !selected.is_empty() {
        return Ok(selected);
    }
    let siblings = car.siblings();
    if siblings.is_empty() {
        return Err(format!("{}: nothing to export", car.name));
    }
    Err(format!(
        "{}: its GEOMETRY.BIN holds no parts — this directory is a set, try one of: {}",
        car.name,
        siblings.join(", ")
    ))
}

/// What the export wrote, for the report.
struct Written {
    glb: bool,
    obj: bool,
    textures: usize,
}

/// Write the files the format asks for. The `.glb` carries its images inside it, so only the OBJ
/// needs a `tex/` folder beside it.
fn write_files(
    car: &Car,
    out: &Path,
    selected: &[&NfsMeshPart],
    tpk: Option<&gizmo_nfs::Tpk>,
    plan: &MaterialPlan,
    format: &str,
) -> Result<Written> {
    let (want_obj, want_glb) = (format != "glb", format != "obj");
    let mut written = Written { glb: want_glb, obj: want_obj, textures: 0 };

    if want_glb {
        let glb = export::write_glb(selected, tpk)
            .map_err(|e| format!("{}.glb: {e}", car.name))?;
        write(&out.join(format!("{}.glb", car.name)), &glb)?;
    }
    if want_obj {
        let mtl_name = format!("{}.mtl", car.name);
        let obj_text = export::write_obj(selected, &mtl_name, |p, run| plan.name_for(p, run));
        write(&out.join(format!("{}.obj", car.name)), obj_text.as_bytes())?;
        write(&out.join(&mtl_name), export::write_mtl(&plan.materials).as_bytes())?;

        if let Some(tpk) = tpk {
            let dir = out.join("tex");
            std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            for hash in &plan.textures {
                if let Some(t) = tpk.texture(*hash) {
                    write_png(&dir.join(export::png_name(t)), t)?;
                    written.textures += 1;
                }
            }
        }
    }
    Ok(written)
}

/// The lines this car contributes to the run's output.
fn report(
    car: &Car,
    out: &Path,
    selected: &[&NfsMeshPart],
    plan: &MaterialPlan,
    written: &Written,
) -> String {
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
    if written.glb {
        let _ = writeln!(report, "  {}", out.join(format!("{}.glb", car.name)).display());
    }
    if written.obj {
        let _ = writeln!(report, "  {}", out.join(format!("{}.obj", car.name)).display());
        let _ = writeln!(report, "  {}", out.join(format!("{}.mtl", car.name)).display());
    }
    if written.textures > 0 {
        let _ = writeln!(report, "  {}/*.png", out.join("tex").display());
    }
    report
}

fn write_png(path: &Path, t: &NfsTexture) -> Result<()> {
    let bytes = export::png_bytes(t).map_err(|e| format!("{}: {e}", path.display()))?;
    write(path, &bytes)
}

fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}
