//! Decoding one texture blob: JDLZ or HUFF decompression (by magic — a pack mixes both), the
//! embedded `OldTextureInfo` header, and the pixel formats behind its `ImageCompressionType` code.

use super::directory::{texture_name, TpkEntry};
use super::dxt;
use crate::error::{NfsError, NfsResult};
use crate::types::{NfsTexture, PixelFormat, TexFormat};

/// Bytes per pixel in the decoded output.
const RGBA: usize = 4;

/// `ImageCompressionType` codes (embedded `OldTextureInfo`, byte at `P+38`).
mod fmt {
    pub const RGBA8888: u8 = 0x20; // 32bpp, stored B,G,R,A
    pub const DXT1: u8 = 0x22;
    pub const DXT3: u8 = 0x24;
    pub const DXT5: u8 = 0x26;
    pub const P8: u8 = 0x08; // palettised — sized, not decoded
}

// Palettised tags this crate does not decode, counted over one install's 54,885 descriptors:
// `0x08` (25,960), `0x80` (24,071) and `0x81` (1,840). Together they are 94.5% of every TPK image
// the game ships — the whole of every VINYLS.BIN — so "the formats we do not decode" is not a
// footnote about a rare tag. Only `0x08` is named above; the other two are recorded here because a
// number nobody wrote down is a number the next reader has to measure again.

/// The largest texture dimension we will decode (guards allocation from a corrupt header).
const MAX_DIM: usize = 4096;

/// Decompress and decode a single texture into RGBA8. Errors (a pixel format this crate does not
/// decode, an unreadable blob, a malformed header, out-of-range dimensions) mean "skip this
/// texture", not a corrupt file. The first is much the commonest: measured over one install, 51,871
/// of 54,885 declared textures are palettised, which is the whole of every `VINYLS.BIN`.
pub(super) fn decode_texture(file: &[u8], e: &TpkEntry) -> NfsResult<NfsTexture> {
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

    // Locate the embedded OldTextureInfo header and read the fields we need.
    let p = out_size
        .checked_sub(e.header_from_end as usize)
        .and_then(|h| h.checked_add(0x64 + 0x24))
        .ok_or(NfsError::CorruptArchive { detail: "TPK header offset underflow" })?;
    let hdr = pool
        .get(p..p + 39)
        .ok_or(NfsError::CorruptArchive { detail: "TPK header out of range" })?;
    // Self-check: the u32 at P is the texture's own hash. If it isn't, the header formula
    // doesn't apply to this texture — skip rather than decode noise.
    let name_hash = u32::from_le_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]);
    if name_hash != e.hash.0 {
        return Err(NfsError::CorruptArchive { detail: "TPK header hash mismatch" });
    }
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

    let top = top_mip_size(width, height, comp)
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
        // `P8` reaches here: it has a name and a known size, and no decoder. See `named_format`.
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
/// interface, the exporter and the next reader that we did not recognise it. `P8` is the one
/// deliberate case of a name with no decoder — the size is arithmetic, the palette layout is not
/// locked.
fn named_format(comp: u8) -> Option<TexFormat> {
    Some(match comp {
        fmt::DXT1 => TexFormat::Dxt1,
        fmt::DXT3 => TexFormat::Dxt3,
        fmt::DXT5 => TexFormat::Dxt5,
        fmt::RGBA8888 => TexFormat::Bgra8888,
        fmt::P8 => TexFormat::P8,
        _ => return None,
    })
}

/// Byte size of the top mipmap for `width`x`height` in the given compression type, or `None`
/// for a format we do not decode.
fn top_mip_size(width: usize, height: usize, comp: u8) -> Option<usize> {
    let blocks = width.div_ceil(4) * height.div_ceil(4);
    match comp {
        fmt::DXT1 => Some(blocks * 8),
        fmt::DXT3 | fmt::DXT5 => Some(blocks * 16),
        fmt::RGBA8888 => Some(width * height * RGBA),
        fmt::P8 => Some(width * height), // decode not yet supported, but size is known
        _ => None,
    }
}

/// Unpack the `0x20` format: 32-bit source stored B,G,R,A → RGBA8, alpha preserved.
fn unpack_bgra(src: &[u8], width: usize, height: usize) -> Vec<u8> {
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
    fn top_mip_sizes_match_s3tc() {
        // DXT1 128x128 = 32*32 blocks * 8 = 8192 bytes.
        assert_eq!(top_mip_size(128, 128, fmt::DXT1), Some(8192));
        // DXT3/5 double that.
        assert_eq!(top_mip_size(128, 128, fmt::DXT3), Some(16384));
        assert_eq!(top_mip_size(64, 32, fmt::DXT5), Some(64 / 4 * 32 / 4 * 16));
        // RGBA is 4 bytes/pixel.
        assert_eq!(top_mip_size(16, 16, fmt::RGBA8888), Some(16 * 16 * 4));
        // Unknown format -> None.
        assert_eq!(top_mip_size(16, 16, 0x99), None);
    }

    /// Every format with a size is a format with a name. `top_mip_size` returning `Some` is the
    /// crate saying "I know this layout"; reporting `Unknown` for the same tag would contradict it.
    ///
    /// It asserts through [`named_format`] — the function `decode_texture` itself reports from —
    /// rather than against a hand-copied list of the same `fmt::` constants `top_mip_size` matches
    /// on. Written that second way, as it first was, the two sets were the same five constants by
    /// construction and the assertion could not fail: it passed identically with the format left
    /// anonymous, which is the one thing it existed to catch.
    #[test]
    fn a_format_the_crate_can_measure_is_a_format_it_can_name() {
        for comp in 0u8..=255 {
            if top_mip_size(64, 64, comp).is_none() {
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
        // The one name without a decoder, and a tag with neither.
        assert_eq!(named_format(fmt::P8), Some(TexFormat::P8));
        assert_eq!(named_format(0x99), None);
    }

    #[test]
    fn bgra_unpack_reorders_to_rgba() {
        // one BGRA pixel (B=1,G=2,R=3,A=4) -> RGBA (3,2,1,4)
        assert_eq!(unpack_bgra(&[1, 2, 3, 4], 1, 1), vec![3, 2, 1, 4]);
    }
}
