//! Parse NFSU2 `TEXTURES.BIN` (TPK) into per-texture RGBA8 images.
//!
//! The format was reverse-engineered against real cars, cross-checked with the community
//! `xan1242/xnfstpktool` and `NFSTools/GlobalLib` sources.
//!
//! # Layout
//!
//! ```text
//! 0xB3300000  TPK root
//!   0xB3310000  directory
//!     0x33310001  info (name + source .tpk path)
//!     0x33310002  N × (u32 hash, u32 = 0)
//!     0x33310003  ← DESCRIPTORS: N × 24-byte record (below)
//!   0xB3320000  data
//!     0x33320002  the compressed texture blobs
//! ```
//!
//! **Each texture is independent.** The 24-byte descriptor (`0x33310003`, LE `u32`) is:
//!
//! | off | field | role |
//! |-----|-------|------|
//! | 0x00 | `hash`               | asset key |
//! | 0x04 | `abs_offset`         | **whole-file** byte offset of this texture's compressed blob |
//! | 0x08 | `size`               | compressed byte length at `abs_offset` |
//! | 0x0C | `out_size`           | decompressed size of the blob |
//! | 0x10 | `header_from_end`    | distance from the decompressed end back to the header (const `0x100`) |
//! | 0x14 | `unk`                | ignored |
//!
//! To decode one texture: read `file[abs_offset .. abs_offset + size]`, decompress it (JDLZ
//! or HUFF, by magic) into an `out_size`-byte buffer, then read an embedded `OldTextureInfo`
//! header near its tail for the dimensions and pixel format. Pixels always start at buffer
//! offset 0. The header sits at `P = out_size − header_from_end + 0x64 + 0x24`, where the
//! `u32` at `P` is the texture's own hash (a self-check); from `P`: `Width = u16@P+32`,
//! `Height = u16@P+34`, `ImageCompressionType = u8@P+38`. The image is the *top mip* only,
//! decoded by [`dxt`] (DXT1/3/5), unpacked directly (RGBA), or looked up through a palette.
//!
//! The palettised tags need three more fields of that header, all `u32`:
//!
//! | off | field | role |
//! |-----|-------|------|
//! | P+0x0C | `ImagePlacement`   | this image's offset into a notional concatenation of the pack |
//! | P+0x10 | `PalettePlacement` | its palette's offset into the same — so **the difference is the palette's offset inside this blob** |
//! | P+0x14 | `ImageSize`        | the whole mip chain; the palette never starts before it |
//! | P+0x18 | `PaletteSize`      | always 1024, and checked rather than trusted |
//!
//! The palette is 256 entries of four bytes stored **B,G,R,A**, and the pixels are one index byte
//! each. Do not compute the palette's position from `ImageSize`: measured over an install it sits
//! 64 bytes past it 51,844 times and exactly at it 27 times, so there is no constant to add.
//!
//! A pack mixes codecs, and the blob's own magic says which: [`crate::compression::jdlz`] and
//! [`crate::compression::huff`] both decompress, so **no texture is skipped for its codec** any
//! more. A golden test reads all 73 of the 240SX's — 44 JDLZ, 29 HUFF.
//!
//! Pixel format used to be the other limit, and is not any more. Measured over one install: 78
//! packs, 54,885 declared textures, **54,873 decoded**. The palettised tags (`ImageCompressionType`
//! `0x08`: 25,960, `0x80`: 24,071, `0x81`: 1,840) were 51,871 of them and read as nothing; they are
//! one layout — a 1024-byte palette and one index byte per pixel — and [`decode`] now unpacks all
//! three, so a `CARS/*/VINYLS.BIN` gives its 1,786 images rather than an empty pack. Every texture
//! in a car's `TEXTURES.BIN` still decodes: 2,123 of 2,123 across 30 packs.
//!
//! The 12 that remain are not a format at all: their embedded header does not hold the hash the
//! descriptor names, so the header formula does not apply to them and they are skipped by the
//! self-check rather than decoded into noise. They are 6 each in `IMPREZA` and `LANCER`, whose
//! `VINYLS.BIN` are ~50 KB stubs of six descriptors where every other car's is 14 MB of 1,786 —
//! i.e. the whole of both files, and nothing at all of any real pack.
//!
//! A caller should know what that costs. A car's pack is 73 images and 8.7 MB of RGBA8; a vinyls
//! pack is 1,786 images, every one 512², and **1.87 GB** — so [`Tpk::parse`], which decodes the lot,
//! is no longer a reasonable thing to call on an arbitrary file. [`Tpk::directory`] with
//! [`Tpk::decode_one`] is there for exactly that, and is what PryHUB uses to hold a pack to a
//! budget.
//!
//! Both codecs a pack uses now encode as well as decode, and [`write`] keeps the one a blob arrived
//! in — so a HUFF texture written back is still HUFF. What is left is that re-compressing anything
//! rarely reproduces a stream to the byte, which is why an in-place write can still be refused for
//! size and [`write::relocate`] exists.
//!
//! The module is split by layer: [`directory`] reads the descriptor table (and the DebugNames
//! beside it), [`decode`] turns one descriptor's blob into RGBA8, and [`dxt`] holds the S3TC
//! block decoders. This file is only the container: find the directory, decode what it lists.

pub mod dxt;
pub mod encode;
pub mod write;
mod decode;
mod directory;

pub use directory::TpkEntry;
pub use encode::replace_pixels;
pub use write::{blob_of, relocate, replace_blob};

use crate::chunk::{ChunkNode, WalkOptions};
use directory::DESCRIPTORS;
use crate::error::{NfsError, NfsResult};
use crate::types::{AssetHash, NfsTexture};
use std::collections::HashMap;

/// A parsed TPK: the raw per-texture descriptors plus every texture we could decode to RGBA8.
///
/// The two are counted separately on purpose. [`entries`](Tpk::entries) is what the file *declares*;
/// [`textures`](Tpk::textures) is what came back as pixels. A texture that fails to decode is missing
/// from the second and still present in the first, so a caller can say how many rather than showing a
/// shorter grid and calling it the whole pack.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct Tpk {
    /// Every texture descriptor, in file order.
    pub entries: Vec<TpkEntry>,
    /// Decoded RGBA8 textures, keyed by hash.
    pub textures: HashMap<AssetHash, NfsTexture>,
}

impl Tpk {
    /// Parse a (raw, on-disk) `TEXTURES.BIN` buffer, decoding every texture whose codec is
    /// supported. Returns an error only if the descriptor chunk is absent or malformed; an
    /// individual texture that fails to decode is skipped, not fatal.
    ///
    /// **It decodes the whole pack, and a pack is no longer necessarily small.** A car's is 73
    /// images and 8.7 MB; a `VINYLS.BIN` is 1,786 images of 512² and 1.87 GB, which this will
    /// allocate without asking. Given an arbitrary file, prefer [`Tpk::directory`] and
    /// [`Tpk::decode_one`], which let the caller stop.
    pub fn parse(bytes: &[u8]) -> NfsResult<Tpk> {
        // Only the directory (near the file start) is needed here — the pixel blobs are read
        // later by absolute offset, never by walking. Tool-compiled TPKs (e.g. nfsu360's
        // Texture Compiler) pack raw compressed blocks after the directory with no wrapping
        // chunk, so a strict full-file walk misreads them and overruns; walk tolerantly so the
        // clean directory still yields the descriptor table.
        let entries = Self::directory(bytes)?;
        let mut textures = HashMap::new();
        for e in &entries {
            if let Ok(tex) = Self::decode_one(bytes, e) {
                textures.insert(e.hash, tex);
            }
        }
        Ok(Self::from_decoded(entries, textures))
    }

    /// The descriptor table alone, without decoding anything.
    ///
    /// The pair with [`Tpk::decode_one`] and [`Tpk::from_decoded`]: every texture is independent, so
    /// a caller with threads to spare can decode them in parallel, and one without the memory for a
    /// whole pack can decode part of it. This crate spawns no threads and imposes no budget — both
    /// are decisions that belong to whoever called it.
    ///
    /// The size of that decision has changed. It was 8 MB and 20 ms, which is a car's pack; since
    /// the palettised formats decode it can be 1.87 GB and 2.16 s, which is a car's *vinyls* pack.
    /// This function is the cheap half either way — it reads the descriptor table and touches no
    /// blob.
    ///
    /// # Errors
    /// When the descriptor chunk is missing or unreadable.
    pub fn directory(bytes: &[u8]) -> NfsResult<Vec<TpkEntry>> {
        let opts = WalkOptions { stop_on_overrun: true, ..Default::default() };
        let roots = ChunkNode::parse_with(bytes, opts)?;
        let desc = directory::find_leaf(&roots, DESCRIPTORS, bytes)
            .ok_or(NfsError::CorruptArchive { detail: "TPK missing descriptor chunk 0x33310003" })?;
        Ok(directory::parse_descriptors(desc))
    }

    /// Decode one descriptor's texture.
    ///
    /// # Errors
    /// A pixel format this crate does not decode, an unreadable blob, a malformed embedded header,
    /// or dimensions out of range — all of which mean "skip this texture", not "the file is broken".
    /// Over a whole install this now happens 12 times in 54,885, and none of them is a format: it is
    /// the embedded header failing its own hash self-check.
    pub fn decode_one(bytes: &[u8], entry: &TpkEntry) -> NfsResult<NfsTexture> {
        decode::decode_texture(bytes, entry)
    }

    /// One descriptor's `DebugName`, without decoding its pixels.
    ///
    /// A texture's readable name lives inside its *compressed* blob, so there is no reading it out
    /// of the directory — but it costs the decompression only, where [`Tpk::decode_one`] then walks
    /// every pixel and hands back `width * height * 4` bytes. For a caller that wants what a pack
    /// is *called* rather than what it looks like — a hash dictionary, a listing, a search — that
    /// is the whole difference between a transient buffer per texture and 1.87 GB of RGBA8 for a
    /// `VINYLS.BIN`.
    ///
    /// # Errors
    /// The same ones [`Tpk::decode_one`] raises before it reaches the pixels: an unreadable blob, or
    /// an embedded header that fails its own hash self-check. A name that comes back here is a name
    /// that texture would have decoded under.
    pub fn name_of(bytes: &[u8], entry: &TpkEntry) -> NfsResult<String> {
        decode::texture_name_only(bytes, entry)
    }

    /// Assemble a pack from a directory and textures decoded by the caller.
    #[must_use]
    pub fn from_decoded(
        entries: Vec<TpkEntry>,
        textures: HashMap<AssetHash, NfsTexture>,
    ) -> Self {
        Self { entries, textures }
    }

    /// Look up a decoded texture by asset hash.
    #[must_use]
    pub fn texture(&self, hash: AssetHash) -> Option<&NfsTexture> {
        self.textures.get(&hash)
    }

    /// Look up a texture descriptor by asset hash (present even for a texture that did not decode).
    #[must_use]
    pub fn entry(&self, hash: AssetHash) -> Option<&TpkEntry> {
        self.entries.iter().find(|e| e.hash == hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a chunk: 8-byte header (id LE, size LE) then `payload`.
    fn chunk(id: u32, payload: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&id.to_le_bytes());
        v.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        v.extend_from_slice(payload);
        v
    }

    #[test]
    fn missing_descriptor_chunk_is_an_error() {
        // A pixel-data chunk but no descriptor table.
        let file = chunk(0x3332_0002, &[0u8; 8]);
        assert!(matches!(Tpk::parse(&file), Err(NfsError::CorruptArchive { .. })));
    }
}
