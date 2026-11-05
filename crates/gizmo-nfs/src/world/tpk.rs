//! The city's texture packs: the `0x33310004` variant of the TPK directory.
//!
//! A `STREAM*.BUN` carries whole texture packs inside it — 502 of them across the eight regions,
//! 5,087 textures. They are **not** the pack [`crate::texture::Tpk`] reads. That one is keyed by a
//! `0x33310003` table of 24-byte descriptors pointing at compressed, self-describing blobs; a track
//! pack has **no `0x33310003` at all** (0 of 502) and instead carries a `0x33310004` table of fixed
//! `0x7C`-byte records that state width, height, pixel format, mip count and a pool offset
//! outright. The pixels are raw — `+0x38` equals the exact mip chain in 5,087 of 5,087 records — so
//! there is nothing to decompress and no embedded `OldTextureInfo` to find.
//!
//! ```text
//! 0xB3300000                       pack
//!   0xB3310000                     directory
//!     0x33310001  124              info, byte-identical in all 502 packs — no count, no identity
//!     0x33310002  8 × N            (u32 key, u32 0); the keys equal the record keys, in order
//!     0x33310004  0x7C × N         the record table
//!     0x33310005  0x20 × N         D3DFORMAT at +0x14, agreeing with `+0x4A` in 5,087/5,087
//!   0xB3320000
//!     0x33320002                   120 bytes of 0x11 filler, then the pixel pool
//! ```
//!
//! **The pixel format is stated, so none is inferred.** `+0x4A` holds the same numeric tag
//! [`crate::texture::decode::fmt`] already names — `0x22` DXT1 (4,464), `0x24` DXT3 (532), `0x08`
//! P8 (91) — and `0x33310005 +0x14` states it a second time as a D3DFOURCC. Cross-tabulated over
//! the city the two are perfectly diagonal, 5,087 of 5,087. Inferring the format from the declared
//! size instead happens to work inside `TRACKS/` and breaks outside it: the same record layout in
//! `FRONTEND/ENVMAPS/` carries 256×256 `0x20` (uncompressed BGRA) textures whose size a
//! DXT1-or-DXT3 ratio test reads as DXT3 and decodes as noise. So the tag is read, handed to
//! [`crate::texture::decode::level_size`] unchanged, and a tag neither it nor `named_format` knows
//! is refused by name rather than guessed at.
//!
//! **The pool base is the filler-stripped `0x33320002` payload**, and that is proven rather than
//! assumed: with it, `max(image_offset + image_size, palette_offset + palette_size)` equals the
//! stripped payload's length *exactly*, in 502 of 502 packs. Read from the raw payload start
//! instead and every pack leaves 120 unclaimed bytes at its tail while the 483 packs whose first
//! image sits at offset 0 read filler as mip 0.
//!
//! **The palettes precede the images**, pooled at the front of that body at `0, 1024, 2048, …` —
//! the reverse of the car pack, whose `palette_at` enforces palette-after-image and would refuse
//! all 91 of the city's palettised records. `+0x34` is a direct pool offset, and it is stale
//! garbage when `+0x3C == 0` (all three DXT1 records in `STREAML4RH` carry `0x400`), so it is read
//! only when the record says there is a palette.

use crate::chunk::{walk_positions, Visit, WalkOptions};
use crate::error::{NfsError, NfsResult};
use crate::geometry::skip_leading_filler;
use crate::reader::ByteReader;
use crate::texture::decode::{
    fmt, is_palettised, level_size, named_format, unpack_bgra, unpack_palettised, MAX_DIM,
    PALETTE_BYTES,
};
use crate::texture::dxt;
use crate::types::{AssetHash, NfsTexture, PixelFormat};

/// The pack container.
pub const TRACK_PACK: u32 = 0xB330_0000;
/// The record table. There is no `0x33310003` in the city — 0 of 502 packs carry one.
pub const TRACK_RECORDS: u32 = 0x3331_0004;
/// The pixel pool.
pub const TRACK_PIXELS: u32 = 0x3332_0002;

/// Bytes per record. 502 of 502 tables are an exact multiple of this.
pub const RECORD_STRIDE: usize = 0x7C;

/// Field offsets within one record, all little-endian. Public for the same reason
/// [`crate::geometry::format`] is: an inspector that copies them is an inspector that starts
/// disagreeing with the parser about what a file says.
pub const REC_NAME: usize = 0x0C;
/// Width of the name field. It truncates rather than growing, which is why the key is the
/// identifier and the name is only a label.
pub const REC_NAME_LEN: usize = 24;
pub const REC_KEY: usize = 0x24;
pub const REC_IMAGE_OFFSET: usize = 0x30;
pub const REC_PALETTE_OFFSET: usize = 0x34;
pub const REC_IMAGE_SIZE: usize = 0x38;
pub const REC_PALETTE_SIZE: usize = 0x3C;
pub const REC_TOP_MIP_SIZE: usize = 0x40;
pub const REC_WIDTH: usize = 0x44;
pub const REC_HEIGHT: usize = 0x46;
pub const REC_COMPRESSION: usize = 0x4A;
pub const REC_MIP_LEVELS: usize = 0x4E;

/// One texture as a `0x33310004` record declares it. No pixels, no palette.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TrackTexture {
    /// `+0x0C`, NUL-terminated. The field truncates at 23 characters and 452 of the city's 5,087
    /// records are cut — ask [`TrackTexture::name_is_whole`] before using it as an identifier.
    pub name: String,
    /// `+0x24`. This, not the name, is what a solid's `0x00134012` slot list names.
    pub key: AssetHash,
    /// `+0x30`, into the pack's filler-stripped pixel body.
    pub image_offset: u32,
    /// `+0x34`, into the same body. **Meaningless unless `palette_size != 0`.**
    pub palette_offset: u32,
    /// `+0x38`: the whole stored mip chain, not the top mip.
    pub image_size: u32,
    /// `+0x3C`: 1024 for a palettised texture, 0 otherwise. Checked, not trusted.
    pub palette_size: u32,
    /// `+0x40`: the top mip alone. Equals `level_size(w, h, comp)` in 5,087 of 5,087.
    pub top_mip_size: u32,
    /// `+0x44`. A power of two, 16..=1024.
    pub width: u16,
    /// `+0x46`. Likewise — and 2,527 of the city's 5,087 textures are **not** square.
    pub height: u16,
    /// `+0x4A`: the same numeric tag [`crate::texture::decode::fmt`] names.
    pub compression: u8,
    /// `+0x4E`: levels in the stored chain, 2..=7. Never a full chain down to 1×1, which is why
    /// [`Self::image_size`] cannot be re-derived from the dimensions alone.
    pub mip_levels: u8,
}

impl TrackTexture {
    /// Whether [`Self::name`] is whole rather than a 23-character prefix.
    ///
    /// The stored key is of the *whole* name, so hashing what survived and comparing is a proof
    /// rather than a guess — the same trick [`crate::types::NfsTexture::name_is_whole`] uses. True
    /// for 4,635 of the city's 5,087, and every one of the 452 failures is exactly 23 characters.
    #[must_use]
    pub fn name_is_whole(&self) -> bool {
        !self.name.is_empty() && crate::hash::string_hash(&self.name) == self.key.0
    }

    /// Whether the pixels are palette indices rather than colours.
    #[must_use]
    pub fn is_palettised(&self) -> bool {
        is_palettised(self.compression)
    }

    /// What decoding this one texture allocates.
    ///
    /// The budget is the caller's, the stance [`crate::world::manifest`] takes for geometry: the
    /// city is 5,087 textures and hundreds of megabytes of RGBA8, and one pack in `STREAML4RA`
    /// holds 1,032 records on its own.
    #[must_use]
    pub fn rgba_bytes(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height) * 4
    }
}

/// One `0xB3300000`: its record table and its pixel pool, borrowed from the bundle.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackPack<'a> {
    /// Every record, in file order — which is also ascending [`TrackTexture::key`] order in 502 of
    /// 502 packs, with no pack holding a duplicate.
    pub textures: Vec<TrackTexture>,
    /// The `0x33320002` payload **with its leading `0x11` filler removed**. Every image and
    /// palette offset is relative to byte 0 of this.
    pub pixels: &'a [u8],
    /// How much filler was stripped: 120 in 502 of 502 packs. Reported the way
    /// [`crate::world::WorldObjectInfo::filler`] is, so a caller can see it rather than trust that
    /// it was handled.
    pub filler: usize,
}

/// Every texture pack a bundle carries, in file order.
///
/// The manifest half: it reads the record tables and touches no pixel byte, exactly as
/// [`crate::texture::Tpk::directory`] is the cheap half for a car. A bundle with no packs is
/// `Ok(vec![])` rather than an error — most files are not texture packs.
///
/// # Errors
///
/// Propagates the walk. A `STREAM*.BUN` is sector-padded, so the walk resyncs past the gaps and an
/// early stop is not the end of the file. A pack that declares records but carries no
/// `0x33320002` is [`NfsError::CorruptArchive`] — 0 of 502 in the install.
pub fn packs(bundle: &[u8]) -> NfsResult<Vec<TrackPack<'_>>> {
    // Collected as ranges, not slices: `walk_positions` hands a leaf's payload with the callback's
    // lifetime, and the offset is what lets the pool be re-borrowed from `bundle` afterwards with
    // the lifetime a caller can keep. Recording the position is exactly why that variant exists.
    struct Parts {
        textures: Vec<TrackTexture>,
        pixels: Option<(usize, usize)>,
        filler: usize,
    }
    let mut parts: Vec<Parts> = Vec::new();

    walk_positions(
        bundle,
        WalkOptions::default(),
        |_, _| Ok(Visit::Descend),
        |_, header, at, data| {
            match header.id {
                // A record table starts a pack. Sound for the same reason a `0x00134011` starts an
                // object in `world::manifest`: it precedes the pack's pixel chunk in 502 of 502,
                // and packs never nest.
                TRACK_RECORDS => {
                    parts.push(Parts { textures: records(data), pixels: None, filler: 0 });
                }
                TRACK_PIXELS => {
                    if let Some(p) = parts.last_mut() {
                        let body = skip_leading_filler(data);
                        p.filler = data.len() - body.len();
                        p.pixels = Some((at + p.filler, at + data.len()));
                    }
                }
                _ => {}
            }
            Ok(())
        },
    )?;

    parts
        .into_iter()
        .map(|p| {
            let (from, to) = p.pixels.ok_or(NfsError::CorruptArchive {
                detail: "track TPK pack has records but no pixel chunk",
            })?;
            let pixels = bundle.get(from..to).ok_or(NfsError::CorruptArchive {
                detail: "track TPK pixel chunk lies outside the bundle",
            })?;
            Ok(TrackPack { textures: p.textures, pixels, filler: p.filler })
        })
        .collect()
}

/// Parse one `0x33310004` payload on its own.
///
/// Separate from [`packs`] because the same record layout is used outside `TRACKS/` —
/// `GLOBAL/GLOBALB.BUN`, `GLOBAL/InGameCommon.bun` and the `InGame*.bun` all carry tables of it.
/// A trailing partial record is ignored, the way the car pack's descriptor table ignores one.
#[must_use]
pub fn records(payload: &[u8]) -> Vec<TrackTexture> {
    let count = payload.len() / RECORD_STRIDE;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        // The count came from the buffer's own length, so every read below is in bounds; the
        // reader is still used for the bounds check rather than indexing on that reasoning.
        let rec = &payload[i * RECORD_STRIDE..(i + 1) * RECORD_STRIDE];
        let Ok(tex) = record(rec) else { continue };
        out.push(tex);
    }
    out
}

fn record(rec: &[u8]) -> NfsResult<TrackTexture> {
    let field = &rec[REC_NAME..REC_NAME + REC_NAME_LEN];
    let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    let name = String::from_utf8_lossy(&field[..end]).into_owned();

    let u32_at = |at: usize| -> NfsResult<u32> { ByteReader::at(rec, at)?.u32_le() };
    let u16_at = |at: usize| -> NfsResult<u16> { ByteReader::at(rec, at)?.u16_le() };

    Ok(TrackTexture {
        name,
        key: AssetHash(u32_at(REC_KEY)?),
        image_offset: u32_at(REC_IMAGE_OFFSET)?,
        palette_offset: u32_at(REC_PALETTE_OFFSET)?,
        image_size: u32_at(REC_IMAGE_SIZE)?,
        palette_size: u32_at(REC_PALETTE_SIZE)?,
        top_mip_size: u32_at(REC_TOP_MIP_SIZE)?,
        width: u16_at(REC_WIDTH)?,
        height: u16_at(REC_HEIGHT)?,
        compression: rec[REC_COMPRESSION],
        mip_levels: rec[REC_MIP_LEVELS],
    })
}

impl<'a> TrackPack<'a> {
    /// The whole stored mip chain for one record.
    ///
    /// # Errors
    ///
    /// [`NfsError::CorruptArchive`] on an offset that overflows, [`NfsError::BufferSizeMismatch`]
    /// if the extent runs off the pool.
    pub fn image(&self, tex: &TrackTexture) -> NfsResult<&'a [u8]> {
        self.slice(tex.image_offset, tex.image_size as usize, "image")
    }

    /// The top mip alone, cross-checked against the dimensions.
    ///
    /// `+0x40` and `level_size(width, height, compression)` agree in 5,087 of 5,087 records, so a
    /// disagreement means the record was misread rather than that the file is unusual — and that
    /// is worth an error rather than a silently short read.
    ///
    /// # Errors
    ///
    /// [`NfsError::NotImplemented`] for a format `level_size` does not know,
    /// [`NfsError::CorruptArchive`] if the two statements of the top mip's size disagree, and the
    /// bounds errors [`Self::image`] returns.
    pub fn top_mip(&self, tex: &TrackTexture) -> NfsResult<&'a [u8]> {
        let (w, h) = self.dimensions(tex)?;
        let top = level_size(w, h, tex.compression)
            .ok_or(NfsError::NotImplemented { feature: "track TPK pixel format" })?;
        if top != tex.top_mip_size as usize {
            return Err(NfsError::CorruptArchive {
                detail: "track TPK top-mip size disagrees with its own dimensions",
            });
        }
        self.slice(tex.image_offset, top, "top mip")
    }

    /// This record's palette, or `None` when it has none.
    ///
    /// `palette_offset` is read **only** when `palette_size != 0`: on the 4,996 records without a
    /// palette the field holds a stale cursor value, so trusting it unconditionally would hand a
    /// neighbouring mip over as colours.
    ///
    /// # Errors
    ///
    /// [`NfsError::NotImplemented`] for a palette size this arm has not been shown, and the bounds
    /// errors [`Self::image`] returns.
    pub fn palette(&self, tex: &TrackTexture) -> NfsResult<Option<&'a [u8; PALETTE_BYTES]>> {
        if tex.palette_size == 0 {
            return Ok(None);
        }
        if tex.palette_size as usize != PALETTE_BYTES {
            return Err(NfsError::NotImplemented { feature: "track TPK palette size" });
        }
        let bytes = self.slice(tex.palette_offset, PALETTE_BYTES, "palette")?;
        // The fixed-size array is load-bearing rather than decorative: it is what makes the
        // palette lookup in `unpack_palettised` safe by type instead of by caller discipline.
        bytes
            .try_into()
            .map(Some)
            .map_err(|_| NfsError::BufferSizeMismatch { detail: "track TPK palette length" })
    }

    /// Decode one record's **top mip** into RGBA8.
    ///
    /// # Errors
    ///
    /// Every error here means "skip this texture", not "the bundle is broken": dimensions of zero
    /// or past [`crate::texture::decode::MAX_DIM`], a pixel format the crate does not decode, or an
    /// extent that runs off the pool.
    pub fn decode(&self, tex: &TrackTexture) -> NfsResult<NfsTexture> {
        let (w, h) = self.dimensions(tex)?;
        let pixels = self.top_mip(tex)?;
        let source_format = named_format(tex.compression)
            .ok_or(NfsError::NotImplemented { feature: "track TPK pixel format" })?;

        let rgba = match tex.compression {
            fmt::DXT1 => dxt::decode_dxt1(pixels, w, h),
            fmt::DXT3 => dxt::decode_dxt3(pixels, w, h),
            fmt::DXT5 => dxt::decode_dxt5(pixels, w, h),
            fmt::RGBA8888 => unpack_bgra(pixels, w, h),
            c if is_palettised(c) => {
                let palette = self.palette(tex)?.ok_or(NfsError::CorruptArchive {
                    detail: "track TPK palettised texture has no palette",
                })?;
                unpack_palettised(pixels, palette, w, h)
            }
            _ => return Err(NfsError::NotImplemented { feature: "track TPK pixel format" }),
        };

        Ok(NfsTexture {
            name: tex.name.clone(),
            hash: tex.key,
            width: w as u32,
            height: h as u32,
            rgba,
            source_format,
            format: PixelFormat::Rgba8,
        })
    }

    /// What decoding every record in this pack would allocate.
    #[must_use]
    pub fn rgba_bytes(&self) -> u64 {
        self.textures.iter().map(TrackTexture::rgba_bytes).sum()
    }

    /// The record for `key`, by binary search — records are strictly ascending by key in 502 of
    /// 502 packs and no pack holds a duplicate.
    #[must_use]
    pub fn get(&self, key: AssetHash) -> Option<&TrackTexture> {
        self.textures.binary_search_by_key(&key.0, |t| t.key.0).ok().map(|i| &self.textures[i])
    }

    /// Dimensions, refused before anything is allocated against them.
    fn dimensions(&self, tex: &TrackTexture) -> NfsResult<(usize, usize)> {
        let (w, h) = (tex.width as usize, tex.height as usize);
        if w == 0 || h == 0 || w > MAX_DIM || h > MAX_DIM {
            return Err(NfsError::CorruptArchive {
                detail: "track TPK texture dimensions out of range",
            });
        }
        Ok((w, h))
    }

    fn slice(&self, offset: u32, len: usize, what: &'static str) -> NfsResult<&'a [u8]> {
        let at = offset as usize;
        let end = at.checked_add(len).ok_or(NfsError::CorruptArchive {
            detail: "track TPK extent overflows",
        })?;
        self.pixels.get(at..end).ok_or(match what {
            "palette" => NfsError::BufferSizeMismatch { detail: "track TPK palette past the pool" },
            "top mip" => NfsError::BufferSizeMismatch { detail: "track TPK top mip past the pool" },
            _ => NfsError::BufferSizeMismatch { detail: "track TPK image past the pool" },
        })
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    /// Build a `0x7C` record. `image` and `palette` are pool offsets.
    #[allow(clippy::too_many_arguments)]
    fn rec(
        name: &str,
        w: u16,
        h: u16,
        comp: u8,
        image: u32,
        image_size: u32,
        top: u32,
        palette: u32,
        palette_size: u32,
    ) -> Vec<u8> {
        let mut r = vec![0u8; RECORD_STRIDE];
        let fits = name.len().min(REC_NAME_LEN - 1);
        r[REC_NAME..REC_NAME + fits].copy_from_slice(&name.as_bytes()[..fits]);
        r[REC_KEY..REC_KEY + 4].copy_from_slice(&crate::hash::string_hash(name).to_le_bytes());
        r[REC_IMAGE_OFFSET..REC_IMAGE_OFFSET + 4].copy_from_slice(&image.to_le_bytes());
        r[REC_PALETTE_OFFSET..REC_PALETTE_OFFSET + 4].copy_from_slice(&palette.to_le_bytes());
        r[REC_IMAGE_SIZE..REC_IMAGE_SIZE + 4].copy_from_slice(&image_size.to_le_bytes());
        r[REC_PALETTE_SIZE..REC_PALETTE_SIZE + 4].copy_from_slice(&palette_size.to_le_bytes());
        r[REC_TOP_MIP_SIZE..REC_TOP_MIP_SIZE + 4].copy_from_slice(&top.to_le_bytes());
        r[REC_WIDTH..REC_WIDTH + 2].copy_from_slice(&w.to_le_bytes());
        r[REC_HEIGHT..REC_HEIGHT + 2].copy_from_slice(&h.to_le_bytes());
        r[REC_COMPRESSION] = comp;
        r[REC_MIP_LEVELS] = 4;
        r
    }

    fn chunk(id: u32, payload: &[u8]) -> Vec<u8> {
        let mut v = id.to_le_bytes().to_vec();
        v.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        v.extend_from_slice(payload);
        v
    }

    /// A pack whose pool carries `filler` bytes of `0x11` in front of `pool`.
    fn pack(table: &[u8], pool: &[u8], filler: usize) -> Vec<u8> {
        let mut body = chunk(0xB331_0000, &chunk(TRACK_RECORDS, table));
        let mut px = vec![0x11u8; filler];
        px.extend_from_slice(pool);
        body.extend_from_slice(&chunk(0xB332_0000, &chunk(TRACK_PIXELS, &px)));
        chunk(TRACK_PACK, &body)
    }

    /// The four DXT1 bytes of a 4×4 block, both endpoints black.
    const DXT1_BLOCK: [u8; 8] = [0, 0, 0, 0, 0, 0, 0, 0];

    #[test]
    fn a_record_states_its_own_format_and_size() {
        let table = rec("TRN_GRASSC", 4, 4, fmt::DXT1, 0, 8, 8, 0, 0);
        let got = records(&table);
        assert_eq!(got.len(), 1);
        let t = &got[0];
        assert_eq!(t.name, "TRN_GRASSC");
        assert!(t.name_is_whole());
        assert_eq!((t.width, t.height), (4, 4));
        assert_eq!(t.compression, fmt::DXT1);
        assert!(!t.is_palettised());
        assert_eq!(t.rgba_bytes(), 4 * 4 * 4);
    }

    /// The pool base is the payload past its filler. Every width the format uses must land on the
    /// same pixels, because reading from the raw payload start would take filler as mip 0.
    #[test]
    fn the_pool_base_is_past_the_filler() {
        let table = rec("OBJ_PYLON", 4, 4, fmt::DXT1, 0, 8, 8, 0, 0);
        for filler in [0usize, 4, 120] {
            let bundle = pack(&table, &DXT1_BLOCK, filler);
            let p = packs(&bundle).unwrap();
            assert_eq!(p.len(), 1, "{filler}: one pack");
            assert_eq!(p[0].filler, filler, "{filler}: filler reported");
            assert_eq!(p[0].pixels, &DXT1_BLOCK, "{filler}: pool starts at the pixels");
            assert_eq!(p[0].image(&p[0].textures[0]).unwrap(), &DXT1_BLOCK);
        }
    }

    /// A palette sitting *before* the image is the city's normal case and must not be refused.
    #[test]
    fn a_palette_ahead_of_the_image_is_read() {
        let mut pool = vec![0u8; PALETTE_BYTES];
        pool[0..4].copy_from_slice(&[10, 20, 30, 255]); // B,G,R,A for index 0
        pool.extend_from_slice(&[0u8; 16]); // a 4×4 P8 image, every texel index 0
        let table = rec("RDP_PARKING", 4, 4, fmt::P8, PALETTE_BYTES as u32, 16, 16, 0, 1024);
        let bundle = pack(&table, &pool, 120);
        let p = packs(&bundle).unwrap();
        let t = &p[0].textures[0];

        assert!(t.is_palettised());
        assert!(p[0].palette(t).unwrap().is_some(), "palette before image must be accepted");
        let img = p[0].decode(t).unwrap();
        assert_eq!((img.width, img.height), (4, 4));
        // B,G,R,A on disk comes back R,G,B,A.
        assert_eq!(&img.rgba[..4], &[30, 20, 10, 255]);
    }

    /// `palette_offset` is stale on a record with no palette, so it must not be read.
    #[test]
    fn a_stale_palette_offset_is_ignored_when_there_is_no_palette() {
        // Offset 0x400 with a 16-byte pool: reading it would run off the end.
        let table = rec("TRN_ROADA", 4, 4, fmt::DXT1, 0, 8, 8, 0x400, 0);
        let bundle = pack(&table, &DXT1_BLOCK, 120);
        let p = packs(&bundle).unwrap();
        let t = &p[0].textures[0];
        assert_eq!(p[0].palette(t).unwrap(), None, "no palette means the offset is not read");
        assert!(p[0].decode(t).is_ok(), "and decoding still works");
    }

    /// The two statements of the top mip's size agree in every real record; a disagreement means
    /// the record was misread and is worth refusing rather than reading short.
    #[test]
    fn a_top_mip_size_that_contradicts_the_dimensions_is_refused() {
        let table = rec("OBJ_BLKPLAS", 4, 4, fmt::DXT1, 0, 8, 999, 0, 0);
        let bundle = pack(&table, &DXT1_BLOCK, 120);
        let p = packs(&bundle).unwrap();
        assert!(matches!(
            p[0].top_mip(&p[0].textures[0]),
            Err(NfsError::CorruptArchive { .. })
        ));
    }

    /// A tag the crate cannot size is refused by name rather than guessed at from the size — the
    /// whole reason the stated format is read instead of inferred.
    #[test]
    fn an_unknown_format_tag_is_refused_by_name() {
        let table = rec("MYSTERY", 4, 4, 0x99, 0, 8, 8, 0, 0);
        let bundle = pack(&table, &DXT1_BLOCK, 120);
        let p = packs(&bundle).unwrap();
        assert!(matches!(
            p[0].decode(&p[0].textures[0]),
            Err(NfsError::NotImplemented { feature: "track TPK pixel format" })
        ));
    }

    #[test]
    fn extents_past_the_pool_are_refused_rather_than_read() {
        let table = rec("TRN_GRASSC", 4, 4, fmt::DXT1, 4, 8, 8, 0, 0);
        let bundle = pack(&table, &DXT1_BLOCK, 120); // 8-byte pool, image wants 4..12
        let p = packs(&bundle).unwrap();
        assert!(matches!(
            p[0].image(&p[0].textures[0]),
            Err(NfsError::BufferSizeMismatch { .. })
        ));
        // And an offset that would overflow is caught before the slice.
        let table = rec("TRN_GRASSC", 4, 4, fmt::DXT1, u32::MAX, 8, 8, 0, 0);
        let bundle = pack(&table, &DXT1_BLOCK, 120);
        let p = packs(&bundle).unwrap();
        assert!(p[0].image(&p[0].textures[0]).is_err());
    }

    /// A name too long for the field is reported as truncated rather than passed off as one.
    #[test]
    fn a_truncated_name_fails_its_own_key() {
        let long = "RDP_AIRPORT_ROADPATCH_LONG_A";
        assert!(long.len() > REC_NAME_LEN - 1);
        let table = rec(long, 4, 4, fmt::DXT1, 0, 8, 8, 0, 0);
        let t = &records(&table)[0];
        assert!(!t.name.is_empty());
        assert!(!t.name_is_whole());
    }

    #[test]
    fn a_trailing_partial_record_is_ignored() {
        let mut table = rec("TRN_GRASSC", 4, 4, fmt::DXT1, 0, 8, 8, 0, 0);
        table.extend_from_slice(&[0u8; 17]);
        assert_eq!(records(&table).len(), 1);
        assert_eq!(records(&[]).len(), 0);
        assert_eq!(records(&[0u8; 5]).len(), 0);
    }

    #[test]
    fn records_are_found_by_key_and_a_bundle_without_packs_is_empty() {
        let mut table = Vec::new();
        // binary_search needs ascending keys, which is what the file gives; sort to build one.
        let mut built: Vec<(u32, Vec<u8>)> = ["A_ONE", "B_TWO", "C_THREE"]
            .iter()
            .map(|n| (crate::hash::string_hash(n), rec(n, 4, 4, fmt::DXT1, 0, 8, 8, 0, 0)))
            .collect();
        built.sort_by_key(|(k, _)| *k);
        for (_, r) in &built {
            table.extend_from_slice(r);
        }
        let bundle = pack(&table, &DXT1_BLOCK, 120);
        let p = packs(&bundle).unwrap();
        assert_eq!(p[0].textures.len(), 3);
        let key = crate::hash::asset_hash("B_TWO");
        assert_eq!(p[0].get(key).map(|t| t.name.as_str()), Some("B_TWO"));
        assert_eq!(p[0].get(AssetHash(0)), None);

        assert!(packs(&[]).unwrap().is_empty(), "a file with no pack is not an error");
    }

    #[test]
    fn a_pack_with_records_but_no_pixels_is_refused() {
        let table = rec("TRN_GRASSC", 4, 4, fmt::DXT1, 0, 8, 8, 0, 0);
        let bundle = chunk(TRACK_PACK, &chunk(0xB331_0000, &chunk(TRACK_RECORDS, &table)));
        assert!(matches!(packs(&bundle), Err(NfsError::CorruptArchive { .. })));
    }
}
