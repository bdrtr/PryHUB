//! Putting a texture back into the pack the interface is showing.
//!
//! The first thing this program writes that is not its own output. An export lands in a folder of
//! its own making and nothing reads it but the person who asked for it; this writes a **game file**,
//! and one the game may be asked to load. So the two decisions the shape of this module makes are
//! about that rather than about textures:
//!
//! * **A copy by default.** The pack is written to `pryhub-edit/`, beside the export folder and for
//!   the same reason. Someone trying the feature out should not discover what it does by finding
//!   out what it did to their install.
//! * **Overwriting is a choice with a backup.** It is genuinely what a modder wants — the game reads
//!   `CARS/240SX/TEXTURES.BIN` and nowhere else — so it is offered rather than withheld, but it is
//!   asked for explicitly and it writes a `.bak` first, once. Once, because a second edit would
//!   otherwise overwrite the backup of the original with a backup of the first edit, which is the
//!   moment a backup stops being one.
//!
//! The work itself is [`gizmo_nfs::texture::replace_images`] — the encodes, the in-place attempt and
//! the fall back to relocation — so this file is the part that reads and writes files, and none of
//! the part that decides what the bytes are.
//!
//! # A set, not a texture
//!
//! What arrives here is every staged replacement at once, and one rewrite of the pack takes all of
//! them. That is not a batching convenience. Written one at a time, each edit rewrote an 8.7 MB file
//! on its own, and — the part that was actually wrong — each one read the pack **from disk**, so a
//! second edit into a *copy* started from the original again and produced a copy holding the second
//! texture and not the first. Accumulating was left to the file, and in copy mode the file was never
//! the one being accumulated into.
//!
//! Holding the set in the interface instead fixes that at the root and is what the design asks for
//! anyway: its CARP screen edits into a pending set, marks what is dirty, counts it, and has a Save
//! and a Revert. The texture tab now borrows exactly that.
//!
//! A set is also refused **whole**. Every image is decoded and checked against the texture it is
//! going into before any of them is encoded, so a set with one wrong-sized PNG writes nothing rather
//! than most of itself.

use std::path::{Path, PathBuf};

/// What to replace, decided on the UI thread and carried to the worker.
///
/// It holds no borrows, like every other job's spec: the interface goes on drawing while this runs.
/// The pack's path is resolved when the button is pressed rather than looked up on the worker,
/// because the file the user was looking at is the file they meant.
pub struct Spec {
    /// Which document this was asked for — the identity the result is matched against when it
    /// lands. Not the same as [`Self::pack`]: a `GEOMETRY.BIN` is drawn with the `TEXTURES.BIN`
    /// beside it, so the file being written is routinely not the file that is open.
    pub doc: PathBuf,
    /// The pack to read and rewrite.
    pub pack: PathBuf,
    /// Every staged replacement, applied together.
    pub edits: Vec<Edit>,
    /// Write over `pack` instead of into `pryhub-edit/`.
    pub over: bool,
}

/// One staged replacement: a texture, and the image to put in it.
#[derive(Clone)]
pub struct Edit {
    pub hash: gizmo_nfs::AssetHash,
    /// What the texture is called, for the interface's own list. The worker reads the file's name.
    pub name: String,
    pub png: PathBuf,
}

/// What happened, for the log.
pub struct Done {
    /// How many textures went in.
    pub count: usize,
    /// One of them, so the interface can put the selection back after a reload.
    pub hash: gizmo_nfs::AssetHash,
    pub into: PathBuf,
    /// Whether the pack had to be laid out afresh to take them.
    pub moved: bool,
    /// The **worst** round trip in the set, or `None` when every one came back identical. The worst
    /// rather than the mean, because the number is there to answer "did this cost me anything", and
    /// an average over one exact and one poor replacement answers it for neither.
    pub psnr: Option<f32>,
    /// The backup written before overwriting, if one was.
    pub backup: Option<PathBuf>,
}

/// Do it. Runs on the worker thread.
///
/// Every staged edit goes into **one** rewrite of the pack. Applying them one at a time would
/// rewrite an 8.7 MB file once per texture, and once any of them relocated the next would start
/// from a pack whose every blob had already moved.
pub fn run(spec: &Spec) -> Result<Done, String> {
    if spec.edits.is_empty() {
        return Err("nothing staged".into());
    }
    let bytes = std::fs::read(&spec.pack).map_err(|e| format!("{}: {e}", spec.pack.display()))?;
    let entries = gizmo_nfs::Tpk::directory(&bytes)
        .map_err(|e| format!("{}: {e}", spec.pack.display()))?;

    // Decode each image and check it against the texture it is going into, before anything is
    // encoded — so a set with one wrong-sized image is refused whole rather than half-written.
    let mut pixels: Vec<(gizmo_nfs::AssetHash, Vec<u8>, u32, u32)> = Vec::new();
    for edit in &spec.edits {
        let entry = entries
            .iter()
            .find(|e| e.hash == edit.hash)
            .ok_or_else(|| format!("{:#010x}: not in {}", edit.hash.0, spec.pack.display()))?;
        let was = gizmo_nfs::Tpk::decode_one(&bytes, entry).map_err(|e| {
            format!("{} does not decode, so it cannot be written either: {e}", edit.name)
        })?;
        let image = std::fs::read(&edit.png).map_err(|e| format!("{}: {e}", edit.png.display()))?;
        let (rgba, w, h) = gizmo_nfs::export::png_pixels(&image)
            .map_err(|e| format!("{}: {e}", edit.png.display()))?;
        if (w, h) != (was.width, was.height) {
            // The one refusal a person will actually meet, so it says both sizes rather than
            // "invalid image".
            return Err(format!(
                "{} is {w}×{h}, {} is {}×{}",
                edit.png.file_name().unwrap_or_default().to_string_lossy(),
                was.name,
                was.width,
                was.height
            ));
        }
        pixels.push((edit.hash, rgba, w, h));
    }

    let images: Vec<gizmo_nfs::texture::Image> = pixels
        .iter()
        .map(|(hash, rgba, w, h)| gizmo_nfs::texture::Image {
            hash: *hash,
            rgba,
            width: *w,
            height: *h,
        })
        .collect();
    let (written, moved) =
        gizmo_nfs::texture::replace_images(&bytes, &images).map_err(|e| format!("{e}"))?;

    // Read back what is about to be written, before writing it. Nothing else in this program can
    // check its own output — an export is read by other tools and a screenshot by a person — but a
    // pack can be handed straight back to the parser, and a pack that will not decode is not one to
    // write over somebody's game. Every edited texture is checked, not just the first.
    let check = gizmo_nfs::Tpk::directory(&written)
        .map_err(|e| format!("the rewritten pack has no directory: {e}"))?;
    let mut worst: Option<f32> = None;
    for (hash, rgba, ..) in &pixels {
        let after = check
            .iter()
            .find(|x| x.hash == *hash)
            .ok_or("the rewritten pack lost a descriptor it was given")
            .and_then(|e| {
                gizmo_nfs::Tpk::decode_one(&written, e)
                    .map_err(|_| "the rewritten pack does not decode back")
            })?;
        if let Some(db) = quality(&after.rgba, rgba) {
            worst = Some(worst.map_or(db, |w: f32| w.min(db)));
        }
    }

    let out = target(&spec.pack, spec.over)?;
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    // The backup goes down before the file it is a backup of is touched, and only if there is not
    // one already — see the module note.
    let backup = if spec.over { back_up(&spec.pack)? } else { None };
    std::fs::write(&out, &written).map_err(|e| format!("{}: {e}", out.display()))?;

    Ok(Done {
        count: spec.edits.len(),
        hash: spec.edits[0].hash,
        into: out,
        moved,
        psnr: worst,
        backup,
    })
}

/// Where the rewritten pack goes.
///
/// Public and taking the two things it depends on rather than a whole [`Spec`], because the dialog
/// shows this path *before* the button is pressed. A target the user is told about afterwards is not
/// one they agreed to.
pub fn target(pack: &Path, over: bool) -> Result<PathBuf, String> {
    if over {
        return Ok(pack.to_path_buf());
    }
    let cwd = std::env::current_dir().map_err(|e| format!("working directory: {e}"))?;
    // The car's name and the pack's, the way the export folder is named — `240SX_TEXTURES.BIN`
    // rather than a `TEXTURES.BIN` that says nothing about which car's it is once it has been
    // copied out of its directory.
    let file = pack.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let car = pack
        .parent()
        .and_then(Path::file_name)
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let name = if car.is_empty() { file } else { format!("{car}_{file}") };
    Ok(cwd.join("pryhub-edit").join(name))
}

/// `<pack>.bak`, written once and never overwritten.
///
/// Copied to a temporary name and **renamed into place**, which is what makes the rule above it
/// sound. Skipping on existence is only safe if existence means *completeness*: a plain copy that
/// died half-way — a full disk, a pull on the cable — leaves a short file that looks exactly like a
/// good one, and the next run would then overwrite the pack with a truncated backup behind it. A
/// rename within a directory is atomic, so a `.bak` that is there was finished.
fn back_up(pack: &Path) -> Result<Option<PathBuf>, String> {
    let with = |suffix: &str| {
        let mut p = pack.as_os_str().to_owned();
        p.push(suffix);
        PathBuf::from(p)
    };
    let bak = with(".bak");
    if bak.exists() {
        // The existing one is older, which makes it the one worth keeping: it is the only copy of
        // the file as it was before this program touched it.
        return Ok(None);
    }
    let partial = with(".bak.part");
    let copied = std::fs::copy(pack, &partial).map_err(|e| format!("{}: {e}", partial.display()))?;
    let want = std::fs::metadata(pack).map(|m| m.len()).unwrap_or(copied);
    if copied != want {
        std::fs::remove_file(&partial).ok();
        return Err(format!("{}: backup came out {copied} bytes of {want}", bak.display()));
    }
    std::fs::rename(&partial, &bak).map_err(|e| format!("{}: {e}", bak.display()))?;
    Ok(Some(bak))
}

/// How close the written texture is to the image that was asked for.
///
/// `None` means identical, which is a real answer here and not a rounding: `0x20` is a channel swap
/// and an image that fits inside its tag's palette is reproduced entry for entry. The S3TC formats
/// cannot promise it — two endpoints per 4×4 block — so they get a number instead of a claim.
fn quality(after: &[u8], wanted: &[u8]) -> Option<f32> {
    if after == wanted {
        return None;
    }
    let mse: f64 = after
        .iter()
        .zip(wanted.iter())
        .map(|(a, b)| {
            let d = f64::from(i32::from(*a) - i32::from(*b));
            d * d
        })
        .sum::<f64>()
        / after.len().max(1) as f64;
    if mse == 0.0 {
        return None;
    }
    Some((10.0 * (255.0f64 * 255.0 / mse).log10()) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A second decodable texture in the pack, different from `first`.
    fn texs_two(
        bytes: &[u8],
        first: gizmo_nfs::AssetHash,
    ) -> Option<(gizmo_nfs::AssetHash, gizmo_nfs::NfsTexture)> {
        let entries = gizmo_nfs::Tpk::directory(bytes).ok()?;
        entries
            .iter()
            .filter(|e| e.hash != first)
            .find_map(|e| gizmo_nfs::Tpk::decode_one(bytes, e).ok().map(|t| (e.hash, t)))
    }

    fn spec(over: bool) -> Spec {
        Spec {
            doc: PathBuf::from("/game/CARS/240SX/GEOMETRY.BIN"),
            pack: PathBuf::from("/game/CARS/240SX/TEXTURES.BIN"),
            edits: vec![Edit {
                hash: gizmo_nfs::AssetHash(1),
                name: "T".into(),
                png: PathBuf::from("/tmp/x.png"),
            }],
            over,
        }
    }

    #[test]
    fn a_copy_is_named_after_its_car_and_its_pack() {
        let s = spec(false);
        let out = target(&s.pack, s.over).expect("cwd");
        assert!(out.ends_with("pryhub-edit/240SX_TEXTURES.BIN"), "{}", out.display());
    }

    #[test]
    fn overwriting_writes_where_it_read() {
        let s = spec(true);
        assert_eq!(target(&s.pack, s.over).expect("path"), s.pack);
    }

    #[test]
    fn identical_pixels_report_no_number() {
        assert_eq!(quality(&[1, 2, 3, 4], &[1, 2, 3, 4]), None);
    }

    #[test]
    fn a_difference_reports_a_finite_one() {
        let db = quality(&[10, 10, 10, 255], &[12, 10, 10, 255]).expect("a number");
        assert!(db > 20.0 && db < 60.0, "{db}");
    }

    /// The whole worker path over a real pack: decode a texture out, paint it, put it back, and read
    /// the written file to see that it took.
    ///
    /// Skipped unless `NFSU2_ROOT` is set, like every other test in this workspace that needs the
    /// game. It works on a **copy** in a temporary directory — a test that overwrote an install
    /// would be a bug report waiting to happen — and it exercises overwriting there, because the
    /// backup is the part of this module that only runs on that branch.
    #[test]
    fn a_real_pack_takes_a_real_image() {
        let Some(root) = std::env::var_os("NFSU2_ROOT").map(PathBuf::from) else {
            eprintln!("NFSU2_ROOT unset — skipping");
            return;
        };
        let src = root.join("CARS/240SX/TEXTURES.BIN");
        let Ok(bytes) = std::fs::read(&src) else { return };

        let dir = std::env::temp_dir().join(format!("pryhub-replace-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let pack = dir.join("TEXTURES.BIN");
        std::fs::write(&pack, &bytes).expect("copy the pack");

        // A texture, and an image of its size that is unmistakably not what was there.
        let entries = gizmo_nfs::Tpk::directory(&bytes).expect("directory");
        let (entry, tex) = entries
            .iter()
            .find_map(|e| gizmo_nfs::Tpk::decode_one(&bytes, e).ok().map(|t| (*e, t)))
            .expect("one decodable texture");
        let magenta: Vec<u8> = (0..tex.width as usize * tex.height as usize)
            .flat_map(|_| [255u8, 0, 255, 255])
            .collect();
        // `NfsTexture` is `#[non_exhaustive]` — a consumer is meant to get one from the parser
        // rather than build one — so the painted image goes through a clone of the real texture.
        let mut painted = tex.clone();
        painted.rgba = magenta;
        let png = dir.join("in.png");
        std::fs::write(&png, gizmo_nfs::export::png_bytes(&painted).expect("encode"))
            .expect("write the png");

        let done = run(&Spec {
            doc: pack.clone(),
            pack: pack.clone(),
            edits: vec![Edit { hash: entry.hash, name: tex.name.clone(), png }],
            over: true,
        })
        .expect("the replacement runs");
        assert_eq!(done.count, 1);

        assert_eq!(done.into, pack, "overwriting writes where it read");
        assert!(done.backup.is_some_and(|b| b.exists()), "a backup went down first");

        // And the file on disk now holds the new image.
        let after = std::fs::read(&pack).expect("read it back");
        let dir2 = gizmo_nfs::Tpk::directory(&after).expect("directory");
        let e2 = dir2.iter().find(|e| e.hash == entry.hash).expect("descriptor");
        let got = gizmo_nfs::Tpk::decode_one(&after, e2).expect("decode");
        let magenta_pixels = got
            .rgba
            .chunks_exact(4)
            .filter(|p| p[0] > 200 && p[1] < 60 && p[2] > 200)
            .count();
        let total = got.rgba.len() / 4;
        assert!(magenta_pixels * 10 >= total * 9, "{magenta_pixels} of {total} came back magenta");

        // Two at once, into a *copy* — the case that was wrong before there was a set. Written one
        // at a time each read the pack from disk, so the second produced a copy holding the second
        // texture and not the first. Here both go in, and the assertion is that both are there.
        let second = texs_two(&bytes, entry.hash);
        if let Some((e2, t2)) = second {
            let cyan: Vec<u8> = (0..t2.width as usize * t2.height as usize)
                .flat_map(|_| [0u8, 255, 255, 255])
                .collect();
            let mut painted2 = t2.clone();
            painted2.rgba = cyan;
            let png2 = dir.join("in2.png");
            std::fs::write(&png2, gizmo_nfs::export::png_bytes(&painted2).expect("encode"))
                .expect("write");
            let mut painted1 = tex.clone();
            painted1.rgba =
                (0..tex.width as usize * tex.height as usize).flat_map(|_| [255u8, 0, 255, 255]).collect();
            let png1 = dir.join("in1.png");
            std::fs::write(&png1, gizmo_nfs::export::png_bytes(&painted1).expect("encode"))
                .expect("write");

            let both = dir.join("both.BIN");
            std::fs::write(&both, &bytes).expect("a fresh copy to write into");
            let done2 = run(&Spec {
                doc: both.clone(),
                pack: both.clone(),
                edits: vec![
                    Edit { hash: entry.hash, name: tex.name.clone(), png: png1 },
                    Edit { hash: e2, name: t2.name.clone(), png: png2 },
                ],
                over: true,
            })
            .expect("two at once");
            assert_eq!(done2.count, 2);

            let after2 = std::fs::read(&both).expect("read back");
            let dir3 = gizmo_nfs::Tpk::directory(&after2).expect("directory");
            let count_of = |hash, want: [u8; 3]| {
                let d = dir3.iter().find(|x| x.hash == hash).expect("descriptor");
                let t = gizmo_nfs::Tpk::decode_one(&after2, d).expect("decode");
                let n = t.rgba.len() / 4;
                let hit = t
                    .rgba
                    .chunks_exact(4)
                    .filter(|p| {
                        p.iter().take(3).zip(want.iter()).all(|(a, b)| a.abs_diff(*b) < 60)
                    })
                    .count();
                (hit, n)
            };
            let (a_hit, a_n) = count_of(entry.hash, [255, 0, 255]);
            let (b_hit, b_n) = count_of(e2, [0, 255, 255]);
            assert!(a_hit * 10 >= a_n * 9, "the first edit is missing: {a_hit} of {a_n}");
            assert!(b_hit * 10 >= b_n * 9, "the second edit is missing: {b_hit} of {b_n}");
        }

        // And the reason `PryHub::refresh_after` reopens the document rather than merely dropping
        // the decoded pack: a `Doc` is a *snapshot*. One opened before the write goes on decoding
        // the bytes it read at open time, however many times it is asked, because
        // `decode_textures` reads `self.bytes` when the open file is itself the pack. Nothing short
        // of opening it again sees what is now on disk — which is why the interface used to redraw
        // the pre-edit image under a log line saying it had written a new one.
        let stale = crate::doc::Doc::open(&pack).expect("open");
        // (opened *after* the write, so this one is fresh — then the file changes under it.)
        std::fs::write(&pack, &bytes).expect("put the original back");
        let still = stale.decode_textures(&|_, _| {}).expect("decode").0;
        let from_snapshot = still.texture(entry.hash).expect("the texture").rgba.clone();
        let magenta_in_snapshot = from_snapshot
            .chunks_exact(4)
            .filter(|p| p[0] > 200 && p[1] < 60 && p[2] > 200)
            .count();
        assert!(
            magenta_in_snapshot * 10 >= total * 9,
            "a Doc decodes the bytes it was opened with, not the file: {magenta_in_snapshot} of {total}"
        );
        let reopened = crate::doc::Doc::open(&pack).expect("reopen");
        let fresh = reopened.decode_textures(&|_, _| {}).expect("decode").0;
        let magenta_after_reopen = fresh
            .texture(entry.hash)
            .expect("the texture")
            .rgba
            .chunks_exact(4)
            .filter(|p| p[0] > 200 && p[1] < 60 && p[2] > 200)
            .count();
        assert!(
            magenta_after_reopen * 10 < total,
            "reopening is what sees the file: {magenta_after_reopen} of {total} still magenta"
        );

        std::fs::remove_dir_all(&dir).ok();
    }
}
