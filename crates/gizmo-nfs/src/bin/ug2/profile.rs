//! `ug2 profile` — what a player profile says a car has fitted.

use crate::paths::Result;
use gizmo_nfs::profile::{Category, Profile, INSTALLED_AT, TUNING_AT, VINYL_AT};
use std::path::{Path, PathBuf};

pub fn run(path: &Path) -> Result<()> {
    let file = locate(path)?;
    let bytes = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let p = Profile::parse(&bytes).map_err(|e| format!("{}: {e}", file.display()))?;

    outln!("{}\n", file.display());
    outln!("{:<14} {:>8}   fitted", "category", "total");
    for (c, label) in [
        (Category::Transmission, "transmission"),
        (Category::Nitrous, "nitrous"),
        (Category::Engine, "engine"),
    ] {
        outln!("{label:<14} {:>8.2}", p.total(c));
    }
    // The flags by offset, because which byte is lit is the part of this that was measured — a
    // product's *name* is not in the profile, only its slot.
    outln!("\n{} products fitted:", p.fitted());
    for (i, on) in p.installed.iter().enumerate() {
        if *on {
            outln!("  +{:#07x}", INSTALLED_AT + i);
        }
    }
    // The vinyl is a hash, so it is printed as one; a name only appears when a pack can produce it.
    outln!("\nvinyl:");
    match p.vinyl {
        None => outln!("  none applied"),
        Some(h) => {
            let named = vinyl_names(&file).and_then(|(car, names)| {
                p.vinyl_name(&car, names.iter().map(String::as_str))
            });
            match named {
                Some(n) => outln!("  {:#010x}  {n}", h.0),
                None => outln!("  {:#010x}  (no name in reach hashes to it)", h.0),
            }
        }
    }

    outln!("\ntotals read at {TUNING_AT:#07x}, flags at {INSTALLED_AT:#07x}, vinyl at {VINYL_AT:#07x}");
    outln!("— all measured on one profile, so this refuses anything whose shape does not match");
    outln!("rather than guessing.");
    Ok(())
}

/// The profile file, or a directory holding one.
fn locate(path: &Path) -> Result<PathBuf> {
    if path.is_file() {
        return Ok(path.to_path_buf());
    }
    // `<dir>/<name>/<name>` is how the game lays it out.
    if let Some(name) = path.file_name() {
        let inner = path.join(name);
        if inner.is_file() {
            return Ok(inner);
        }
    }
    Err(format!("no profile at {}", path.display()))
}

/// The vinyl names a nearby install can offer, for turning the profile's hash back into a name.
///
/// The profile does not say which car it is about, so there is nothing to look the pack up by:
/// `NFSU2_ROOT` is consulted and every car's `VINYLS.BIN` contributes its names. A hash that two
/// cars share resolves the same either way, since the stored name has the car prefix stripped.
fn vinyl_names(_profile: &Path) -> Option<(String, Vec<String>)> {
    let root = PathBuf::from(std::env::var_os("NFSU2_ROOT")?);
    let mut names = Vec::new();
    let cars = std::fs::read_dir(root.join("CARS")).ok()?;
    for car in cars.flatten() {
        let pack = car.path().join("VINYLS.BIN");
        let Ok(bytes) = std::fs::read(&pack) else { continue };
        let Ok(entries) = gizmo_nfs::Tpk::directory(&bytes) else { continue };
        let prefix = car.file_name().to_string_lossy().to_string();
        for e in &entries {
            if let Ok(n) = gizmo_nfs::Tpk::name_of(&bytes, e) {
                names.push(n.strip_prefix(&format!("{prefix}_")).unwrap_or(&n).to_string());
            }
        }
    }
    (!names.is_empty()).then(|| (String::new(), names))
}
