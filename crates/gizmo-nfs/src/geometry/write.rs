//! Writing a `GEOMETRY.BIN` back — the part that makes it safe to.
//!
//! [`crate::repack::rebuild`] already reassembles any chunk stream, recomputing container sizes and
//! alignment padding, and it handles a leaf that changed length. Used on a `GEOMETRY.BIN` directly it
//! also quietly breaks the file, and for exactly the reason a TPK cannot be reassembled either:
//! **there is a table of absolute file offsets, and the repacker does not know about it.**
//!
//! `0x80134001 → 0x00134004` is one 24-byte record per solid. Measured over the install's 57
//! `GEOMETRY.BIN` — 18,230 records — every field of it holds:
//!
//! | lane | holds | measured |
//! |---|---|---|
//! | 0 | the solid's name hash | — |
//! | 1 | the **absolute file offset** of its `0x80134010` header | 18,230 / 18,230 |
//! | 2 | the solid's chunk size **+ 8**, i.e. with its header | 18,230 / 18,230 |
//! | 3 | the same value again | 18,230 / 18,230 |
//! | 4, 5 | zero | 18,230 / 18,230 |
//!
//! One car of the 57 has no such chunk at all; in the 56 that do, the record count equals the solid
//! count exactly, and every record's lane 1 lands on a solid header. The records are **not** in file
//! order — the first record of a 240SX points at offset 5,154,048 while the first solid is at
//! 19,712 — so a fix-up cannot pair them by position.
//!
//! [`rebuild`] is therefore what a caller should use instead of the general repacker: it does the
//! same job and then puts the directory back in agreement with the file. Grow one vertex buffer
//! without it and 608 of a 240SX's 609 records point at a byte that is no longer a solid.

use super::format::SOLID;
use crate::chunk::ChunkNode;
use crate::error::{NfsError, NfsResult};
use crate::repack::Edits;
use std::collections::BTreeMap;

/// The per-solid directory: `0x80134001 → 0x00134004`.
pub const SOLID_DIRECTORY: u32 = 0x0013_4004;

/// Bytes per directory record, and the lanes this module writes.
const RECORD: usize = 24;
const LANE_OFFSET: usize = 4;
const LANE_SIZE_A: usize = 8;
const LANE_SIZE_B: usize = 12;

/// Rebuild a `GEOMETRY.BIN` with edited leaves, keeping its solid directory true.
///
/// The same contract as [`crate::repack::rebuild`] — `edits` maps a leaf's header offset to its new
/// payload — plus the one thing that repacker cannot know: if anything changed length, the solids
/// after it moved, and the directory has to be told.
///
/// Solids are paired between the two files **by tree order**, which is exact because rebuilding
/// neither adds nor removes a chunk; the directory's own records are then found by their *old*
/// offset and rewritten to the new one. Pairing them by position in the directory would be wrong:
/// the records are not in file order.
///
/// A file with no directory chunk is rebuilt and returned unchanged in that respect — there is
/// nothing to keep true. A file whose directory disagrees with its own solids before the rebuild is
/// refused rather than repaired, because a table this function cannot explain is one it should not
/// be rewriting.
///
/// # Errors
/// - The chunk stream will not parse, or the repacker refuses it.
/// - The rebuild changed how many solids there are, which would mean `edits` did something this
///   function does not model.
/// - A record's offset does not name a solid in the original file.
pub fn rebuild(bytes: &[u8], edits: &Edits) -> NfsResult<Vec<u8>> {
    let before = ChunkNode::parse(bytes)?;
    let mut out = crate::repack::rebuild(bytes, edits)?;

    let Some(dir) = find(&before, SOLID_DIRECTORY) else {
        return Ok(out); // no table to keep true
    };
    let dir_len = dir.header.size as usize;

    let old = solids(&before);
    let after = ChunkNode::parse(&out)?;
    let new = solids(&after);
    if old.len() != new.len() {
        return Err(NfsError::CorruptArchive {
            detail: "rebuilding changed how many solids the file has",
        });
    }
    // Where each solid went, and how big it is now.
    let moved: BTreeMap<u32, (u32, u32)> = old
        .iter()
        .zip(new.iter())
        .map(|(o, n)| (o.0, (n.0, n.1)))
        .collect();

    // The directory sits ahead of every solid and does not change length, so it is at the same
    // offset in the rebuilt file — which is worth asserting rather than assuming, because the whole
    // point of this function is that offsets move.
    let dir_now = find(&after, SOLID_DIRECTORY)
        .ok_or(NfsError::CorruptArchive { detail: "the rebuilt file lost its solid directory" })?;
    if dir_now.header.size as usize != dir_len {
        return Err(NfsError::CorruptArchive { detail: "the solid directory changed length" });
    }
    let at = dir_now.data_offset;

    for r in 0..dir_len / RECORD {
        let base = at + r * RECORD;
        let read = |o: usize| -> NfsResult<u32> {
            let b = out
                .get(base + o..base + o + 4)
                .ok_or(NfsError::CorruptArchive { detail: "solid directory out of range" })?;
            Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        };
        let was = read(LANE_OFFSET)?;
        let Some((now, size)) = moved.get(&was).copied() else {
            return Err(NfsError::CorruptArchive {
                detail: "a solid directory record does not name a solid",
            });
        };
        for (lane, value) in [(LANE_OFFSET, now), (LANE_SIZE_A, size + 8), (LANE_SIZE_B, size + 8)] {
            out.get_mut(base + lane..base + lane + 4)
                .ok_or(NfsError::CorruptArchive { detail: "solid directory out of range" })?
                .copy_from_slice(&value.to_le_bytes());
        }
    }
    Ok(out)
}

/// Every solid's header offset and chunk size, in tree order.
fn solids(nodes: &[ChunkNode]) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    fn walk(nodes: &[ChunkNode], out: &mut Vec<(u32, u32)>) {
        for n in nodes {
            if n.header.id == SOLID {
                out.push((n.offset as u32, n.header.size));
            }
            walk(&n.children, out);
        }
    }
    walk(nodes, &mut out);
    out
}

/// The first chunk with `id`, roots included.
fn find(nodes: &[ChunkNode], id: u32) -> Option<&ChunkNode> {
    for n in nodes {
        if n.header.id == id {
            return Some(n);
        }
        if let Some(found) = find(&n.children, id) {
            return Some(found);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A chunk header plus payload.
    fn chunk(id: u32, payload: &[u8]) -> Vec<u8> {
        let mut out = id.to_le_bytes().to_vec();
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(payload);
        out
    }

    /// A minimal geometry-shaped file: a header container holding the directory, then two solids.
    ///
    /// Built in two passes — assembled with blank records, then parsed so the *real* solid offsets
    /// can be written into them. Computing those by hand is how the first version of this fixture
    /// was wrong, and it is also what the function under test refuses to do.
    ///
    /// The records are written in the **reverse** of file order, which is what makes pairing them by
    /// position wrong — as they are in the real files.
    fn file() -> (Vec<u8>, Vec<usize>) {
        let solid_a = chunk(SOLID, &chunk(0x0013_4011, &[0xAA; 16]));
        let solid_b = chunk(SOLID, &chunk(0x0013_4011, &[0xBB; 16]));
        let dir = vec![0u8; RECORD * 2];
        let header_container = chunk(0x8013_4001, &chunk(SOLID_DIRECTORY, &dir));

        let mut body = header_container;
        body.extend_from_slice(&solid_a);
        body.extend_from_slice(&solid_b);
        let mut bytes = chunk(0x8013_4000, &body);

        // Now ask the parser where things actually landed, and fill the records in reverse.
        let tree = ChunkNode::parse(&bytes).expect("fixture parses");
        let found = solids(&tree);
        assert_eq!(found.len(), 2);
        let at = find(&tree, SOLID_DIRECTORY).expect("directory").data_offset;
        for (r, (off, size)) in found.iter().rev().enumerate() {
            let base = at + r * RECORD;
            bytes[base + LANE_OFFSET..base + LANE_OFFSET + 4].copy_from_slice(&off.to_le_bytes());
            for lane in [LANE_SIZE_A, LANE_SIZE_B] {
                bytes[base + lane..base + lane + 4].copy_from_slice(&(size + 8).to_le_bytes());
            }
        }
        let leaves = found.iter().map(|(o, _)| *o as usize + 8).collect();
        (bytes, leaves)
    }

    /// Read a record's offset lane.
    fn record_offsets(file: &[u8]) -> Vec<u32> {
        let tree = ChunkNode::parse(file).expect("parse");
        let dir = find(&tree, SOLID_DIRECTORY).expect("directory");
        let data = dir.data(file);
        (0..data.len() / RECORD)
            .map(|r| {
                let o = r * RECORD + LANE_OFFSET;
                u32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]])
            })
            .collect()
    }

    #[test]
    fn a_rebuild_that_moves_nothing_leaves_the_directory_true() {
        let (bytes, _) = file();
        let out = rebuild(&bytes, &Edits::new()).expect("rebuild");
        // Not "byte-identical": the repacker aligns `0x80134010` to 128, so a hand-built fixture
        // whose solids are not on that grid is moved by the rebuild itself. What must hold is the
        // claim — every record names a solid.
        let tree = ChunkNode::parse(&out).expect("parse");
        let live: std::collections::BTreeSet<u32> = solids(&tree).iter().map(|s| s.0).collect();
        for off in record_offsets(&out) {
            assert!(live.contains(&off), "{off} is not a solid after a no-op rebuild");
        }
    }

    #[test]
    fn growing_a_leaf_moves_the_solids_and_the_directory_follows() {
        let (bytes, leaves) = file();
        let before = record_offsets(&bytes);

        // Grow the *first* solid's leaf, which pushes the second solid along.
        let mut edits = Edits::new();
        edits.insert(leaves[0], vec![0xAA; 64]);
        let out = rebuild(&bytes, &edits).expect("rebuild");

        let after = record_offsets(&out);
        assert_ne!(before, after, "the second solid moved and its record must say so");
        // Every record still names a solid, which is the whole claim.
        let tree = ChunkNode::parse(&out).expect("parse");
        let live: std::collections::BTreeSet<u32> = solids(&tree).iter().map(|s| s.0).collect();
        for off in &after {
            assert!(live.contains(off), "{off} is not a solid in the rebuilt file");
        }
        // And the records are still in the order they were written, not re-sorted into file order.
        assert!(after[0] > after[1], "record order must be preserved, not normalised");
    }

    #[test]
    fn the_plain_repacker_is_what_this_exists_to_replace() {
        // The same edit through `repack::rebuild` leaves the directory pointing at the old bytes,
        // which is the failure this module is for. Asserted so the difference cannot quietly go away.
        let (bytes, leaves) = file();
        let mut edits = Edits::new();
        edits.insert(leaves[0], vec![0xAA; 64]);
        let naive = crate::repack::rebuild(&bytes, &edits).expect("rebuild");

        let tree = ChunkNode::parse(&naive).expect("parse");
        let live: std::collections::BTreeSet<u32> = solids(&tree).iter().map(|s| s.0).collect();
        let stale = record_offsets(&naive).into_iter().filter(|o| !live.contains(o)).count();
        assert!(stale > 0, "the general repacker was expected to leave a stale pointer");
    }

    #[test]
    fn a_file_with_no_directory_is_just_rebuilt() {
        let solid = chunk(SOLID, &chunk(0x0013_4011, &[1, 2, 3, 4]));
        let bytes = chunk(0x8013_4000, &solid);
        // Whatever the repacker makes of it, this function must agree — it has nothing to add.
        let plain = crate::repack::rebuild(&bytes, &Edits::new()).expect("repack");
        assert_eq!(rebuild(&bytes, &Edits::new()).expect("rebuild"), plain);
    }

    #[test]
    fn a_record_that_names_nothing_is_refused_rather_than_repaired() {
        let (mut bytes, _) = file();
        // Point the first record somewhere that is not a solid.
        let tree = ChunkNode::parse(&bytes).expect("parse");
        let at = find(&tree, SOLID_DIRECTORY).expect("directory").data_offset + LANE_OFFSET;
        bytes[at..at + 4].copy_from_slice(&0xDEAD_u32.to_le_bytes());
        assert!(matches!(
            rebuild(&bytes, &Edits::new()).expect_err("must refuse"),
            NfsError::CorruptArchive { .. }
        ));
    }
}
