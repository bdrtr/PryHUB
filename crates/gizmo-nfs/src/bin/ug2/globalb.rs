//! `ug2 globalb` — the per-car records in the game's global bundle.

use crate::paths::{self, Result};
use gizmo_nfs::globalb::parse_cartypeinfos;
use std::path::{Path, PathBuf};

pub fn run(path: &Path, filter: Option<&str>, parts: bool, handling: bool) -> Result<()> {
    let file = locate(path)?;
    let bytes = paths::read(&file)?;
    if parts {
        return car_parts(&file, &bytes, filter);
    }
    let cars = parse_cartypeinfos(&bytes);
    outln!("{} CarTypeInfo records in {}\n", cars.len(), file.display());
    if handling {
        return show_handling(&cars, filter);
    }
    outln!("{:<14} {:>9} {:>7} {:>7} {:>8}   front-left mount", "car", "wheelbase", "track", "radius", "mass");
    for c in cars.iter().filter(|c| filter.is_none_or(|f| c.name.contains(f))) {
        let (fl, rr) = (c.wheels[0], c.wheels[2]);
        outln!(
            "{:<14} {:>8.2}m {:>6.2}m {:>6.3}m {:>6.0}kg   fa={:+.2} lat={:+.2} rh={:+.2}",
            c.name,
            (fl.fore_aft - rr.fore_aft).abs(),
            fl.lateral.abs() * 2.0,
            fl.radius,
            c.mass_kg,
            fl.fore_aft,
            fl.lateral,
            fl.ride_height
        );
    }
    Ok(())
}

/// Accept the bundle itself, a car directory, or the game root.
fn locate(path: &Path) -> Result<PathBuf> {
    if path.is_file() {
        return Ok(path.to_path_buf());
    }
    if let Some(p) = paths::globalb_beside(path) {
        return Ok(p); // a car directory
    }
    let direct = path.join("GLOBAL").join("GLOBALB.BUN");
    if direct.is_file() {
        return Ok(direct); // the game root
    }
    Err(format!("no GLOBALB.BUN at or under {}", path.display()))
}

/// `--parts`: the `CarParts` tables. Counts first, because they are what the header claims and what
/// the reader checked; then a sample of names, the attribute keys by frequency, and the palette.
fn car_parts(file: &Path, bytes: &[u8], filter: Option<&str>) -> Result<()> {
    use gizmo_nfs::globalb::carparts::{self, CarParts};
    use std::collections::BTreeMap;

    let cp = CarParts::parse(bytes).map_err(|e| format!("{}: {e}", file.display()))?;
    outln!("{} parts · {} attributes in {}\n", cp.parts.len(), cp.attributes.len(), file.display());

    let shown: Vec<_> =
        cp.parts.iter().filter(|p| filter.is_none_or(|f| p.name.contains(f))).collect();
    outln!("parts{}:", filter.map_or(String::new(), |f| format!(" matching {f:?}")));
    for p in shown.iter().take(20) {
        match p.block {
            Some(b) => outln!("  {:<28} block {b}", p.name),
            None => outln!("  {:<28} —", p.name),
        }
    }
    if shown.len() > 20 {
        // A cap that truncated in silence would read as "that is all of them".
        outln!("  … {} more", shown.len() - 20);
    }

    let mut by_key: BTreeMap<u32, usize> = BTreeMap::new();
    for a in &cp.attributes {
        *by_key.entry(a.key.0).or_default() += 1;
    }
    // Two of the 51 are still unnamed, and they stay a "?" rather than a guess.
    let named = |k: u32| carparts::name_of(k).unwrap_or("?");
    let mut keys: Vec<_> = by_key.into_iter().collect();
    keys.sort_by_key(|&(_, n)| std::cmp::Reverse(n));
    outln!("\nattribute keys ({} distinct):", keys.len());
    for (k, n) in keys.iter().take(12) {
        outln!("  {:#010x}  ×{:<5} {}", k, n, named(*k));
    }

    let palette = cp.palette();
    outln!("\npaint palette ({} colours):", palette.len());
    for row in palette.chunks(6) {
        let cells: Vec<String> =
            row.iter().map(|c| format!("#{:02X}{:02X}{:02X}", c.red, c.green, c.blue)).collect();
        outln!("  {}", cells.join("  "));
    }
    Ok(())
}

/// What the record says about how each car drives.
///
/// The gearbox is printed once per upgrade level because it is the one thing the record stores four
/// times; everything else it stores once. The torque curve is nine magnitudes with **no rpm axis in
/// the file**, so it is printed as the points it is rather than against invented engine speeds.
fn show_handling(cars: &[gizmo_nfs::CarTypeInfo], filter: Option<&str>) -> Result<()> {
    for c in cars.iter().filter(|c| filter.is_none_or(|f| c.name.contains(f))) {
        let h = &c.handling;
        let drive = match h.rear_drive {
            r if r <= 0.01 => "FWD",
            r if r >= 0.99 => "RWD",
            _ => "AWD",
        };
        let [l, w, ht] = h.body_m;
        outln!(
            "== {} ==  {drive}  {:.0} kg  {l:.2}×{w:.2}×{ht:.2} m  tyres {:.0} mm",
            c.name,
            c.mass_kg,
            h.tyre_width_m[0] * 1000.0
        );
        outln!(
            "   rpm    idle {:.0}  red line {:.0}  limiter {:.0}",
            h.engine.idle_rpm,
            h.engine.red_line_rpm,
            h.engine.limiter_rpm
        );
        let peak = h.torque_nm.iter().copied().fold(f32::MIN, f32::max);
        let curve: Vec<String> = h.torque_nm.iter().map(|t| format!("{t:.0}")).collect();
        outln!("   torque {} Nm  (peak {peak:.0}, 9 points, no rpm axis in the file)", curve.join(" "));
        for (level, g) in h.gearbox.iter().enumerate() {
            let ratios: Vec<String> = g.gears().iter().map(|r| format!("{r:.3}")).collect();
            let name = ["stock", "L1", "L2", "L3"][level];
            outln!(
                "   {name:<5}  {} gears  final {:.3}  rev {:.3}  {}",
                g.count,
                g.final_drive,
                g.reverse,
                ratios.join(" ")
            );
        }
        outln!("");
    }
    Ok(())
}
