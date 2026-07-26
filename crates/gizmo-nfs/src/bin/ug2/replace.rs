//! `ug2 replace` — put a PNG back into a texture pack.
//!
//! The first command here that *writes* an asset file. Everything else in this tool reads, so the
//! shape of this one is deliberately cautious: it never writes over its input unless told twice, it
//! says which of the two write paths it took, and it re-reads what it wrote before claiming success.

use crate::paths::{read, Result};
use gizmo_nfs::texture::{replace_image, Tpk, TpkEntry};
use gizmo_nfs::AssetHash;
use std::path::{Path, PathBuf};

pub fn run(pack: &Path, texture: &str, png: &Path, out: &Path, force: bool) -> Result<()> {
    let pack_path = resolve(pack)?;
    if !force && same_file(out, &pack_path) {
        return Err(format!(
            "{} is the file being read; pass --force to overwrite it, or -o somewhere else",
            out.display()
        ));
    }

    let bytes = read(&pack_path)?;
    let entries = Tpk::directory(&bytes).map_err(|e| format!("{}: {e}", pack_path.display()))?;
    let entry = find(&bytes, &entries, texture)?;

    let before = Tpk::decode_one(&bytes, &entry)
        .map_err(|e| format!("that texture does not decode, so it cannot be written either: {e}"))?;

    let image = read(png)?;
    let (rgba, w, h) = gizmo_nfs::export::png_pixels(&image).map_err(|e| format!("{}: {e}", png.display()))?;
    if (w, h) != (before.width, before.height) {
        return Err(format!(
            "{} is {w}×{h}; {} is {}×{}. A replacement keeps the texture's own dimensions — they are \
             recorded in the blob's embedded header, which the descriptor locates from the end.",
            png.display(),
            before.name,
            before.width,
            before.height
        ));
    }

    // The encode, the in-place attempt and the fall back to relocation are one function in the
    // library, so this command and PryHUB's texture tab cannot reach different conclusions about
    // when a pack has to be rewritten.
    let (written, moved) =
        replace_image(&bytes, entry.hash, &rgba, w, h).map_err(|e| format!("{e}"))?;
    let how = if moved { "relocated" } else { "in place" };

    // Read back what is about to be written, before writing it. A pack that does not decode is not
    // one to hand somebody with a message saying it worked.
    let check = Tpk::directory(&written).map_err(|e| format!("the written pack has no directory: {e}"))?;
    let e2 = check
        .iter()
        .find(|x| x.hash == entry.hash)
        .ok_or("the written pack lost the descriptor it was given")?;
    let after = Tpk::decode_one(&written, e2)
        .map_err(|e| format!("the written pack does not decode back: {e}"))?;

    if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(out, &written).map_err(|e| format!("{}: {e}", out.display()))?;

    outln!("{} {:#010x}  {}×{}  {:?}", before.name, entry.hash.0, w, h, before.source_format);
    outln!("  {} → {}", png.display(), out.display());
    outln!(
        "  {how}; {} bytes → {} bytes; every other texture in the pack untouched",
        bytes.len(),
        written.len()
    );
    // What the round trip cost, said rather than assumed. BGRA and the palettised tags come back
    // identical; S3TC cannot, and a caller replacing a headlight should be told which they got.
    let diff = after
        .rgba
        .iter()
        .zip(rgba.iter())
        .filter(|(a, b)| a != b)
        .count();
    if diff == 0 {
        outln!("  read back identical to the PNG");
    } else {
        let mse: f64 = after
            .rgba
            .iter()
            .zip(rgba.iter())
            .map(|(a, b)| {
                let d = f64::from(i32::from(*a) - i32::from(*b));
                d * d
            })
            .sum::<f64>()
            / after.rgba.len() as f64;
        let psnr = 10.0 * (255.0f64 * 255.0 / mse).log10();
        outln!("  read back at {psnr:.1} dB — {:?} stores two endpoints per 4×4 block", before.source_format);
    }
    Ok(())
}

/// Whether two paths name the same file on disk.
///
/// Compared after resolving rather than as written, because the guard this feeds is about *intent*
/// and the strings routinely disagree while the file does not: `-o TEXTURES.BIN` from inside a car's
/// directory is the same file as `./TEXTURES.BIN`, and a comparison of the two paths says otherwise.
/// A target that does not exist yet cannot be the input, which is the case the `unwrap_or(false)`
/// covers.
pub fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// A pack path, or a car directory to find one in.
fn resolve(path: &Path) -> Result<PathBuf> {
    if path.is_file() {
        return Ok(path.to_path_buf());
    }
    let guess = path.join("TEXTURES.BIN");
    if guess.is_file() {
        return Ok(guess);
    }
    Err(format!("{}: not a pack, and no TEXTURES.BIN in it", path.display()))
}

/// Find one texture by hash (`0x…`) or by name.
///
/// Names are matched through the truncation rather than around it: the field is 23 characters, so a
/// long name arrives cut and the thing a person types is often longer than what the file holds. An
/// exact match wins; failing that, a name the file's own (possibly cut) name is a prefix of; failing
/// that, a unique substring.
///
/// **Ambiguity is checked in every one of the three**, including the exact match. Two textures
/// sharing a name is normal in this format rather than an edge case, so "the first one called that"
/// is never an answer — the candidates are listed with their hashes, which is the thing that tells
/// them apart and the thing to pass instead.
fn find(bytes: &[u8], entries: &[TpkEntry], want: &str) -> Result<TpkEntry> {
    if let Some(hex) = want.strip_prefix("0x").or_else(|| want.strip_prefix("0X")) {
        let hash = u32::from_str_radix(hex, 16).map_err(|_| format!("{want}: not a hash"))?;
        return entries
            .iter()
            .find(|e| e.hash == AssetHash(hash))
            .copied()
            .ok_or_else(|| format!("{want}: no texture with that hash in this pack"));
    }

    let named: Vec<(TpkEntry, String)> = entries
        .iter()
        .filter_map(|e| Tpk::name_of(bytes, e).ok().map(|n| (*e, n)))
        .collect();
    let upper = want.to_ascii_uppercase();

    // Three ways to match, narrowest first — but all three funnel through the same ambiguity check
    // at the bottom, and the exact one especially. An exact match used to short-circuit on the first
    // hit, which is precisely backwards for this format: names are truncated to 23 characters, so
    // *sharing* one is ordinary rather than exceptional — `240SX_DOORLINE_WIDEBODY` and its `_MASK`
    // twin arrive under one name — and a command that writes a user's PNG into whichever of them
    // comes first in file order, without saying there was a choice, is the one failure this whole
    // function exists to prevent.
    let mut hits: Vec<&(TpkEntry, String)> =
        named.iter().filter(|(_, n)| n.eq_ignore_ascii_case(want)).collect();
    if hits.is_empty() {
        hits = named
            .iter()
            .filter(|(_, n)| upper.starts_with(&n.to_ascii_uppercase()) && !n.is_empty())
            .collect();
    }
    if hits.is_empty() {
        hits = named.iter().filter(|(_, n)| n.to_ascii_uppercase().contains(&upper)).collect();
    }
    match hits.len() {
        0 => Err(format!("{want}: no texture of that name in this pack")),
        1 => Ok(hits[0].0),
        _ => {
            // By hash, because that is the thing that tells two textures under one truncated name
            // apart — and the thing the next invocation should be given.
            let mut msg = format!("{want} names {} textures — say which by hash:\n", hits.len());
            for (e, n) in hits.iter().take(12) {
                msg.push_str(&format!("  {:#010x}  {n}\n", e.hash.0));
            }
            Err(msg)
        }
    }
}
