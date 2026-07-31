//! Where an edited bundle goes — the two files an install keeps, and the backup taken first.
//!
//! This is a **file-boundary** module, like [`crate::decompress_file`] and unlike everything else
//! here: the rest of the crate takes `&[u8]` and is fuzzable, and this reads and writes paths. It
//! exists rather than living in each caller because the semantics below are the sort that diverge
//! silently — a command line that installs into one file and an interface that installs into another
//! produce a bug report reading "it works in the CLI".
//!
//! # Which file the game reads
//!
//! `GLOBAL/GlobalB.lzc`. Not `GLOBAL/GLOBALB.BUN`, which sits beside it holding the same 46 records
//! and which the game does not open. That was settled by experiment: tripling a 240SX's mass in the
//! `.BUN` was installed and driven and felt like nothing at all, and the same edit to the same lane
//! of the `.lzc` produced a car that could barely get off the line.
//!
//! Both are written anyway. The `.BUN` is what every reading tool here reaches for first — including
//! this crate's own [`crate::globalb::parse_cartypeinfos`] callers — so leaving it stale means the
//! inspector shows one car and the game drives another.
//!
//! # Compressed, and what to write back
//!
//! A pristine `GlobalB.lzc` is **JDLZ**, 5,145,778 bytes, and decompresses byte-for-byte to the
//! 8,008,064-byte `.BUN`. (A note elsewhere in this repository once said the `.lzc` was not
//! compressed. It was describing a file that had already been replaced with a decompressed one —
//! which is the same mistake as reading your own output for the original.)
//!
//! So there are two ways to put it back, and the default is the one with evidence behind it:
//!
//! * [`Codec::None`] — write it **plain**. The `.lzc` is picked up by its magic bytes rather than
//!   its extension, and a plain one has been installed and driven. It costs 3 MB of disk.
//!
//!   That last sentence was inherited from an older note and is now **this crate's own measurement**.
//!   It matters more than it sounds: every null result from an edit rests on the edit having
//!   arrived, and the tool had never proved its own default arrived. So a 240SX's mass was tripled
//!   to 3,660 kg, written plain by this function, and driven — the car was unmistakably heavy. The
//!   channel carries. Anything that does nothing in the game after this is a lane that does nothing,
//!   not a file the game never opened.
//! * [`Codec::Jdlz`] — recompress. Keeps the shape the game shipped, and rests on this crate's own
//!   encoder rather than on EA's. [`write`] round-trips whatever it produces through the decoder
//!   before it will write it, so the failure mode is a refusal and not a game that will not start.

use crate::compression::{self, Codec};
use crate::error::{NfsError, NfsResult};
use std::path::{Path, PathBuf};

/// The pair of files an install keeps its car database in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundle {
    /// `GLOBAL/GlobalB.lzc` — the one the game reads.
    pub lzc: PathBuf,
    /// `GLOBAL/GLOBALB.BUN` — the same records, and what the tools read. `None` if this install
    /// does not have one.
    pub bun: Option<PathBuf>,
}

/// What a write did.
#[derive(Debug, Clone, Default)]
pub struct Written {
    /// The files written, in the order they were written.
    pub files: Vec<PathBuf>,
    /// Backups taken on the way, if any were still to take.
    pub backups: Vec<PathBuf>,
}

/// Find the bundle from anything that points into an install: the root, its `GLOBAL` folder, a car
/// directory, or either of the two files themselves.
///
/// The `.lzc` is what makes a result: an install without one is an install this cannot write to in
/// the way that matters, and saying so here is better than writing a `.BUN` nothing reads.
#[must_use]
pub fn find(from: &Path) -> Option<Bundle> {
    let global = global_dir(from)?;
    let lzc = first_named(&global, "globalb.lzc")?;
    Some(Bundle { bun: first_named(&global, "globalb.bun"), lzc })
}

/// The `GLOBAL` directory an install keeps, from any of the places someone might point at.
fn global_dir(from: &Path) -> Option<PathBuf> {
    let dir = if from.is_dir() { from.to_path_buf() } else { from.parent()?.to_path_buf() };
    // `<root>/GLOBAL`, `<root>`, `<root>/CARS/<car>` — two levels up covers the last one.
    let tries = [
        dir.join("GLOBAL"),
        dir.clone(),
        dir.parent().map(|p| p.join("GLOBAL")).unwrap_or_default(),
        dir.parent().and_then(Path::parent).map(|p| p.join("GLOBAL")).unwrap_or_default(),
    ];
    tries.into_iter().find(|p| first_named(p, "globalb.lzc").is_some())
}

/// A file in `dir` whose name matches `want` case-insensitively.
///
/// Case matters here in a way it does not on the machine the game was built for: an install laid
/// down under Wine keeps `GlobalB.lzc` beside `GLOBALB.BUN`, mixed case and all, and a Linux
/// filesystem will not find one by asking for the other.
fn first_named(dir: &Path, want: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| {
        p.file_name().is_some_and(|n| n.to_string_lossy().to_ascii_lowercase() == want)
    })
}

/// Read the bundle, decompressed. The `.lzc` is the source of truth, because it is what the game
/// reads — a `.BUN` that has drifted from it is showing the wrong car.
pub fn read(bundle: &Bundle) -> NfsResult<Vec<u8>> {
    let raw = std::fs::read(&bundle.lzc)?;
    compression::decompress(&raw)
}

/// Write the edited bytes into both files, taking a backup of each first.
///
/// `as_codec` decides what the `.lzc` is written as — see the module note for why [`Codec::None`] is
/// the sound default. Anything compressed is **decompressed again and compared** before it reaches
/// the disk: an encoder that has produced a stream its own decoder cannot read is a refusal here,
/// rather than a game that will not start and no idea which of the two files did it.
pub fn write(bundle: &Bundle, bytes: &[u8], as_codec: Codec) -> NfsResult<Written> {
    let packed = match as_codec {
        Codec::None => bytes.to_vec(),
        Codec::Jdlz => {
            let packed = compression::jdlz::compress(bytes)?;
            if compression::decompress(&packed)? != bytes {
                return Err(NfsError::Decompression {
                    codec: "jdlz",
                    detail: "the re-encoded bundle did not come back as itself",
                });
            }
            packed
        }
        _ => {
            return Err(NfsError::NotImplemented { feature: "installing a bundle under this codec" })
        }
    };

    let mut done = Written::default();
    // The `.lzc` goes down first: it is the file that matters, so if the disk fills half way through
    // the pair, the half that got written is the half the game reads.
    for (path, payload) in [(&bundle.lzc, packed.as_slice())]
        .into_iter()
        .chain(bundle.bun.iter().map(|p| (p, bytes)))
    {
        if let Some(backup) = back_up(path)? {
            done.backups.push(backup);
        }
        std::fs::write(path, payload)?;
        done.files.push(path.clone());
    }
    Ok(done)
}

/// `<file>.bak`, written once and never overwritten.
///
/// Once, because a second edit would otherwise replace the backup of the original with a backup of
/// the first edit, which is the moment a backup stops being one.
///
/// Copied to a temporary name and **renamed into place**, which is what makes skipping-on-existence
/// sound: a plain copy that died half way — a full disk, a pull on the cable — leaves a short file
/// that looks exactly like a good one, and the next run would restore from it. A rename within a
/// directory is atomic, so a `.bak` that is there was finished.
fn back_up(file: &Path) -> NfsResult<Option<PathBuf>> {
    let with = |suffix: &str| {
        let mut p = file.as_os_str().to_owned();
        p.push(suffix);
        PathBuf::from(p)
    };
    let bak = with(".bak");
    if bak.exists() {
        return Ok(None);
    }
    let partial = with(".bak.part");
    let copied = std::fs::copy(file, &partial)?;
    let want = std::fs::metadata(file).map(|m| m.len()).unwrap_or(copied);
    if copied != want {
        std::fs::remove_file(&partial).ok();
        return Err(NfsError::BufferSizeMismatch { detail: "the backup came out short" });
    }
    std::fs::rename(&partial, &bak)?;
    Ok(Some(bak))
}

/// Put the backups back, undoing every install this module has made.
///
/// The one operation a person reaches for at speed, so it is a function rather than a paragraph of
/// documentation telling them which file to copy where.
pub fn restore(bundle: &Bundle) -> NfsResult<Vec<PathBuf>> {
    let mut back = Vec::new();
    for file in std::iter::once(&bundle.lzc).chain(bundle.bun.iter()) {
        let mut p = file.as_os_str().to_owned();
        p.push(".bak");
        let bak = PathBuf::from(p);
        if bak.is_file() {
            std::fs::copy(&bak, file)?;
            back.push(file.clone());
        }
    }
    Ok(back)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory shaped like an install's `GLOBAL`, with the mixed case a real one has.
    fn install(tag: &str, payload: &[u8]) -> PathBuf {
        let root = std::env::temp_dir().join(format!("gizmo-nfs-install-{}-{tag}", std::process::id()));
        let global = root.join("GLOBAL");
        std::fs::create_dir_all(&global).expect("temp dir");
        std::fs::write(global.join("GlobalB.lzc"), payload).expect("lzc");
        std::fs::write(global.join("GLOBALB.BUN"), payload).expect("bun");
        root
    }

    #[test]
    fn a_bundle_is_found_from_anywhere_in_an_install() {
        let root = install("find", b"plain bytes");
        let want = find(&root).expect("from the root");
        assert!(want.lzc.ends_with("GlobalB.lzc"));
        assert!(want.bun.is_some(), "the .BUN beside it is picked up too");

        assert_eq!(find(&root.join("GLOBAL")).as_ref(), Some(&want), "from GLOBAL itself");
        assert_eq!(find(&want.lzc).as_ref(), Some(&want), "from the file");
        let car = root.join("CARS").join("240SX");
        std::fs::create_dir_all(&car).expect("car dir");
        assert_eq!(find(&car).as_ref(), Some(&want), "from a car directory");

        // Somewhere that is not an install is not one.
        assert_eq!(find(&std::env::temp_dir().join("nothing-here-at-all")), None);
        std::fs::remove_dir_all(&root).ok();
    }

    /// The round trip that matters: write, read back, and find the edit in the file the game opens.
    #[test]
    fn an_install_writes_both_files_and_backs_them_up_once() {
        let root = install("write", b"the original bytes");
        let bundle = find(&root).expect("bundle");

        let done = write(&bundle, b"the edited bytes", Codec::None).expect("write");
        assert_eq!(done.files.len(), 2, "both files");
        assert_eq!(done.backups.len(), 2, "and a backup of each");
        assert_eq!(read(&bundle).expect("read back"), b"the edited bytes");

        // The backups hold the *original*, and a second write does not replace them with the first
        // edit — which is the whole point of taking one only once.
        let bak = done.backups[0].clone();
        assert_eq!(std::fs::read(&bak).expect("bak"), b"the original bytes");
        let again = write(&bundle, b"edited twice", Codec::None).expect("second write");
        assert!(again.backups.is_empty(), "the second write takes no backup");
        assert_eq!(std::fs::read(&bak).expect("bak"), b"the original bytes");

        // And restoring puts them back.
        let back = restore(&bundle).expect("restore");
        assert_eq!(back.len(), 2);
        assert_eq!(read(&bundle).expect("read"), b"the original bytes");
        std::fs::remove_dir_all(&root).ok();
    }

    /// Written as JDLZ, the file comes back as itself — which is the check `write` makes before it
    /// will hand a compressed stream to somebody's game.
    #[test]
    fn a_compressed_install_round_trips() {
        // Repetitive enough that the encoder actually has matches to find. A counter is not: an
        // earlier version of this test used one, and JDLZ made it 13 % *larger*, which is the right
        // answer to the wrong input rather than a bug in the encoder.
        let payload: Vec<u8> = (0..512u32)
            .flat_map(|i| {
                let mut row = b"CARS\\TESTX\\GEOMETRY.BIN\0\0\0\0\0\0\0\0\0".to_vec();
                row.extend_from_slice(&(i % 7).to_le_bytes());
                row
            })
            .collect();
        let root = install("jdlz", &payload);
        let bundle = find(&root).expect("bundle");

        write(&bundle, &payload, Codec::Jdlz).expect("write");
        let raw = std::fs::read(&bundle.lzc).expect("read the lzc");
        assert_eq!(compression::detect(&raw), Codec::Jdlz, "it went down compressed");
        assert!(raw.len() < payload.len(), "and smaller: {} of {}", raw.len(), payload.len());
        assert_eq!(read(&bundle).expect("decompress"), payload);

        // The `.BUN` twin is written plain whatever the `.lzc` was — it is not a `.lzc`.
        let bun = std::fs::read(bundle.bun.as_ref().expect("bun")).expect("read");
        assert_eq!(bun, payload);
        std::fs::remove_dir_all(&root).ok();
    }
}
