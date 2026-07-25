//! `ug2 profile` — what a player profile says a car has fitted.

use crate::paths::Result;
use gizmo_nfs::profile::{Category, Profile, INSTALLED_AT, TUNING_AT};
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
    outln!("\ntotals read at {TUNING_AT:#07x}, flags at {INSTALLED_AT:#07x} — both measured on one");
    outln!("profile, so this refuses anything whose shape does not match rather than guessing.");
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
