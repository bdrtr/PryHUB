//! Writing chunk streams back out — the read path run backwards.
//!
//! Everything else in this crate takes bytes apart. This puts them together again, and the reason
//! it exists before any editor does is that a repacker which cannot reproduce a file it did not
//! change has no business changing one. [`rebuild`] with no edits is asserted **byte-exact against
//! 113 real files** in `tests/golden_assets.rs`; that is what makes the layout rules below facts
//! rather than an opinion.
//!
//! # The layout rules, as measured
//!
//! Over 216,713 chunks of a real install: every chunk size and every chunk offset is a multiple of
//! 4, and the file uses `id == 0` chunks — invisible in the tree, which skips them — purely as
//! padding to put certain chunks on a boundary:
//!
//! * **`0x80134010` (SolidObject) starts on a 128-byte boundary. 18,230 out of 18,230.** The
//!   padding chunk before it exists only when it is needed: 644 solids that already landed on 128
//!   have none.
//! * When the debt is 4 bytes there is no way to pay it — a chunk header is 8 — so the file pays
//!   132 (4 + 128) instead. 646 paddings are exactly that, and 8 is the smallest.
//! * `0x80034020` aligns to **64**, not 128.
//!
//! And one thing the measurement did *not* say until a rebuild disagreed: an `id == 0` chunk is not
//! always alignment. BUS's `GEOMETRY.BIN` has one of 1,332 bytes where alignment asks for 52, with a
//! non-zero byte inside it. So padding is only re-derived when it is exactly what the rule would
//! have produced, and copied otherwise — see [`is_alignment_padding`].
//!
//! # What is copied rather than recomputed
//!
//! Bytes the chunk tree does not describe: the slack some containers leave after their last child,
//! and the tail some files carry after their last chunk (a TPK's pixel blobs live there — 57 KB of
//! it in one file). Those are moved through verbatim, because nothing here knows what they mean.
//! A TPK is therefore *not* yet safely editable: its descriptors point at **absolute file offsets**
//! into that tail, so moving anything invalidates them. [`rebuild`] will faithfully rebuild one;
//! it will not fix up its offsets.

use crate::chunk::ChunkNode;
use crate::error::NfsResult;
use std::collections::BTreeMap;

/// Bytes of a chunk header: the id and the payload size, both little-endian.
const HEADER: usize = 8;

/// The boundary a chunk of this id starts on, as measured on a real install. Everything not listed
/// simply follows the format's universal 4-byte alignment.
///
/// This is a lookup rather than a rule because it *is* a lookup: nothing in the bytes says which
/// ids are aligned, only that these ones always are.
#[must_use]
pub fn required_alignment(id: u32) -> usize {
    match id {
        // A car's solids, and the TPK data part's container and its blob chunk.
        0x8013_4010 | 0xB332_0000 | 0x3332_0002 => 128,
        0x8003_4020 => 64,
        _ => 4,
    }
}

/// Padding needed before a chunk of `id` placed at `offset`, in bytes (header included).
///
/// Zero when it already lands right. Otherwise the smallest padding that both reaches the boundary
/// and is big enough to *be* a chunk: a debt under 8 cannot hold a header, so it is paid with a
/// whole further boundary. The files do exactly this — 646 paddings are 132, which is a 4-byte debt
/// plus 128.
#[must_use]
pub fn padding_for(id: u32, offset: usize) -> usize {
    let align = required_alignment(id);
    let mut pad = (align - offset % align) % align;
    while pad != 0 && pad < HEADER {
        pad += align;
    }
    pad
}

/// Payload sizes in this format are always multiples of 4 — 216,713 chunks of a real install, no
/// exceptions — so an edited payload is zero-padded up to one.
///
/// Silently changing someone's bytes deserves a word: the alternative is emitting a size the format
/// never uses, which would put every chunk after it off its alignment and produce a file the game
/// reads as garbage. The pad is at the end, where a parser reading declared counts will not look.
fn aligned_len(len: usize) -> usize {
    len.next_multiple_of(4)
}

/// New payloads for leaves, keyed by the leaf's **header offset in the original file** — the same
/// key selection and the inspector use, so a caller names a chunk the way it already does.
pub type Edits = BTreeMap<usize, Vec<u8>>;

/// Rebuild `bytes` as a chunk stream, replacing the payloads named in `edits`.
///
/// Container sizes are recomputed from what they end up containing, padding is recomputed from the
/// alignment rules, and everything the tree does not describe is copied. With no edits the result
/// is the input, byte for byte.
///
/// # Errors
/// Only from parsing the input; assembling cannot fail.
pub fn rebuild(bytes: &[u8], edits: &Edits) -> NfsResult<Vec<u8>> {
    let tree = ChunkNode::parse(bytes)?;
    let mut out = Vec::with_capacity(bytes.len());
    write_nodes(&tree, bytes, edits, &mut out, (0, bytes.len()));
    Ok(out)
}

/// Whether the bytes between two chunks are nothing but alignment padding.
///
/// Not every `id == 0` chunk is: BUS's `GEOMETRY.BIN` carries one of 1,332 bytes where alignment
/// asks for 52, and its payload is not even all zeroes — one byte in it is set. Whatever that is,
/// it is content, and a repacker that recomputed it away would quietly delete part of the file. So
/// a gap is only *re-derived* when it is exactly what the alignment rule would have produced;
/// anything else is copied through untouched.
fn is_alignment_padding(gap: &[u8], id: u32, gap_start: usize) -> bool {
    gap.len() >= HEADER
        && gap.len() == padding_for(id, gap_start)
        && gap[0..4] == [0, 0, 0, 0]
        && gap[HEADER..].iter().all(|b| *b == 0)
}

/// Emit a sibling list, then whatever the tree left behind up to `region_end`.
fn write_nodes(
    nodes: &[ChunkNode],
    bytes: &[u8],
    edits: &Edits,
    out: &mut Vec<u8>,
    region: (usize, usize),
) {
    let (region_start, region_end) = region;
    let mut original_cursor = region_start;
    for node in nodes {
        let gap = bytes.get(original_cursor..node.offset).unwrap_or(&[]);
        if !gap.is_empty() && !is_alignment_padding(gap, node.header.id, original_cursor) {
            out.extend_from_slice(gap);
        }
        let pad = padding_for(node.header.id, out.len());
        if pad > 0 {
            // A padding chunk is a zero id, a size, and zeroes — the file's own idiom.
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&((pad - HEADER) as u32).to_le_bytes());
            out.resize(out.len() + pad - HEADER, 0);
        }
        write_node(node, bytes, edits, out);
        original_cursor = node.data_offset + node.header.size as usize;
    }
    // Bytes after the last chunk of this region that the tree does not account for: a container's
    // slack, or a file's tail. Copied, because their meaning is not this module's to know.
    if original_cursor < region_end {
        out.extend_from_slice(&bytes[original_cursor..region_end]);
    }
}

fn write_node(node: &ChunkNode, bytes: &[u8], edits: &Edits, out: &mut Vec<u8>) {
    let header_at = out.len();
    out.extend_from_slice(&node.header.id.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // size, written once it is known
    if node.children.is_empty() {
        match edits.get(&node.offset) {
            Some(payload) => {
                out.extend_from_slice(payload);
                out.resize(out.len() + aligned_len(payload.len()) - payload.len(), 0);
            }
            None => out.extend_from_slice(node.data(bytes)),
        }
    } else {
        let region_end = node.data_offset + node.header.size as usize;
        write_nodes(&node.children, bytes, edits, out, (node.data_offset, region_end));
    }
    let size = (out.len() - header_at - HEADER) as u32;
    out[header_at + 4..header_at + HEADER].copy_from_slice(&size.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::tests::chunk;
    use crate::chunk::CONTAINER_FLAG;

    #[test]
    fn a_stream_with_nothing_to_change_comes_back_identical() {
        let mut bytes = chunk(0x0000_0001, &[1, 2, 3, 4]);
        bytes.extend(chunk(CONTAINER_FLAG | 0x10, &chunk(0x0000_0002, &[5; 8])));
        let out = rebuild(&bytes, &Edits::new()).expect("rebuild");
        assert_eq!(out, bytes);
    }

    #[test]
    fn a_replaced_payload_resizes_every_container_above_it() {
        let inner = chunk(0x0000_0002, &[5; 8]);
        let bytes = chunk(CONTAINER_FLAG | 0x10, &inner);
        // The leaf sits 8 bytes in, behind its parent's header.
        let mut edits = Edits::new();
        edits.insert(HEADER, vec![7; 16]);
        let out = rebuild(&bytes, &edits).expect("rebuild");

        let tree = ChunkNode::parse(&out).expect("parse the rebuilt stream");
        assert_eq!(tree[0].header.size, (HEADER + 16) as u32, "the container grew by 8");
        assert_eq!(tree[0].children[0].data(&out), &[7; 16], "the new payload is there");
        assert_eq!(out.len(), bytes.len() + 8);
    }

    /// The rule that cost this project a measurement: a solid must start on 128, the padding chunk
    /// exists only when it is needed, and a 4-byte debt is paid with 132 because 4 cannot hold a
    /// header.
    #[test]
    fn a_solid_is_padded_onto_its_boundary_and_never_by_four() {
        assert_eq!(padding_for(0x8013_4010, 0), 0, "already aligned");
        assert_eq!(padding_for(0x8013_4010, 128), 0);
        assert_eq!(padding_for(0x8013_4010, 120), 8, "the smallest padding a chunk can be");
        assert_eq!(padding_for(0x8013_4010, 124), 132, "4 is unpayable, so pay 4 + 128");
        assert_eq!(padding_for(0x8003_4020, 100), 28, "this one aligns to 64");
        // No id-specific rule, so only the universal 4 applies — and a 2-byte debt is likewise
        // unpayable, so it becomes 10. In a well-formed file this never comes up: payload sizes are
        // multiples of 4, which is why `rebuild` pads an edited payload up to one.
        assert_eq!(padding_for(0x0013_4900, 6), 10);
        assert_eq!(padding_for(0x0013_4900, 8), 0);
    }

    #[test]
    fn a_solid_that_lands_off_the_boundary_gets_a_zero_id_chunk_in_front() {
        // A 12-byte leaf first, so the solid would otherwise start at 20. The solid's payload is a
        // chunk of its own, because an id with the container bit set is read as a container.
        let mut bytes = chunk(0x0000_0001, &[0; 12]);
        bytes.extend(chunk(0x8013_4010, &chunk(0x0013_4011, &[9; 8])));
        let out = rebuild(&bytes, &Edits::new()).expect("rebuild");

        // The original was not aligned, so the rebuild is *not* the input here — it is the input
        // with the padding the format asks for.
        let tree = ChunkNode::parse(&out).expect("parse");
        let solid = tree.iter().find(|n| n.header.id == 0x8013_4010).expect("the solid");
        assert_eq!(solid.offset % 128, 0, "the solid landed on its boundary");
        assert_eq!(u32::from_le_bytes(out[20..24].try_into().unwrap()), 0, "padding id");
        assert_eq!(solid.children[0].data(&out), &[9; 8], "and its payload survived the move");
    }
}
