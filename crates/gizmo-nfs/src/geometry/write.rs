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

/// New attribute values for one solid's mesh, at its **existing** topology.
///
/// Every array is per-vertex and must be as long as the mesh already is; `indices` likewise. That is
/// the whole restriction, and it is what makes this the safe half of a mesh write: nothing changes
/// length, so no chunk moves, no count in the mesh header goes stale, the submesh runs still tile
/// the index buffer, and the alignment padding in front of each buffer stays the padding the file
/// chose. A mesh whose vertex *count* changes needs all of those recomputed and is a different
/// function — see [`replace_mesh`]'s own note.
pub struct Mesh<'a> {
    pub positions: &'a [[f32; 3]],
    pub normals: &'a [[f32; 3]],
    /// RGBA8, as [`crate::NfsMeshPart::colours`] hands it out. Written back B,G,R,A.
    pub colours: &'a [[u8; 4]],
    pub uvs: &'a [[f32; 2]],
    /// Triangle-list indices. Must be the length the mesh already has, and in range.
    pub indices: &'a [u32],
}

/// Write one solid's mesh back, at its existing topology.
///
/// `solid` is the header offset of a `0x80134010` chunk — the same number the chunk tree and
/// PryHUB's selection use, and the only stable name a solid has: 24.8% of them share their *name*
/// with another solid in the same file.
///
/// What it rewrites is the vertex buffer, the index buffer, and the bounding box in the solid's
/// header. The bbox is not the vertices' AABB and copying one in would be wrong: measured over
/// 23,299 real solids it is the minimum **minus 0.01** and the maximum **plus 0.01** in 23,292 of
/// them, every exception being in the `PEUGOT` mod. So it is recomputed by that rule, which is also
/// why writing a mesh back unchanged reproduces the file's own bytes.
///
/// **Same topology only.** The vertex and index counts must be what the mesh already has. Changing
/// them means rewriting the mesh header's two counts, the 60-byte submesh runs that tile the index
/// buffer, and the alignment padding in front of both buffers — the vertices start on a 128-byte
/// file boundary, which is a property of where the buffer lands and therefore of the rebuild that
/// has not happened yet. That is a real next step, not a hidden one; this function refuses rather
/// than guessing at it.
///
/// # Errors
/// - `solid` is not a `0x80134010` header, or it has no mesh.
/// - Any array is not the length the mesh already has, or an index is out of range.
/// - The solid uses the packed vertex layout this crate does not decode (one solid in the install).
pub fn replace_mesh(bytes: &[u8], solid: usize, mesh: &Mesh<'_>) -> NfsResult<Vec<u8>> {
    use super::format::{
        FILLER_BYTE, INDEX_BUFFER, MESH_HEADER, MESH_TRI_COUNT_FIELD, MESH_VERT_COUNT_FIELD,
        SOLID_HEADER, VERTEX_BUFFER, VERTEX_STRIDE,
    };

    let tree = ChunkNode::parse(bytes)?;
    let node = at(&tree, solid)
        .filter(|n| n.header.id == SOLID)
        .ok_or(NfsError::CorruptArchive { detail: "no solid at that offset" })?;
    let (Some(header), Some(mh), Some(vb), Some(ib)) = (
        find(std::slice::from_ref(node), SOLID_HEADER),
        find(std::slice::from_ref(node), MESH_HEADER),
        find(std::slice::from_ref(node), VERTEX_BUFFER),
        find(std::slice::from_ref(node), INDEX_BUFFER),
    ) else {
        return Err(NfsError::CorruptArchive { detail: "that solid has no mesh" });
    };

    // The counts the file states, read the way the parser reads them.
    let md = mh.data(bytes);
    let body = super::skip_leading_filler(md);
    let word = |i: usize| -> NfsResult<usize> {
        let o = i * 4;
        let b = body
            .get(o..o + 4)
            .ok_or(NfsError::BufferSizeMismatch { detail: "mesh header too short" })?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
    };
    let verts = word(MESH_VERT_COUNT_FIELD)?;
    let tris = word(MESH_TRI_COUNT_FIELD)?;

    let vbuf = vb.data(bytes);
    if !super::standard_vertex_layout(verts, vbuf.len()) {
        return Err(NfsError::NotImplemented { feature: "packed vertex layout" });
    }
    let n = mesh.positions.len();
    if n != verts
        || mesh.normals.len() != n
        || mesh.colours.len() != n
        || mesh.uvs.len() != n
        || mesh.indices.len() != tris * 3
    {
        return Err(NfsError::BufferSizeMismatch {
            detail: "a mesh replacement must keep the vertex and index counts it already has",
        });
    }
    if mesh.indices.iter().any(|i| *i as usize >= n) {
        return Err(NfsError::CorruptArchive { detail: "an index is past the last vertex" });
    }

    // The vertex buffer: the file's own leading padding, then the records.
    let lead = vbuf.len() - n * VERTEX_STRIDE;
    let mut vout = vbuf[..lead].to_vec();
    for i in 0..n {
        let (p, nr, c, uv) = (mesh.positions[i], mesh.normals[i], mesh.colours[i], mesh.uvs[i]);
        for f in [p[0], p[1], p[2], nr[0], nr[1], nr[2]] {
            vout.extend_from_slice(&f.to_le_bytes());
        }
        vout.extend_from_slice(&[c[2], c[1], c[0], c[3]]); // back to B,G,R,A
        for f in [uv[0], uv[1]] {
            vout.extend_from_slice(&f.to_le_bytes());
        }
    }

    // The index buffer, and it is **not** anchored the way the vertex buffer is. Vertices occupy the
    // last `count * 36` bytes, so a tail anchor is right for them; indices start just past the
    // leading `0x11` filler and some solids — wheels, bumpers — carry a *trailing* pad as well. The
    // reader says so in as many words (`geometry::index`), having been the bug that shredded meshes
    // into shards; anchoring the write at the tail reintroduced it here, on a decal whose buffer has
    // four bytes of tail. Both ends are kept exactly as they were and only the indices are written.
    let ibuf = ib.data(bytes);
    let ilead = ibuf.iter().take_while(|&&b| b == FILLER_BYTE).count();
    let end = ilead
        .checked_add(tris * 6)
        .filter(|e| *e <= ibuf.len())
        .ok_or(NfsError::BufferSizeMismatch { detail: "index buffer smaller than tri_count*6" })?;
    let mut iout = ibuf[..ilead].to_vec();
    for i in mesh.indices {
        iout.extend_from_slice(&(*i as u16).to_le_bytes());
    }
    iout.extend_from_slice(&ibuf[end..]);
    debug_assert_eq!(iout.len(), ibuf.len());

    // The bounding box, by the file's own rule rather than as a plain AABB.
    let mut hout = header.data(bytes).to_vec();
    let (min, max) = super::vertex::bounds(mesh.positions);
    for (base, values, sign) in [(0x20usize, min, -0.01f32), (0x30, max, 0.01)] {
        for (k, v) in values.iter().enumerate() {
            let o = base + k * 4;
            hout.get_mut(o..o + 4)
                .ok_or(NfsError::BufferSizeMismatch { detail: "solid header too short" })?
                .copy_from_slice(&(v + sign).to_le_bytes());
        }
    }

    let mut edits = Edits::new();
    edits.insert(vb.offset, vout);
    edits.insert(ib.offset, iout);
    edits.insert(header.offset, hout);
    rebuild(bytes, &edits)
}

/// The chunk whose header sits at `offset`.
fn at(nodes: &[ChunkNode], offset: usize) -> Option<&ChunkNode> {
    for n in nodes {
        if n.offset == offset {
            return Some(n);
        }
        if let Some(f) = at(&n.children, offset) {
            return Some(f);
        }
    }
    None
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
