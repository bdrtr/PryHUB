//! `ug2 tune` — change a car's handling by name, and put it where the game will read it.
//!
//! The difference from [`crate::poke`] is what is being claimed. `poke` writes a lane by hex offset
//! and refuses to say what it is, because its whole purpose is the experiment that finds out. This
//! writes lanes this crate already reads, under the names it reads them by, and so it is a settings
//! editor rather than an instrument.
//!
//! Without `--set` it prints the car, which is also how someone finds out what a field is called.

use crate::paths::Result;
use gizmo_nfs::compression::Codec;
use gizmo_nfs::globalb::{edit, install};
use gizmo_nfs::CarField;
use std::path::{Path, PathBuf};

/// Where the edited bundle goes.
pub enum Target {
    /// Into the game's own `GLOBAL/` files, with a backup taken first.
    Install,
    /// To a file of the caller's choosing, decompressed.
    File(PathBuf),
    /// Nowhere: print what would change and stop.
    DryRun,
}

pub fn run(
    path: &Path,
    car: Option<&str>,
    sets: &[String],
    target: Target,
    keep_codec: bool,
    restore: bool,
    csv: bool,
) -> Result<()> {
    let bundle = install::find(path)
        .ok_or_else(|| format!("{}: no GLOBAL/GlobalB.lzc at or above this", path.display()))?;

    if restore {
        let back = install::restore(&bundle).map_err(|e| format!("{e}"))?;
        if back.is_empty() {
            return Err("no .bak beside either file — nothing to restore".into());
        }
        for p in back {
            outln!("restored {}", p.display());
        }
        return Ok(());
    }

    let mut bytes = install::read(&bundle).map_err(|e| format!("{}: {e}", bundle.lzc.display()))?;

    // No `--car` means every car, which is the reading this command should have started with: the
    // bundle holds all 46 records and the interesting question is almost always how one car's number
    // sits against the other 45.
    let Some(car) = car else {
        if !sets.is_empty() {
            return Err("--set needs a --car; a set applied to all 46 records is not an edit, it is an accident".into());
        }
        return show_all(&bytes, &bundle, csv);
    };
    let want = car.to_ascii_uppercase();

    if sets.is_empty() {
        return show(&bytes, &want, &bundle);
    }

    let edits = parse_sets(sets)?;
    let rec_at = gizmo_nfs::globalb::find_record(&bytes, &want)
        .ok_or_else(|| format!("{want}: no record of that name in {}", bundle.lzc.display()))?;

    // What each edit is replacing, read before anything is written — so the report is a before and
    // after rather than an after.
    outln!("{want} in {}", bundle.lzc.display());
    for &(field, value) in &edits {
        let was = edit::get(&bytes[rec_at..], field);
        match was {
            Some(was) => outln!("  {:<16} {:>12.4}  →{:>12.4}", field.key(), was, value),
            None => outln!("  {:<16} {:>12}  →{:>12.4}", field.key(), "—", value),
        }
    }

    let changed = edit::apply(&mut bytes, &want, &edits).map_err(|e| format!("{e}"))?;
    if changed == 0 {
        outln!("\nnothing to write — every value is already what was asked for");
        return Ok(());
    }

    match target {
        Target::DryRun => {
            outln!(
                "\n{changed} lane(s) would change. Add --install to write into the game, or -o FILE."
            );
        }
        Target::File(out) => {
            std::fs::write(&out, &bytes).map_err(|e| format!("{}: {e}", out.display()))?;
            outln!("\n{changed} lane(s) → {} (decompressed)", out.display());
        }
        Target::Install => {
            let codec = if keep_codec { Codec::Jdlz } else { Codec::None };
            let done = install::write(&bundle, &bytes, codec).map_err(|e| format!("{e}"))?;
            for b in &done.backups {
                outln!("\nbacked up  {}", b.display());
            }
            for f in &done.files {
                outln!("wrote      {}", f.display());
            }
            outln!("\n{changed} lane(s) written. Restart the game — the record is read when a car loads.");
        }
    }
    Ok(())
}

/// `field=value`, as many times as the caller wants.
fn parse_sets(sets: &[String]) -> Result<Vec<edit::Edit>> {
    let mut out = Vec::with_capacity(sets.len());
    for s in sets {
        let (name, value) = s
            .split_once('=')
            .ok_or_else(|| format!("{s:?}: a --set looks like `idle_rpm=900`"))?;
        let field = CarField::parse(name).ok_or_else(|| {
            format!("{name:?}: not a field. Run without --set to see what a car's are called.")
        })?;
        let value: f32 = value
            .trim()
            .parse()
            .map_err(|_| format!("{value:?}: not a number, in {s:?}"))?;
        out.push((field, value));
    }
    Ok(out)
}

/// Print the car: what every field is called and what it holds, plus what the four upgrade levels
/// come to.
///
/// The power line is repeated per level because that is the question this command exists to answer —
/// the game's own dynamometer prints the same figure, so anyone with the game can hold this up
/// against it.
fn show(bytes: &[u8], car: &str, bundle: &install::Bundle) -> Result<()> {
    let info = gizmo_nfs::globalb::find_car(bytes, car)
        .ok_or_else(|| format!("{car}: no record of that name in {}", bundle.lzc.display()))?;
    let rec_at = gizmo_nfs::globalb::find_record(bytes, car).ok_or("located, then not")?;
    let h = &info.handling;

    outln!("{} — {}\n", info.name, bundle.lzc.display());
    for level in 0..4 {
        let p = h.peak_power_at(level);
        let name = ["stock", "L1", "L2", "L3"][level];
        let g = &h.gearbox[level];
        outln!(
            "  {name:<5}  {:>6.1} kW  {:>5.0} hp @ {:>5.0} rpm   {} gears, final {:.3}",
            p.kw(),
            p.hp(),
            p.rpm,
            g.count,
            g.final_drive
        );
    }
    // A `?` in the unit column marks a lane whose *meaning* is a candidate rather than a claim —
    // see `gizmo_nfs::Unproven`. The field's own name carries it too: those end in `?`.
    outln!("\n  {:<16} {:>12}  {}", "field", "value", "unit");
    let record = &bytes[rec_at..];
    for field in CarField::all() {
        let Some(value) = edit::get(record, field) else { continue };
        outln!("  {:<16} {:>12.4}  {}", field.key(), value, unit(field));
    }
    Ok(())
}

/// Every car in the bundle, in one table.
///
/// Two shapes, and the reason for both is that a lane is *named* by how it varies. One car's `0.535`
/// says nothing; the same lane read across 46 records — ordered by drivetrain, with the traffic
/// vehicles falling outside the playable range — is what turned `+0x38C` into a brake-bias candidate
/// and what disqualified a rival reading of `+0x380`. So the wide form (`--csv`) exists to be sorted
/// and correlated in something else, and the tall form to be read here.
///
/// The candidate lanes are in both, with their `?` names intact. They are the ones this table is
/// most useful for.
fn show_all(bytes: &[u8], bundle: &install::Bundle, csv: bool) -> Result<()> {
    let cars = gizmo_nfs::globalb::located_cartypeinfos(bytes);
    if cars.is_empty() {
        return Err(format!("{}: no car records", bundle.lzc.display()));
    }
    let fields: Vec<CarField> = CarField::all();

    if csv {
        let mut head = vec!["car".to_string(), "drive".into(), "kw_stock".into(), "kw_l3".into()];
        head.extend(fields.iter().map(|f| f.key()));
        outln!("{}", head.join(","));
        for (at, info) in &cars {
            let record = &bytes[*at..];
            let mut row = vec![
                info.name.clone(),
                drive_word(info.handling.rear_drive).to_string(),
                format!("{:.2}", info.handling.peak_power().kw()),
                format!("{:.2}", info.handling.peak_power_at(3).kw()),
            ];
            row.extend(fields.iter().map(|f| match edit::get(record, *f) {
                Some(v) => format!("{v:.4}"),
                None => String::new(),
            }));
            outln!("{}", row.join(","));
        }
        return Ok(());
    }

    outln!("{} records in {}\n", cars.len(), bundle.lzc.display());
    outln!(
        "{:<12} {:>4} {:>7} {:>7} {:>7} {:>6} {:>6} {:>6} {:>7} {:>7} {:>7} {:>7}",
        "car", "drv", "kW", "kW L3", "mass", "idle", "red", "limit", "steer?", "a284?", "cg?", "bias?"
    );
    for (_, info) in &cars {
        let h = &info.handling;
        let u = &h.unproven;
        outln!(
            "{:<12} {:>4} {:>7.1} {:>7.1} {:>7.0} {:>6.0} {:>6.0} {:>6.0} {:>7.2} {:>7.1} {:>7.3} {:>7.3}",
            info.name,
            drive_word(h.rear_drive),
            h.peak_power().kw(),
            h.peak_power_at(3).kw(),
            info.mass_kg,
            h.engine.idle_rpm,
            h.engine.red_line_rpm,
            h.engine.limiter_rpm,
            h.steer_ratio,
            u.angle_284_deg,
            u.cg_height,
            u.brake_bias_f
        );
    }
    outln!("\n  ? = a candidate lane, not a claim — see gizmo_nfs::Unproven.");
    outln!("  --csv gives every field of every car, for sorting somewhere else.");
    Ok(())
}

/// Front, all or rear, from the rear-drive fraction.
fn drive_word(rear_drive: f32) -> &'static str {
    match rear_drive {
        r if r <= 0.01 => "FWD",
        r if r >= 0.99 => "RWD",
        _ => "AWD",
    }
}

/// What a field is measured in, for the last column.
fn unit(field: CarField) -> &'static str {
    match field {
        CarField::MassKg => "kg",
        CarField::IdleRpm | CarField::RedLineRpm | CarField::LimiterRpm => "rpm",
        CarField::TorqueNm(_) | CarField::TorqueGainNm { .. } => "Nm",
        CarField::TyreWidthMm(_) => "mm",
        CarField::WheelRadiusM(_) => "m",
        CarField::GearCount(_) => "",
        CarField::Angle284 => "°  ?",
        CarField::CgHeight => "m  ?",
        CarField::BrakeBiasFront | CarField::Spring(_) | CarField::Damper(_) => "   ?",
        _ => ":1",
    }
}
