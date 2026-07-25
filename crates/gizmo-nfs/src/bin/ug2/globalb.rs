//! `ug2 globalb` — the per-car records in the game's global bundle.

use crate::paths::{self, Result};
use gizmo_nfs::globalb::parse_cartypeinfos;
use std::path::{Path, PathBuf};

pub fn run(path: &Path, filter: Option<&str>, parts: bool) -> Result<()> {
    let file = locate(path)?;
    let bytes = paths::read(&file)?;
    if parts {
        return car_parts(&file, &bytes, filter);
    }
    let cars = parse_cartypeinfos(&bytes);
    outln!("{} CarTypeInfo records in {}\n", cars.len(), file.display());
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
    use gizmo_nfs::globalb::carparts::{key, CarParts};
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
    let named = |k: u32| match k {
        key::TEXTURE => "TEXTURE",
        key::NAME => "NAME",
        key::RED => "RED",
        key::GREEN => "GREEN",
        key::BLUE => "BLUE",
        key::CARBONFIBRE => "CARBONFIBRE",
        // The other 45 keys are hashes whose word is not in the file; saying so beats inventing one.
        _ => "?",
    };
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
