//! Writing the CARP screen's edits into the game's own car database.
//!
//! The second thing this program writes that is not its own output, and it is a heavier decision
//! than the first. A texture goes back into one car's pack; this goes into `GLOBAL/GlobalB.lzc`,
//! which holds **every** car — so the file being rewritten is one the game will not start without.
//!
//! Three rules follow, and two of them are the texture path's, kept deliberately:
//!
//! * **A backup, once.** Taken before the file it is a backup of is touched, and never replaced, so
//!   what is beside the bundle is always the install's own — not a backup of yesterday's edit.
//! * **Refused whole.** [`gizmo_nfs::globalb::edit::apply`] resolves every lane before writing any,
//!   so a set with one bad field leaves the bundle exactly as it was.
//! * **Both files, always.** `GLOBALB.BUN` is the one every reading tool here reaches for and
//!   `GlobalB.lzc` is the one the game opens. Writing only the second would leave this program's own
//!   inspector showing a car that is no longer the car being driven.
//!
//! Unlike the texture path there is **no copy-by-default mode**. A `TEXTURES.BIN` written into
//! `pryhub-edit/` is still a useful thing — you can look at it, diff it, install it later — and an
//! edited bundle sitting in a folder is not, because there is exactly one place it does anything.
//! The consent is asked for in the interface instead, next to the path it is about.

use std::path::PathBuf;

/// What to write, decided on the UI thread and carried to the worker.
pub struct Spec {
    /// The file the user has open. The install is found from it, the same walk the record was read
    /// by — so the bundle written is the bundle the values on screen came out of.
    pub beside: PathBuf,
    /// Which car's record, by the name it carries.
    pub car: String,
    /// Every pending edit, applied together. One read of an 8 MB bundle, one write.
    pub edits: Vec<(gizmo_nfs::CarField, f32)>,
}

/// What happened, for the log and for the strip under the Save button.
pub struct Done {
    pub car: String,
    /// How many lanes actually differed. Zero is a real answer: it means the numbers on screen were
    /// already what the file held.
    pub changed: usize,
    pub files: Vec<PathBuf>,
    /// Backups taken on the way. Empty on a second save, which is correct — see the module note.
    pub backups: Vec<PathBuf>,
}

/// Do it. Runs on the worker thread.
pub fn run(spec: &Spec) -> Result<Done, String> {
    if spec.edits.is_empty() {
        return Err("nothing to save".into());
    }
    let bundle = gizmo_nfs::globalb::install::find(&spec.beside).ok_or_else(|| {
        format!("{}: no GLOBAL/GlobalB.lzc above this file", spec.beside.display())
    })?;
    let mut bytes = gizmo_nfs::globalb::install::read(&bundle)
        .map_err(|e| format!("{}: {e}", bundle.lzc.display()))?;

    let changed = gizmo_nfs::globalb::edit::apply(&mut bytes, &spec.car, &spec.edits)
        .map_err(|e| format!("{}: {e}", spec.car))?;
    if changed == 0 {
        return Ok(Done { car: spec.car.clone(), changed: 0, files: Vec::new(), backups: Vec::new() });
    }

    // Read the whole car back out of the buffer that is about to be written, through the parser,
    // before any of it reaches the disk. `apply` already checks the record still parses; this checks
    // the *bundle* still yields the car by the same scan the reader uses to find it, which is the
    // thing a caller would notice too late.
    gizmo_nfs::globalb::find_car(&bytes, &spec.car)
        .ok_or_else(|| format!("{}: the edited bundle no longer holds this car", spec.car))?;

    // Plain rather than JDLZ: the game picks the codec by magic bytes, and a plain `.lzc` is what
    // has actually been installed and driven. See `gizmo_nfs::globalb::install`.
    let written =
        gizmo_nfs::globalb::install::write(&bundle, &bytes, gizmo_nfs::compression::Codec::None)
            .map_err(|e| format!("{}: {e}", bundle.lzc.display()))?;

    Ok(Done {
        car: spec.car.clone(),
        changed,
        files: written.files,
        backups: written.backups,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole worker path over a real install, on a **copy**: edit a car, write it, and read the
    /// file back to see the number took.
    ///
    /// Skipped unless `NFSU2_ROOT` is set, like every other test here that needs the game. It copies
    /// the two `GLOBAL` files into a temporary install rather than touching the real one — a test
    /// that rewrote somebody's car database would be a bug report waiting to happen.
    #[test]
    fn a_real_bundle_takes_a_real_edit() {
        let Some(root) = std::env::var_os("NFSU2_ROOT").map(PathBuf::from) else {
            eprintln!("NFSU2_ROOT unset — skipping");
            return;
        };
        let Some(real) = gizmo_nfs::globalb::install::find(&root) else { return };

        let dir = std::env::temp_dir().join(format!("pryhub-tune-{}", std::process::id()));
        let global = dir.join("GLOBAL");
        std::fs::create_dir_all(&global).expect("temp install");
        std::fs::copy(&real.lzc, global.join("GlobalB.lzc")).expect("copy the lzc");
        if let Some(bun) = &real.bun {
            std::fs::copy(bun, global.join("GLOBALB.BUN")).expect("copy the bun");
        }
        // A car directory to point at, the way the interface does — the install is found from the
        // open file rather than configured.
        let car_dir = dir.join("CARS").join("240SX");
        std::fs::create_dir_all(&car_dir).expect("car dir");
        let beside = car_dir.join("GEOMETRY.BIN");
        std::fs::write(&beside, b"not a real geometry file").expect("a file to be beside");

        let done = run(&Spec {
            beside: beside.clone(),
            car: "240SX".into(),
            edits: vec![
                (gizmo_nfs::CarField::IdleRpm, 950.0),
                (gizmo_nfs::CarField::TorqueGainNm { block: 3, point: 5 }, 120.0),
            ],
        })
        .expect("the save runs");
        assert_eq!(done.changed, 2);
        assert_eq!(done.files.len(), 2, "both files");
        assert_eq!(done.backups.len(), 2, "and a backup of each, first");

        let bundle = gizmo_nfs::globalb::install::find(&dir).expect("the temporary install");
        let bytes = gizmo_nfs::globalb::install::read(&bundle).expect("read it back");
        let car = gizmo_nfs::globalb::find_car(&bytes, "240SX").expect("still there");
        assert_eq!(car.handling.engine.idle_rpm, 950.0);
        assert!((car.handling.torque_gain_nm[3][5] - 120.0).abs() < 0.5);
        // The rest of the record is untouched — a write that moved something would show here.
        assert_eq!(car.handling.engine.limiter_rpm, 7000.0);
        // Approximately, and the reason is the file rather than the write: torque is stored in
        // kN·m, so 216 makes the round trip as 0.216 × 1000 and comes back 216.00002. An exact
        // comparison here fails on a lane nothing touched.
        assert!((car.handling.torque_nm[5] - 216.0).abs() < 0.001, "{}", car.handling.torque_nm[5]);

        // A second save takes no second backup, so the `.bak` still holds the install's own bytes.
        let again = run(&Spec {
            beside,
            car: "240SX".into(),
            edits: vec![(gizmo_nfs::CarField::IdleRpm, 1000.0)],
        })
        .expect("a second save");
        assert!(again.backups.is_empty());
        let mut bak = bundle.lzc.clone().into_os_string();
        bak.push(".bak");
        let original = gizmo_nfs::compression::decompress(
            &std::fs::read(PathBuf::from(bak)).expect("the backup"),
        )
        .expect("it decompresses");
        let was = gizmo_nfs::globalb::find_car(&original, "240SX").expect("in the backup");
        assert_eq!(was.handling.engine.idle_rpm, 800.0, "the backup is the install's own");

        std::fs::remove_dir_all(&dir).ok();
    }
}
