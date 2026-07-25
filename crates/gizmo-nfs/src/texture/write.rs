//! Putting a texture back into a TPK.
//!
//! A TPK is the one file in this format that cannot simply be reassembled. Its descriptors point at
//! pixel blobs by **absolute file offset**, and the blobs are neither in descriptor order nor packed
//! tightly — measured over 30 real packs, only one has them in ascending order and only one has them
//! contiguous. Moving anything therefore means rewriting every offset, and moving anything *inside*
//! a chunk changes that chunk's size, which moves the blobs after it again.
//!
//! So this module does the part that needs no relocation: replace a texture **in place**. The new
//! blob is compressed with [`crate::compression::jdlz`], and if it fits the slot the old one
//! occupied, it is written there and the descriptor's compressed size updated. Nothing else in the
//! file moves — not one other offset changes — which is why this is safe to do without a theory of
//! the whole layout.
//!
//! When it does not fit, that is said plainly rather than worked around. Relocation is the next
//! piece of work, and it needs the layout question answered properly.

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
/// When the hash is not in the pack, or its blob cannot be decompressed (a HUFF-compressed texture
/// still cannot be read, so it cannot be written either).
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
///   caller is told rather than silently overwriting whatever follows.
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
    let packed = crate::compression::jdlz::compress(blob)?;
    if packed.len() > entry.size as usize {
        return Err(NfsError::BufferSizeMismatch {
            detail: "the recompressed texture is larger than the slot it has to fit in",
        });
    }

    let at = entry.abs_offset as usize;
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
