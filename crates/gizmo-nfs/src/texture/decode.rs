//! Decoding one texture blob: JDLZ or HUFF decompression (by magic — a pack mixes both), the
//! embedded `OldTextureInfo` header, and the pixel formats behind its `ImageCompressionType` code.

use super::directory::{texture_name, TpkEntry};
use super::dxt;
use crate::error::{NfsError, NfsResult};
use crate::types::{NfsTexture, PixelFormat, TexFormat};

/// Bytes per pixel in the decoded output.
const RGBA: usize = 4;

/// `ImageCompressionType` codes (embedded `OldTextureInfo`, byte at `P+38`).
pub(crate) mod fmt {
    pub const RGBA8888: u8 = 0x20; // 32bpp, stored B,G,R,A
    pub const DXT1: u8 = 0x22;
    pub const DXT3: u8 = 0x24;
    pub const DXT5: u8 = 0x26;
    pub const P8: u8 = 0x08; // 8-bit indices, palette populated to 256
    pub const P8_64: u8 = 0x81; // the same layout, palette populated to 64
    pub const P8_16: u8 = 0x80; // the same layout, palette populated to 16
}

/// Is this one of the three tags that store one palette index per pixel?
pub(crate) const fn is_palettised(comp: u8) -> bool {
    matches!(comp, fmt::P8 | fmt::P8_16 | fmt::P8_64)
}

// The three palettised tags, counted over one install's 54,885 descriptors: `0x08` (25,960),
// `0x80` (24,071) and `0x81` (1,840) — 94.5% of every TPK image the game ships, and the whole of
// every `VINYLS.BIN`. They are **one layout**: a 1024-byte palette and one index byte per pixel, in
// the same place in the blob, which is why one arm decodes all three.
//
// What the tag says is how much of the palette is populated, and that is measured rather than read
// off the number: across all 24,071 `0x80` textures no pixel ever indexes above **15**, and across
// all 1,840 `0x81` textures never above **63**, where `0x08` reaches 255. The commonest distinct
// count is the cap itself — 16 for `0x80` (14,591 of them), 64 for `0x81`, 256 for `0x08` — so these
// are 16-, 64- and 256-colour images sharing one 8-bit index. Nothing in the decode depends on it:
// an index that never exceeds the populated region reads the same either way. It is recorded because
// the alternative reading — that `0x80` is 4-bit packed — is ruled out by the same arithmetic that
// locks the offsets: `ImageSize` is the full 8-bit mip chain, 349,504 for a 512², not half of it.
// (Every `0x80` and `0x81` texture in the install is 512²; `0x08` is 25,916 of 25,960, the rest
// being small `HUD_CustomTextures` strips.)
//
// The tags also differ in what they carry, which is content and not shape: `0x08` is mostly opaque
// greyscale ramps (23,042 of 25,960 have a palette whose entries are all B == G == R, and 25,580 are
// fully opaque), while `0x80` is colour with alpha (17,247 of 24,071 have partial alpha, and exactly
// one has a greyscale palette).

/// A palette is 256 entries of four bytes.
///
/// `PaletteSize` at `+0x18` says 1024 in every one of the install's 51,871 palettised textures, and
/// [`palette_of`] checks it against this rather than trusting it — a pack that says otherwise is a
/// pack this arm has not been shown, and decoding it against the wrong length would silently read a
/// neighbouring mip as colours.
pub(crate) const PALETTE_BYTES: usize = 256 * 4;

/// The largest texture dimension we will decode (guards allocation from a corrupt header).
pub(crate) const MAX_DIM: usize = 4096;

/// Bytes of the embedded header this module reads, and the constant that finds it.
///
/// `P` is not stored: it is `out_size − header_from_end` plus a fixed distance into the structure
/// the descriptor points at. Both halves are the file's own numbers; only the `0x88` is ours, and it
/// is the same one [`decompress_to_header`] has always used — factored out so the write side finds
/// the header at the byte the read side does rather than at a byte that agrees today.
pub(super) const HEADER_LEN: usize = 39;
const HEADER_INTO: usize = 0x64 + 0x24;

/// Where the embedded `OldTextureInfo` sits inside a decompressed blob of `out_size` bytes.
pub(super) fn header_at(out_size: usize, header_from_end: usize) -> NfsResult<usize> {
    out_size
        .checked_sub(header_from_end)
        .and_then(|h| h.checked_add(HEADER_INTO))
        .ok_or(NfsError::CorruptArchive { detail: "TPK header offset underflow" })
}

/// One texture's blob, decompressed, and the offset of its embedded `OldTextureInfo` header.
///
/// Everything up to and including the header's own self-check, which is all a caller needs to read
/// the `DebugName` and no more. Split out because a name costs the decompression and nothing else:
/// the pixels are a second pass over the same buffer, and a caller after the names of a 1,786-image
/// pack should not have to allocate 1.87 GB of RGBA8 to get them.
fn decompress_to_header(file: &[u8], e: &TpkEntry) -> NfsResult<(Vec<u8>, usize)> {
    let abs = e.abs_offset as usize;
    let end = abs.checked_add(e.size as usize).ok_or(NfsError::CorruptArchive {
        detail: "TPK texture offset+size overflow",
    })?;
    let blob = file
        .get(abs..end)
        .ok_or(NfsError::CorruptArchive { detail: "TPK texture blob out of range" })?;

    // The codec is whatever the blob's magic says — JDLZ or HUFF, and a pack mixes both.
    let pool = crate::compression::decompress(blob)?;
    let out_size = e.out_size as usize;
    if pool.len() < out_size {
        return Err(NfsError::BufferSizeMismatch { detail: "TPK blob decompressed short" });
    }

    // Locate the embedded OldTextureInfo header.
    let p = header_at(out_size, e.header_from_end as usize)?;
    let hdr = pool
        .get(p..p + HEADER_LEN)
        .ok_or(NfsError::CorruptArchive { detail: "TPK header out of range" })?;
    // Self-check: the u32 at P is the texture's own hash. If it isn't, the header formula
    // doesn't apply to this texture — skip rather than decode noise.
    let name_hash = u32::from_le_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]);
    if name_hash != e.hash.0 {
        return Err(NfsError::CorruptArchive { detail: "TPK header hash mismatch" });
    }
    Ok((pool, p))
}

/// The `DebugName` the compiler left beside one texture, without decoding its pixels.
///
/// The same name [`decode_texture`] reports, reached the same way and failing the same self-check,
/// so a pack read for its names and a pack read for its images agree about what is in it.
pub(super) fn texture_name_only(file: &[u8], e: &TpkEntry) -> NfsResult<String> {
    let (pool, p) = decompress_to_header(file, e)?;
    Ok(texture_name(&pool, p))
}

/// Decompress and decode a single texture into RGBA8. Errors (a pixel format this crate does not
/// decode, an unreadable blob, a malformed header, out-of-range dimensions) mean "skip this
/// texture", not a corrupt file.
pub(super) fn decode_texture(file: &[u8], e: &TpkEntry) -> NfsResult<NfsTexture> {
    let (pool, p) = decompress_to_header(file, e)?;
    let hdr = pool
        .get(p..p + 39)
        .ok_or(NfsError::CorruptArchive { detail: "TPK header out of range" })?;
    // The `DebugName[24]` sits just before the NameHash (struct 0x0C, i.e. `P − 0x18`); it
    // carries the texture's readable name (e.g. `240SX_KIT00_HEADLIGHT`), which the renderer
    // matches to part names.
    let name = texture_name(&pool, p);
    let width = u16::from_le_bytes([hdr[32], hdr[33]]) as usize;
    let height = u16::from_le_bytes([hdr[34], hdr[35]]) as usize;
    let comp = hdr[38];
    if width == 0 || height == 0 || width > MAX_DIM || height > MAX_DIM {
        return Err(NfsError::CorruptArchive { detail: "TPK texture dimensions out of range" });
    }

    let top = level_size(width, height, comp)
        .ok_or(NfsError::NotImplemented { feature: "TPK pixel format" })?;
    let pixels = pool
        .get(0..top)
        .ok_or(NfsError::BufferSizeMismatch { detail: "TPK pixel data shorter than top mip" })?;

    let source_format =
        named_format(comp).ok_or(NfsError::NotImplemented { feature: "TPK pixel format" })?;
    let rgba = match comp {
        fmt::DXT1 => dxt::decode_dxt1(pixels, width, height),
        fmt::DXT3 => dxt::decode_dxt3(pixels, width, height),
        fmt::DXT5 => dxt::decode_dxt5(pixels, width, height),
        // Decoded, so it is named. It reported as `Unknown(32)` for as long as the enum had no
        // variant for it, which put "we do not know this format" next to every `_DOORLINE` in the
        // install — a texture the crate unpacks correctly and has done all along.
        fmt::RGBA8888 => unpack_bgra(pixels, width, height),
        c if is_palettised(c) => {
            unpack_palettised(pixels, palette_of(&pool, hdr, top)?, width, height)
        }
        _ => return Err(NfsError::NotImplemented { feature: "TPK pixel format" }),
    };

    Ok(NfsTexture {
        name,
        hash: e.hash,
        width: width as u32,
        height: height as u32,
        rgba,
        source_format,
        format: PixelFormat::Rgba8,
    })
}

/// The [`TexFormat`] a compression tag is reported as, or `None` for a tag this module has never
/// been shown.
///
/// It is its own function so the invariant below it is testable against the decoder rather than
/// against a second copy of the decoder's arm list: a tag the crate can *size* or *decode* must
/// have a name, because `Unknown` next to a texture we have just finished unpacking tells the
/// interface, the exporter and the next reader that we did not recognise it. Every tag named here
/// now decodes.
pub(crate) fn named_format(comp: u8) -> Option<TexFormat> {
    Some(match comp {
        fmt::DXT1 => TexFormat::Dxt1,
        fmt::DXT3 => TexFormat::Dxt3,
        fmt::DXT5 => TexFormat::Dxt5,
        fmt::RGBA8888 => TexFormat::Bgra8888,
        // Three tags, one layout, so one name. They differ in how much of the palette is
        // populated (256 / 64 / 16) and not in how a decoder reads them, and `TexFormat` names
        // on-disk *layouts* — splitting it three ways would report a difference the pixels do not
        // have.
        fmt::P8 | fmt::P8_16 | fmt::P8_64 => TexFormat::P8,
        _ => return None,
    })
}

/// Byte size of one mip level at `width`x`height` in the given compression type, or `None`
/// for a format we do not decode.
///
/// Named for a *level* rather than for the top one because the write side walks the whole chain
/// with it: the same arithmetic that says where the top mip ends says where every level below it
/// does, and the two sides must agree or a re-encoded texture would write its second level over
/// the tail of its first.
pub(crate) fn level_size(width: usize, height: usize, comp: u8) -> Option<usize> {
    let blocks = width.div_ceil(4) * height.div_ceil(4);
    match comp {
        fmt::DXT1 => Some(blocks * 8),
        fmt::DXT3 | fmt::DXT5 => Some(blocks * 16),
        fmt::RGBA8888 => Some(width * height * RGBA),
        // All three palettised tags are one index byte per pixel; the palette is separate.
        fmt::P8 | fmt::P8_16 | fmt::P8_64 => Some(width * height),
        _ => None,
    }
}

/// The palette bytes inside the decompressed blob, exactly [`PALETTE_BYTES`] of them.
///
/// The offset is **not** derived from `ImageSize` at `+0x14`, and the reason is a measurement rather
/// than a preference. That field is the mip chain (349,504 for the 512² these textures almost all
/// are, i.e. down to 8×8), and the palette begins 64 bytes past it in 51,844 of the install's 51,871
/// palettised textures — but exactly *at* it in the other 27, which are the small `HUD_CustomTextures`
/// and the `MAGAZINES_STREAMING_HIGH` sheets. So neither `ImageSize` nor `ImageSize + 64` is the
/// answer; there is no constant to add.
///
/// The header states the position itself. `+0x0C` is this image's offset and `+0x10` its palette's,
/// both into a conceptual concatenation of every texture in the pack, so their **difference** is the
/// palette's offset within this one blob. Read that way it lands in range for all 51,871, with the
/// top mip always ending before it.
/// It returns a **fixed-size array**, which is what makes the unchecked indexing in
/// [`unpack_palettised`] safe by construction rather than by the caller being careful: a `u8` index
/// times four plus three is at most 1023, and the type says there are 1024 bytes. Returned as a
/// slice instead, the same loop is correct only for as long as nobody hands it a shorter one.
fn palette_of<'a>(pool: &'a [u8], hdr: &[u8], top: usize) -> NfsResult<&'a [u8; PALETTE_BYTES]> {
    let start = palette_at(hdr, top)?;
    pool.get(start..start + PALETTE_BYTES)
        .and_then(|s| s.try_into().ok())
        .ok_or(NfsError::BufferSizeMismatch { detail: "TPK palette out of range" })
}

/// Where the palette begins inside the decompressed blob.
///
/// Split out of [`palette_of`] so the write side puts a re-quantised palette at the byte the read
/// side will look for it, rather than at a byte derived a second time and agreeing by luck. All the
/// reasoning below is about this arithmetic; [`palette_of`] is only the slice it names.
pub(super) fn palette_at(hdr: &[u8], top: usize) -> NfsResult<usize> {
    let at = |o: usize| -> Option<u32> {
        let b = hdr.get(o..o + 4)?;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    // `PaletteSize` is checked, not trusted: every measured pack says 1024, and one that says
    // anything else is a layout this arm has not been shown.
    if at(0x18) != Some(PALETTE_BYTES as u32) {
        return Err(NfsError::NotImplemented { feature: "TPK palette size" });
    }
    let image = at(0x0C).ok_or(NfsError::CorruptArchive { detail: "TPK header out of range" })?;
    let palette = at(0x10).ok_or(NfsError::CorruptArchive { detail: "TPK header out of range" })?;
    let start = palette
        .checked_sub(image)
        .ok_or(NfsError::CorruptArchive { detail: "TPK palette before image" })? as usize;
    // The palette begins after the image, never inside it — and "the image" is the whole mip chain,
    // not just the mip being decoded. `ImageSize` at `+0x14` is that chain, and the measured gap to
    // the palette is 0 or +64 and never negative, so this holds for all 51,871. Worth stating
    // because the failure it prevents is silent: a palette read from within the chain still decodes,
    // into colours that are somebody else's pixels. `top` is the floor when `ImageSize` is the
    // smaller of the two, so a header understating it cannot buy its way past the pixels we read.
    let floor = top.max(at(0x14).unwrap_or(0) as usize);
    if start < floor {
        return Err(NfsError::CorruptArchive { detail: "TPK palette overlaps the image" });
    }
    start
        .checked_add(PALETTE_BYTES)
        .ok_or(NfsError::CorruptArchive { detail: "TPK palette offset overflow" })?;
    Ok(start)
}

/// Unpack one palette index per pixel into RGBA8.
///
/// The palette entries are stored B,G,R,A, the same order the uncompressed `0x20` format uses — the
/// engine is reading both through the same D3D surface description. Confirmed on images that can
/// say so rather than on the ordering being plausible: a greyscale ramp reads the same either way,
/// so it was checked against `240SX_FLAGS_THAILAND` (red outer bands, blue centre — a swap inverts
/// them) and the `AEM_CLARION` wordmark, which comes out in Clarion's own red.
///
/// `src` may be shorter than the image only if the blob was truncated, in which case the tail stays
/// the zero the buffer was allocated with rather than reading whatever follows.
pub(crate) fn unpack_palettised(
    src: &[u8],
    palette: &[u8; PALETTE_BYTES],
    width: usize,
    height: usize,
) -> Vec<u8> {
    let mut out = vec![0u8; width * height * RGBA];
    for (dst, &index) in out.chunks_exact_mut(4).zip(src.iter()) {
        // `index` is a byte and `palette` is 256 four-byte entries *by type*, so `e + 3` is at most
        // 1023. The bound is the array's, not a caller's promise, so these need no check.
        let e = index as usize * 4;
        dst[0] = palette[e + 2];
        dst[1] = palette[e + 1];
        dst[2] = palette[e];
        dst[3] = palette[e + 3];
    }
    out
}

/// Unpack the `0x20` format: 32-bit source stored B,G,R,A → RGBA8, alpha preserved.
pub(crate) fn unpack_bgra(src: &[u8], width: usize, height: usize) -> Vec<u8> {
    let mut out = vec![0u8; width * height * RGBA];
    for (dst, s) in out.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
        dst.copy_from_slice(&[s[2], s[1], s[0], s[3]]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_sizes_match_s3tc() {
        // DXT1 128x128 = 32*32 blocks * 8 = 8192 bytes.
        assert_eq!(level_size(128, 128, fmt::DXT1), Some(8192));
        // DXT3/5 double that.
        assert_eq!(level_size(128, 128, fmt::DXT3), Some(16384));
        assert_eq!(level_size(64, 32, fmt::DXT5), Some(64 / 4 * 32 / 4 * 16));
        // RGBA is 4 bytes/pixel.
        assert_eq!(level_size(16, 16, fmt::RGBA8888), Some(16 * 16 * 4));
        // Unknown format -> None.
        assert_eq!(level_size(16, 16, 0x99), None);
    }

    /// Every format with a size is a format with a name. `level_size` returning `Some` is the
    /// crate saying "I know this layout"; reporting `Unknown` for the same tag would contradict it.
    ///
    /// It asserts through [`named_format`] — the function `decode_texture` itself reports from —
    /// rather than against a hand-copied list of the same `fmt::` constants `level_size` matches
    /// on. Written that second way, as it first was, the two sets were the same five constants by
    /// construction and the assertion could not fail: it passed identically with the format left
    /// anonymous, which is the one thing it existed to catch.
    #[test]
    fn a_format_the_crate_can_measure_is_a_format_it_can_name() {
        for comp in 0u8..=255 {
            if level_size(64, 64, comp).is_none() {
                continue;
            }
            let named = named_format(comp);
            assert!(
                matches!(named, Some(f) if !matches!(f, TexFormat::Unknown(_))),
                "0x{comp:02X} has a size but no name: {named:?}"
            );
        }
        // Named individually as well, so reverting one arm fails here and not only in the loop.
        assert_eq!(named_format(fmt::RGBA8888), Some(TexFormat::Bgra8888));
        assert_eq!(named_format(fmt::DXT1), Some(TexFormat::Dxt1));
        // All three palettised tags answer to the one name, and a tag with neither.
        assert_eq!(named_format(fmt::P8), Some(TexFormat::P8));
        assert_eq!(named_format(fmt::P8_16), Some(TexFormat::P8));
        assert_eq!(named_format(fmt::P8_64), Some(TexFormat::P8));
        assert_eq!(named_format(0x99), None);
    }

    #[test]
    fn bgra_unpack_reorders_to_rgba() {
        // one BGRA pixel (B=1,G=2,R=3,A=4) -> RGBA (3,2,1,4)
        assert_eq!(unpack_bgra(&[1, 2, 3, 4], 1, 1), vec![3, 2, 1, 4]);
    }

    /// A palette entry is B,G,R,A, and the index selects it. Written with four distinct channel
    /// values per entry so a swap of any pair fails rather than only a red/blue one.
    #[test]
    fn palettised_unpack_reads_bgra_entries() {
        let mut palette = [0u8; PALETTE_BYTES];
        palette[0..4].copy_from_slice(&[1, 2, 3, 4]); // entry 0
        palette[4..8].copy_from_slice(&[10, 20, 30, 40]); // entry 1
        palette[255 * 4..].copy_from_slice(&[9, 8, 7, 6]); // entry 255
        let out = unpack_palettised(&[0, 1, 255, 0], &palette, 2, 2);
        assert_eq!(out, vec![3, 2, 1, 4, 30, 20, 10, 40, 7, 8, 9, 6, 3, 2, 1, 4]);
    }

    /// The top index is 255 and the palette is 256 entries, so no byte can select past its end.
    /// The type already says so; this walks every one of the 256 anyway, because the loop is the
    /// crate's only unchecked indexing and a regression to a slice parameter should fail here.
    #[test]
    fn every_index_lands_inside_the_palette() {
        let palette = [7u8; PALETTE_BYTES];
        let all: Vec<u8> = (0..=255).collect();
        let out = unpack_palettised(&all, &palette, 16, 16);
        assert_eq!(out.len(), 16 * 16 * RGBA);
        assert!(out.iter().all(|&b| b == 7));
    }

    /// A source shorter than the image leaves the tail transparent black rather than reading on.
    #[test]
    fn a_short_blob_leaves_the_tail_zero() {
        let palette = [0xFFu8; PALETTE_BYTES];
        let out = unpack_palettised(&[0], &palette, 2, 1);
        assert_eq!(out, vec![255, 255, 255, 255, 0, 0, 0, 0]);
    }

    /// The palette is located by the header's own two placements, and its declared size is checked.
    #[test]
    fn palette_is_located_by_placement_difference() {
        // Exactly the width `decode_texture` slices — `pool.get(p..p + 39)` — so a field added to
        // `palette_of` past byte 39 fails here rather than only against a real install.
        let mut hdr = vec![0u8; 39];
        let put = |h: &mut Vec<u8>, o: usize, v: u32| h[o..o + 4].copy_from_slice(&v.to_le_bytes());
        put(&mut hdr, 0x0C, 5_000); // ImagePlacement
        put(&mut hdr, 0x10, 5_064); // PalettePlacement -> 64 bytes into this blob
        put(&mut hdr, 0x18, PALETTE_BYTES as u32);
        let mut pool = vec![0u8; 64 + PALETTE_BYTES];
        pool[64] = 0xAB;
        assert_eq!(palette_of(&pool, &hdr, 64).unwrap()[0], 0xAB);

        // A palette that would start before the image is refused, not wrapped around.
        let mut back = hdr.clone();
        put(&mut back, 0x10, 4_999);
        assert!(palette_of(&pool, &back, 64).is_err());

        // A size this arm has not been shown is refused rather than decoded against 1024.
        let mut sized = hdr.clone();
        put(&mut sized, 0x18, 512);
        assert!(palette_of(&pool, &sized, 64).is_err());

        // A palette that runs off the end of the blob is refused.
        assert!(palette_of(&pool[..64 + PALETTE_BYTES - 1], &hdr, 64).is_err());

        // A palette that would start inside the top mip is refused: those bytes are pixels.
        assert!(palette_of(&pool, &hdr, 65).is_err());

        // And one that would start inside a *later* mip, which the top-mip floor alone would let
        // through: `ImageSize` says the chain runs to 96, past the palette's stated 64.
        let mut chain = hdr.clone();
        put(&mut chain, 0x14, 96);
        assert!(palette_of(&pool, &chain, 8).is_err());
    }
}
