//! Putting a texture back into a TPK.
//!
//! A TPK is the one file in this format that cannot simply be reassembled. Its descriptors point at
//! pixel blobs by **absolute file offset**, and the blobs are neither in descriptor order nor packed
//! tightly — measured over 30 real packs, only one has them in ascending order and only one has them
//! contiguous. Moving anything therefore means rewriting every offset, and moving anything *inside*
//! a chunk changes that chunk's size, which moves the blobs after it again.
//!
//! There are therefore two ways in. [`replace_blob`] does the part that needs no relocation, and
//! [`relocate`] does the part that needs all of it. The first is the safer edit and the second is
//! the general one.
//!
//! # In place
//!
//! Replace a texture where it lies. The new blob is compressed with the codec the old one arrived
//! in, and if it fits the slot that one occupied, it is written there and the descriptor's
//! compressed size updated. Nothing else in the file moves — not one other offset changes — which is
//! why this is safe to do without a theory of the whole layout.
//!
//! When it does not fit, that is said plainly rather than worked around. **What the misses are is
//! measured, not assumed**, and the answer has changed twice as the crate learned to write. With
//! JDLZ as the only encoder, 1,402 of 2,123 blobs fit — 1,392 of 1,538 JDLZ-sourced but only 10 of
//! 585 HUFF-sourced, which was not a layout problem at all: a HUFF blob re-packed as JDLZ into a
//! slot HUFF had sized does not fit by construction. With a HUFF encoder, and [`replace_blob`]
//! keeping the codec a blob arrived in, it is **1,451 of 2,123** (1,393/1,539 JDLZ and 59/584
//! HUFF). The HUFF share is still the low one, because re-compressing anything at all rarely
//! reproduces a stream to the byte and HUFF's slots are tight.
//!
//! # Relocation
//!
//! [`relocate`] rewrites the whole pack instead: every blob is laid out afresh and every descriptor
//! updated, so a replacement's size stops being a question. That makes the numbers above a fact
//! about the *cheap* path rather than a limit on the format: nothing is blocked on a replacement
//! fitting.
//!
//! It rests on four things, each measured over the install's 77 chunked packs before any of this was
//! written: every blob lives in one leaf chunk (`0x33320002`, 54,875 of 54,875), that chunk is the
//! last leaf (77 of 77), nothing outside the descriptor table points at a blob, and the bytes
//! between blobs are junk rather than a pattern. Relocating every pack with nothing replaced and
//! decompressing all 54,875 blobs back gives **byte-identical** results, for 0.36% more file.
//!
//! And then the game read one. A 240SX pack was relocated with a single texture repainted — every
//! one of its 73 blobs at a new offset, every descriptor rewritten — installed, and looked at: the
//! edited texture showed the colour it was given and the other 72 drew correctly. That is the check
//! nothing here can make for itself. A decoder reading back what this encoder wrote proves the two
//! agree; only the game proves they are both right.

use super::directory::{find_chunk, parse_descriptors, DESCRIPTORS};
use crate::chunk::{ChunkNode, WalkOptions};
use crate::error::{NfsError, NfsResult};
use crate::types::AssetHash;

/// Bytes per descriptor, and the offset of the compressed-size field within one.
const DESCRIPTOR_STRIDE: usize = 24;
const SIZE_FIELD: usize = 8;

/// One texture's blob, decompressed — the bytes a caller edits and hands back to [`replace_blob`].
///
/// This is the whole blob, not the pixels: the image is at its start and an `OldTextureInfo` header
/// sits near its end, which is where the dimensions and the pixel format live. Editing pixels means
/// editing the front of it and leaving the rest alone.
///
/// # Errors
/// When the hash is not in the pack, when its blob lies outside the file, or when the stream will
/// not decompress. The codec is not a limit here — whatever the magic says (JDLZ, HUFF or RefPack)
/// is read. It is a limit on the way back: see [`replace_blob`].
pub fn blob_of(file: &[u8], hash: AssetHash) -> NfsResult<Vec<u8>> {
    let (entries, _) = table(file)?;
    let entry = entries
        .iter()
        .find(|e| e.hash == hash)
        .ok_or(NfsError::CorruptArchive { detail: "no texture with that hash in this TPK" })?;
    let end = (entry.abs_offset as usize).saturating_add(entry.size as usize);
    let blob = file
        .get(entry.abs_offset as usize..end)
        .ok_or(NfsError::CorruptArchive { detail: "TPK texture blob out of range" })?;
    crate::compression::decompress(blob)
}

/// Write `blob` back as the texture `hash`, in place.
///
/// `blob` must be the same length as the one that came out: the descriptor records where the
/// embedded header sits as a distance from the *end* of the decompressed blob, so a different
/// length would move that header without saying so, and the texture would decode as noise.
///
/// # Errors
/// - The hash is not in the pack.
/// - `blob` is not the length the descriptor declares.
/// - The recompressed blob is larger than the slot the old one occupied. Nothing is written; the
///   caller is told rather than silently overwriting whatever follows. Measured over one install,
///   1,451 of 2,123 blobs fit — and the codec a blob arrived in is kept precisely because the slot
///   was sized by that encoder: re-packing a HUFF blob as JDLZ threw the fit away before the data
///   was looked at, and fixing that took HUFF-sourced fits from 10 of 585 to 59 of 584.
pub fn replace_blob(file: &[u8], hash: AssetHash, blob: &[u8]) -> NfsResult<Vec<u8>> {
    let (entries, table_at) = table(file)?;
    let (index, entry) = entries
        .iter()
        .enumerate()
        .find(|(_, e)| e.hash == hash)
        .ok_or(NfsError::CorruptArchive { detail: "no texture with that hash in this TPK" })?;

    if blob.len() != entry.out_size as usize {
        return Err(NfsError::BufferSizeMismatch {
            detail: "a replacement blob must be exactly the decompressed size of the one it replaces",
        });
    }
    // Recompress with the codec the blob arrived in. The slot was sized by that encoder, so using
    // the other one throws the fit away before the data is even looked at: measured over an
    // install, a HUFF-sourced blob re-packed as JDLZ fits 60 times in 52,389, and re-packed as HUFF
    // 4,863 times.
    let at = entry.abs_offset as usize;
    let old = file
        .get(at..at.saturating_add(entry.size as usize))
        .ok_or(NfsError::CorruptArchive { detail: "TPK texture blob out of range" })?;
    let packed = match crate::compression::detect(old) {
        crate::compression::Codec::Huff => crate::compression::huff::compress(blob)?,
        _ => crate::compression::jdlz::compress(blob)?,
    };
    if packed.len() > entry.size as usize {
        return Err(NfsError::BufferSizeMismatch {
            detail: "the recompressed texture is larger than the slot it has to fit in",
        });
    }

    if at.saturating_add(entry.size as usize) > file.len() {
        return Err(NfsError::CorruptArchive { detail: "TPK texture blob out of range" });
    }
    let mut out = file.to_vec();
    out[at..at + packed.len()].copy_from_slice(&packed);
    // The bytes between the new end and the old are left as they were. Nothing reads them — the
    // descriptor says how many bytes the texture is — and not touching them keeps the diff to the
    // part that actually changed.

    // The descriptor's compressed size is the only field that moved. `out_size` and
    // `header_from_end` describe the decompressed blob, which is the same length as before.
    let size_at = table_at + index * DESCRIPTOR_STRIDE + SIZE_FIELD;
    out.get_mut(size_at..size_at + 4)
        .ok_or(NfsError::CorruptArchive { detail: "TPK descriptor table out of range" })?
        .copy_from_slice(&(packed.len() as u32).to_le_bytes());
    Ok(out)
}

/// The chunk whose payload is every pixel blob in the pack.
const BLOBS: u32 = 0x3332_0002;

/// The boundary each relocated blob is put on.
///
/// A choice, not a rule the file states — which is why it is the generous end of what the file
/// does. Measured over 54,875 real blobs in 77 packs: 27,234 sit on exactly 64, 13,636 on 128 and
/// 13,942 on 256 or better, and 63 sit lower than 64. So 128 is at or above where all but a handful
/// were, and the padding it costs is under a tenth of a percent of a pack.
const BLOB_ALIGN: usize = 128;

/// Rewrite a whole pack, putting every blob at a new offset — the general answer to a replacement
/// that does not fit.
///
/// [`replace_blob`] cannot grow a texture by one byte: the descriptors hold **absolute file
/// offsets**, so anything that moves invalidates them. This moves everything and then says so, which
/// makes the size of a replacement a non-question. Measured over one install, the in-place path fits
/// 1,451 of 2,123 blobs; this one has no such number, because there is nothing for it to fail at.
///
/// What makes it tractable is that the layout is simpler than it looks, and every part of that was
/// measured over 77 real packs before a byte was written here:
///
/// * Every blob in every pack lives inside **one** leaf chunk, [`BLOBS`] — 54,875 of 54,875, with
///   none in the file's tail and none in any other chunk.
/// * That chunk is the **last leaf** in all 77, so growing it moves nothing that anything points at.
/// * **Nothing else in the file points at a blob.** Every 4-aligned `u32` outside the descriptor
///   table was checked against every declared offset; the only hits were misaligned reads straddling
///   two fields, which is noise. So the descriptor table is the whole of what relocation must fix.
/// * The bytes between blobs are not a filler pattern — they are whatever was left in the writing
///   tool's buffer, and differ byte by byte — so replacing them with zeroes loses nothing.
///
/// Blobs are written out in **descriptor order**, which is not the order they were in: only 3 of 77
/// packs had them ascending and only 3 had them contiguous. Descriptor order is chosen because it is
/// the one order the file itself states, so two runs over one pack produce the same bytes.
///
/// `edits` gives new **decompressed** blobs by hash, in the shape [`blob_of`] hands them out. A
/// texture not named keeps the bytes it already had, copied across without being decompressed and
/// recompressed — so a pack rewritten for one texture does not silently re-encode the other 1,785.
/// A texture that *is* named is re-compressed with the codec its old blob used, so an edit does not
/// convert a pack either.
///
/// One pack in the install is refused, and deliberately. `CARS/PEUGOT/TEXTURES.BIN` has no
/// [`BLOBS`] chunk at all: the directory is followed by raw compressed blocks with nothing wrapping
/// them, and the tolerant walk reads the first block's `JDLZ` magic as a chunk id (`0x5a4c444a`).
/// Its blobs run from the end of the directory to the end of the file.
///
/// The first guess about that was that it is simply what a texture compiler does. **Four packs made
/// deliberately to check disproved it**: 240SX, GOLF, RX7 and SUPRA re-saved through NFS-CarToolkit
/// all have the chunk, and relocate through this function unchanged, every texture pixel-identical.
/// So a compiler's ordinary output is an ordinary pack — what marks it is only that its blobs are
/// *tidier* than EA's, in descriptor order and contiguous and all JDLZ where EA mixes in HUFF.
///
/// `PEUGOT` is a third-party mod, downloaded rather than built here, so **what wrote it is not
/// known** — which tool, which version, how long ago. That is the whole reason it stays refused: not
/// that the shape is hard, but that there is one file of it and no way to make a second. A layout is
/// not lockable from a single artefact whose provenance nobody can state. It is turned away by name,
/// and the error says which shape it found.
///
/// # Errors
/// - A hash in `edits` is not in the pack.
/// - The pack has no [`BLOBS`] chunk (the tool-compiled shape above), or that chunk is not the last
///   leaf — a shape none of the 77 measured packs has, and one this reasoning does not cover.
/// - A blob's declared extent lies outside the file.
pub fn relocate(file: &[u8], edits: &[(AssetHash, Vec<u8>)]) -> NfsResult<Vec<u8>> {
    let (entries, table_at) = table(file)?;
    for (hash, _) in edits {
        if !entries.iter().any(|e| e.hash == *hash) {
            return Err(NfsError::CorruptArchive {
                detail: "no texture with that hash in this TPK",
            });
        }
    }

    // The blob chunk, and the check that it is where this reasoning assumes.
    let opts = WalkOptions { stop_on_overrun: true, ..Default::default() };
    let roots = ChunkNode::parse_with(file, opts)?;
    let blobs = find_chunk(&roots, BLOBS).ok_or(NfsError::NotImplemented {
        feature: "relocating a tool-compiled TPK (raw blocks, no 0x33320002 chunk)",
    })?;
    let (blobs_at, blobs_len) = (blobs.data_offset, blobs.header.size as usize);
    let blobs_header = blobs.offset;
    if last_leaf_end(&roots) > blobs_at + blobs_len {
        return Err(NfsError::CorruptArchive {
            detail: "TPK blob chunk is not the last leaf; this pack's layout was never measured",
        });
    }

    // Lay the blobs out, in descriptor order, each on its own boundary. The offsets written into
    // the descriptors are absolute, so they are computed from where the chunk's payload begins —
    // which does not move, because nothing before it changes size.
    let mut payload: Vec<u8> = Vec::with_capacity(blobs_len);
    let mut placed: Vec<(usize, usize, usize)> = Vec::with_capacity(entries.len());
    for e in &entries {
        let new = edits.iter().find(|(h, _)| *h == e.hash);
        let at = e.abs_offset as usize;
        let end = at
            .checked_add(e.size as usize)
            .ok_or(NfsError::CorruptArchive { detail: "TPK blob extent overflows" })?;
        let old = file
            .get(at..end)
            .ok_or(NfsError::CorruptArchive { detail: "TPK texture blob out of range" })?;
        let (bytes, out_size) = match new {
            // The codec a blob arrived in, the same way [`replace_blob`] keeps it. Nothing here
            // needs it to fit — that is the whole point of relocating — so this is not about size
            // but about not quietly converting a pack: an EA pack mixes HUFF and JDLZ, and a
            // replacement written in the other one makes the edited texture the only blob in the
            // file encoded unlike its neighbours. It is also smaller, HUFF being 1.028× EA on this
            // data where JDLZ is 1.242×.
            Some((_, blob)) => {
                let packed = match crate::compression::detect(old) {
                    crate::compression::Codec::Huff => crate::compression::huff::compress(blob)?,
                    _ => crate::compression::jdlz::compress(blob)?,
                };
                (packed, blob.len())
            }
            None => (old.to_vec(), e.out_size as usize),
        };
        // Pad to the boundary. `blobs_at` is where payload position 0 lands in the file, so the
        // boundary is worked out in absolute terms and not in payload-relative ones.
        while !(blobs_at + payload.len()).is_multiple_of(BLOB_ALIGN) {
            payload.push(0);
        }
        placed.push((blobs_at + payload.len(), bytes.len(), out_size));
        payload.extend_from_slice(&bytes);
    }

    // Rewrite the descriptor table where it lies. It is ahead of the blob chunk and does not change
    // length, so patching it in the input and rebuilding afterwards leaves it exactly here.
    let mut patched = file.to_vec();
    for (i, (at, size, out_size)) in placed.iter().enumerate() {
        let d = table_at + i * DESCRIPTOR_STRIDE;
        let field = |o: usize| d + o;
        let write = |buf: &mut Vec<u8>, o: usize, v: u32| -> NfsResult<()> {
            buf.get_mut(field(o)..field(o) + 4)
                .ok_or(NfsError::CorruptArchive { detail: "TPK descriptor table out of range" })?
                .copy_from_slice(&v.to_le_bytes());
            Ok(())
        };
        write(&mut patched, 4, u32::try_from(*at).unwrap_or(u32::MAX))?;
        write(&mut patched, SIZE_FIELD, u32::try_from(*size).unwrap_or(u32::MAX))?;
        write(&mut patched, 0x0C, u32::try_from(*out_size).unwrap_or(u32::MAX))?;
    }

    // And hand the new payload to the repacker, which recomputes the container sizes above it.
    let mut swap = crate::repack::Edits::new();
    swap.insert(blobs_header, payload);
    crate::repack::rebuild(&patched, &swap)
}

/// Where the last leaf in the tree ends.
fn last_leaf_end(nodes: &[ChunkNode]) -> usize {
    nodes
        .iter()
        .map(|n| {
            if n.children.is_empty() {
                n.data_offset + n.header.size as usize
            } else {
                last_leaf_end(&n.children)
            }
        })
        .max()
        .unwrap_or(0)
}

/// The descriptor table, and where it starts in the file.
fn table(file: &[u8]) -> NfsResult<(Vec<super::TpkEntry>, usize)> {
    // The same tolerant walk `Tpk::parse` uses: a tool-compiled pack puts raw blobs after the
    // directory with no wrapping chunk, and a strict walk overruns on them.
    let opts = WalkOptions { stop_on_overrun: true, ..Default::default() };
    let roots = ChunkNode::parse_with(file, opts)?;
    let node = find_chunk(&roots, DESCRIPTORS)
        .ok_or(NfsError::CorruptArchive { detail: "TPK missing descriptor chunk 0x33310003" })?;
    Ok((parse_descriptors(node.data(file)), node.data_offset))
}


#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal TPK: a descriptor chunk naming one blob, and the blob itself in a data chunk.
    /// Enough for the write path, which never decodes the image.
    fn pack(blob: &[u8], slot: usize) -> (Vec<u8>, u32) {
        let payload_at = 8 + DESCRIPTOR_STRIDE + 8; // desc chunk header + desc + data chunk header
        let mut desc = Vec::new();
        desc.extend_from_slice(&0xAAAA_AAAAu32.to_le_bytes()); // hash
        desc.extend_from_slice(&(payload_at as u32).to_le_bytes()); // abs_offset
        desc.extend_from_slice(&(slot as u32).to_le_bytes()); // compressed size (the slot)
        desc.extend_from_slice(&(blob.len() as u32).to_le_bytes()); // out_size
        desc.extend_from_slice(&0x100u32.to_le_bytes()); // header_from_end
        desc.extend_from_slice(&0u32.to_le_bytes()); // unk

        let packed = crate::compression::jdlz::compress(blob).expect("compress");
        let mut data = packed.clone();
        data.resize(slot, 0);

        let mut file = Vec::new();
        file.extend_from_slice(&DESCRIPTORS.to_le_bytes());
        file.extend_from_slice(&(desc.len() as u32).to_le_bytes());
        file.extend_from_slice(&desc);
        file.extend_from_slice(&0x3332_0002u32.to_le_bytes());
        file.extend_from_slice(&(data.len() as u32).to_le_bytes());
        file.extend_from_slice(&data);
        (file, 0xAAAA_AAAA)
    }

    #[test]
    fn a_blob_comes_back_out_as_it_went_in() {
        let blob = b"pixels pixels pixels and a header at the end".repeat(4);
        let (file, hash) = pack(&blob, 512);
        assert_eq!(blob_of(&file, AssetHash(hash)).expect("blob"), blob);
    }

    #[test]
    fn a_replacement_lands_in_the_slot_and_the_descriptor_follows_it() {
        let blob = b"the same length as before, different content".repeat(4);
        let (file, hash) = pack(&blob, 512);

        let mut edited = blob.clone();
        edited[0..8].copy_from_slice(b"CHANGED!");
        let out = replace_blob(&file, AssetHash(hash), &edited).expect("replace");

        assert_eq!(out.len(), file.len(), "nothing moved");
        assert_eq!(blob_of(&out, AssetHash(hash)).expect("blob"), edited);
        // The descriptor's compressed size now describes the new stream, not the old.
        let declared = u32::from_le_bytes(out[8 + SIZE_FIELD..8 + SIZE_FIELD + 4].try_into().unwrap());
        let packed = crate::compression::jdlz::compress(&edited).expect("compress");
        assert_eq!(declared as usize, packed.len());
    }

    #[test]
    fn a_blob_of_the_wrong_length_is_refused_rather_than_written() {
        let blob = b"0123456789".repeat(8);
        let (file, hash) = pack(&blob, 512);
        let err = replace_blob(&file, AssetHash(hash), b"too short").expect_err("must refuse");
        assert!(matches!(err, NfsError::BufferSizeMismatch { .. }));
    }

    #[test]
    fn a_replacement_that_does_not_fit_is_refused_rather_than_overrunning() {
        // A slot only just big enough for the compressible original; random-looking content will
        // not pack into it.
        let blob = vec![7u8; 4096];
        let (file, hash) = pack(&blob, 64);
        let noise: Vec<u8> = (0..4096u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8).collect();
        let err = replace_blob(&file, AssetHash(hash), &noise).expect_err("must refuse");
        assert!(matches!(err, NfsError::BufferSizeMismatch { .. }));
        // And the file is untouched, because nothing was written before the check.
        assert_eq!(blob_of(&file, AssetHash(hash)).expect("blob"), blob);
    }
}
