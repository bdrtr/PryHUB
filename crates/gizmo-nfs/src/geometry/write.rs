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

/// The mesh header word that states how many `0x00134B02` runs the solid has.
///
/// Measured rather than taken from the parser, which never reads it: word 4 equals the number of
/// entries in 23,300 of 23,300 real solids. The reader derives the count from the chunk's size
/// instead, which is why it could go without — a writer cannot.
const SUBMESH_COUNT_FIELD: usize = 4;

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

/// A mesh to write into a solid.
///
/// Every attribute array is per-vertex and they must all be the same length; `indices` is a triangle
/// list into them. Unlike the first version of this, the counts **may differ** from what the solid
/// holds — that is what makes it usable for a model that has been through an editor, where welding a
/// seam or adding a loop changes them by construction.
///
/// `runs` is the one thing that cannot be invented. The 60-byte `0x00134B02` entries carry thirteen
/// words this crate does not decode — a per-material bounding box among them — so a *new* run has no
/// template to be built from. Their counts and offsets are rewritten from what is given here; their
/// number must stay what the solid already has.
pub struct Mesh<'a> {
    pub positions: &'a [[f32; 3]],
    pub normals: &'a [[f32; 3]],
    /// RGBA8, as [`crate::NfsMeshPart::colours`] hands it out. Written back B,G,R,A.
    pub colours: &'a [[u8; 4]],
    pub uvs: &'a [[f32; 2]],
    /// Triangle-list indices into the arrays above.
    pub indices: &'a [u32],
    /// Per-material index runs, which must tile `indices` end to end from zero — as they do in
    /// 23,300 of 23,300 real solids. `None` keeps the solid's own runs, which is only meaningful
    /// when the index count has not changed.
    pub runs: Option<&'a [Run]>,
}

/// One material's slice of the index list.
///
/// Deliberately **not** [`crate::NfsMaterialRange`], which is what the reader hands out. That type
/// also carries the run's material and shader hashes, and those are not the caller's to give here:
/// they live in the `0x00134012`/`0x00134013` lists the entry indexes into, and the writer keeps
/// them from the entry it is rewriting. Taking the reader's type would ask for two fields and
/// silently ignore them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run {
    /// Where this run starts in `indices`.
    pub offset: usize,
    /// How many indices it covers — a multiple of three.
    pub count: usize,
}

/// Write one solid's mesh back.
///
/// `solid` is the header offset of a `0x80134010` chunk — the same number the chunk tree and
/// PryHUB's selection use, and the only stable name a solid has: 24.8% of them share their *name*
/// with another solid in the same file.
///
/// It rewrites the vertex buffer, the index buffer, the submesh runs, the two counts in the mesh
/// header and the bounding box in the solid's header — and then the file, through [`rebuild`], so
/// the per-solid directory stays true when the solid changes size.
///
/// Three things are computed rather than copied, each because the file computes them:
///
/// * **The bounding box is not the vertices' AABB.** It is the minimum minus 0.01 and the maximum
///   plus 0.01, in 23,292 of 23,299 real solids. Writing an AABB puts six floats 0.01 out.
/// * **The buffers are aligned by where they land**, and each differently: measured over 18,225
///   solids, the first vertex sits on a 128-byte file boundary 18,225 times, and the first index and
///   the first submesh entry on a 16-byte boundary 18,225 times each. The padding in front of each
///   buffer is therefore a function of the rebuilt file, not of the old one — so the file is built,
///   measured and built again until the three settle, which takes at most three passes because the
///   three buffers are in that order and each one's offset depends only on the ones before it.
/// * **The two anchors are opposite ends.** Vertices occupy the *last* `count × 36` bytes of their
///   buffer; indices and submesh entries start just past the *leading* `0x11` filler, with whatever
///   the file kept after them. Getting that backwards is the bug `geometry::index` warns about.
///
/// # Errors
/// - `solid` is not a `0x80134010` header, or it has no mesh.
/// - The attribute arrays disagree in length, the index count is not a multiple of three, or an
///   index is past the last vertex.
/// - `runs` has a different number of runs than the solid, or they do not tile the indices.
/// - The solid uses the packed vertex layout this crate does not decode (one solid in the install).
/// - The alignment did not settle, which would mean the file moves in a way this does not model.
pub fn replace_mesh(bytes: &[u8], solid: usize, mesh: &Mesh<'_>) -> NfsResult<Vec<u8>> {
    use super::format::{
        FILLER_BYTE, INDEX_BUFFER, MATERIAL_RANGES, MAT_RANGE_COUNT, MAT_RANGE_OFFSET,
        MAT_RANGE_STRIDE, MESH_HEADER, MESH_TRI_COUNT_FIELD, MESH_VERT_COUNT_FIELD, SOLID_HEADER,
        VERTEX_BUFFER,
    };

    let tree = ChunkNode::parse(bytes)?;
    let node = at(&tree, solid)
        .filter(|n| n.header.id == SOLID)
        .ok_or(NfsError::CorruptArchive { detail: "no solid at that offset" })?;
    let one = std::slice::from_ref(node);
    let (Some(header), Some(mh), Some(vb), Some(mr), Some(ib)) = (
        find(one, SOLID_HEADER),
        find(one, MESH_HEADER),
        find(one, VERTEX_BUFFER),
        find(one, MATERIAL_RANGES),
        find(one, INDEX_BUFFER),
    ) else {
        return Err(NfsError::CorruptArchive { detail: "that solid has no mesh" });
    };

    let md = mh.data(bytes);
    let filler = md.len() - super::skip_leading_filler(md).len();
    let word = |body: &[u8], i: usize| -> NfsResult<usize> {
        let o = i * 4;
        let b = body
            .get(o..o + 4)
            .ok_or(NfsError::BufferSizeMismatch { detail: "mesh header too short" })?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
    };
    let body = super::skip_leading_filler(md);
    let old_verts = word(body, MESH_VERT_COUNT_FIELD)?;
    if !super::standard_vertex_layout(old_verts, vb.data(bytes).len()) {
        return Err(NfsError::NotImplemented { feature: "packed vertex layout" });
    }

    // What the caller is asking for.
    let n = mesh.positions.len();
    if mesh.normals.len() != n || mesh.colours.len() != n || mesh.uvs.len() != n {
        return Err(NfsError::BufferSizeMismatch { detail: "attribute arrays disagree in length" });
    }
    if n > u32::MAX as usize || n > u16::MAX as usize + 1 {
        return Err(NfsError::BufferSizeMismatch { detail: "more vertices than a u16 index can name" });
    }
    if !mesh.indices.len().is_multiple_of(3) {
        return Err(NfsError::BufferSizeMismatch { detail: "the index count is not whole triangles" });
    }
    if mesh.indices.iter().any(|i| *i as usize >= n) {
        return Err(NfsError::CorruptArchive { detail: "an index is past the last vertex" });
    }
    let tris = mesh.indices.len() / 3;

    // The runs: the solid's own unless the caller replaced them, and they must tile the indices.
    let mrd = mr.data(bytes);
    let old_runs = mrd.len() / MAT_RANGE_STRIDE;
    let template_lead = mrd.len() - old_runs * MAT_RANGE_STRIDE;
    let kept: Vec<Run>;
    let runs: &[Run] = match mesh.runs {
        Some(r) => r,
        None => {
            // Keep the solid's own, read straight back out of the entries.
            kept = (0..old_runs)
                .map(|i| {
                    let base = template_lead + i * MAT_RANGE_STRIDE;
                    let g = |o: usize| -> usize {
                        mrd.get(base + o..base + o + 4)
                            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
                            .unwrap_or(0)
                    };
                    Run { count: g(MAT_RANGE_COUNT), offset: g(MAT_RANGE_OFFSET) }
                })
                .collect();
            &kept
        }
    };
    if runs.len() != old_runs {
        return Err(NfsError::NotImplemented { feature: "changing how many material runs a solid has" });
    }
    let mut walked = 0usize;
    for r in runs {
        if r.offset != walked {
            return Err(NfsError::CorruptArchive { detail: "material runs do not tile the indices" });
        }
        walked += r.count;
    }
    if walked != mesh.indices.len() {
        return Err(NfsError::CorruptArchive { detail: "material runs do not cover the indices" });
    }

    // The mesh header, with the counts the file will now state.
    let mut hout = md.to_vec();
    for (field, value) in
        [(MESH_TRI_COUNT_FIELD, tris), (MESH_VERT_COUNT_FIELD, n), (SUBMESH_COUNT_FIELD, runs.len())]
    {
        let o = filler + field * 4;
        hout.get_mut(o..o + 4)
            .ok_or(NfsError::BufferSizeMismatch { detail: "mesh header too short" })?
            .copy_from_slice(&(value as u32).to_le_bytes());
    }

    // The solid header, with the box the file's own rule produces.
    let mut sout = header.data(bytes).to_vec();
    let (min, max) = super::vertex::bounds(mesh.positions);
    for (base, values, sign) in [(0x20usize, min, -0.01f32), (0x30, max, 0.01)] {
        for (k, v) in values.iter().enumerate() {
            let o = base + k * 4;
            sout.get_mut(o..o + 4)
                .ok_or(NfsError::BufferSizeMismatch { detail: "solid header too short" })?
                .copy_from_slice(&(v + sign).to_le_bytes());
        }
    }

    // The three payloads, given the padding each needs. Sizes: the vertex buffer is lead + n*36; the
    // index buffer keeps the file's own tail rule (4-aligned); the submesh table is lead + runs*60.
    let old_ibuf = ib.data(bytes);
    let build = |vlead: usize, mlead: usize, ilead: usize| -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let mut v = vec![FILLER_BYTE; vlead];
        for i in 0..n {
            let (p, nr, c, uv) = (mesh.positions[i], mesh.normals[i], mesh.colours[i], mesh.uvs[i]);
            for f in [p[0], p[1], p[2], nr[0], nr[1], nr[2]] {
                v.extend_from_slice(&f.to_le_bytes());
            }
            v.extend_from_slice(&[c[2], c[1], c[0], c[3]]);
            for f in [uv[0], uv[1]] {
                v.extend_from_slice(&f.to_le_bytes());
            }
        }

        let mut m = vec![FILLER_BYTE; mlead];
        for (i, r) in runs.iter().enumerate() {
            // The thirteen words this crate does not decode are the template's; only the count and
            // the offset are the caller's.
            let base = template_lead + (i.min(old_runs.saturating_sub(1))) * MAT_RANGE_STRIDE;
            let mut entry = mrd
                .get(base..base + MAT_RANGE_STRIDE)
                .map(<[u8]>::to_vec)
                .unwrap_or_else(|| vec![0u8; MAT_RANGE_STRIDE]);
            entry[MAT_RANGE_COUNT..MAT_RANGE_COUNT + 4]
                .copy_from_slice(&(r.count as u32).to_le_bytes());
            entry[MAT_RANGE_OFFSET..MAT_RANGE_OFFSET + 4]
                .copy_from_slice(&(r.offset as u32).to_le_bytes());
            m.extend_from_slice(&entry);
        }

        let mut ix = vec![FILLER_BYTE; ilead];
        for i in mesh.indices {
            ix.extend_from_slice(&(*i as u16).to_le_bytes());
        }
        if tris * 3 == old_ibuf.len().saturating_sub(ilead) / 2 {
            // Unchanged length: keep whatever the file had after the indices, byte for byte.
            let end = ilead + tris * 6;
            if let Some(tail) = old_ibuf.get(end..) {
                ix.extend_from_slice(tail);
            }
        } else {
            while !ix.len().is_multiple_of(4) {
                ix.push(0);
            }
        }
        (v, m, ix)
    };

    // Build, look at where the buffers landed, and build again. The vertex buffer's offset does not
    // depend on any of the three pads, the submesh table's depends only on the vertex buffer's, and
    // the index buffer's on both — so three passes is the most this can need, and it is asserted
    // rather than assumed by looping until nothing moves.
    let (mut vlead, mut mlead, mut ilead) = (0usize, 0usize, 0usize);
    for _ in 0..4 {
        let (v, m, ix) = build(vlead, mlead, ilead);
        let mut edits = Edits::new();
        edits.insert(vb.offset, v);
        edits.insert(mr.offset, m);
        edits.insert(ib.offset, ix);
        edits.insert(mh.offset, hout.clone());
        edits.insert(header.offset, sout.clone());
        let out = rebuild(bytes, &edits)?;

        let after = ChunkNode::parse(&out)?;
        let solid_now = at(&after, offset_of(&after, solid, &tree)?)
            .ok_or(NfsError::CorruptArchive { detail: "the solid vanished from the rebuild" })?;
        let one = std::slice::from_ref(solid_now);
        let (Some(v2), Some(m2), Some(i2)) = (
            find(one, VERTEX_BUFFER),
            find(one, MATERIAL_RANGES),
            find(one, INDEX_BUFFER),
        ) else {
            return Err(NfsError::CorruptArchive { detail: "the rebuilt solid lost a mesh buffer" });
        };
        // The vertex data ends the buffer, so its start is `data_offset + lead`; the other two begin
        // just past their filler, so theirs is `data_offset + lead` too.
        let want_v = pad_to(v2.data_offset, 128);
        let want_m = pad_to(m2.data_offset, 16);
        let want_i = pad_to(i2.data_offset, 16);
        if (want_v, want_m, want_i) == (vlead, mlead, ilead) {
            return Ok(out);
        }
        vlead = want_v;
        mlead = want_m;
        ilead = want_i;
    }
    Err(NfsError::CorruptArchive { detail: "the mesh buffers' alignment did not settle" })
}

/// How many bytes of padding put `at + pad` on an `align` boundary.
const fn pad_to(at: usize, align: usize) -> usize {
    (align - at % align) % align
}

/// The solid's header offset in the rebuilt file: the same position among solids as before.
fn offset_of(after: &[ChunkNode], solid: usize, before: &[ChunkNode]) -> NfsResult<usize> {
    let old = solids(before);
    let new = solids(after);
    let i = old
        .iter()
        .position(|(o, _)| *o as usize == solid)
        .ok_or(NfsError::CorruptArchive { detail: "no solid at that offset" })?;
    new.get(i)
        .map(|(o, _)| *o as usize)
        .ok_or(NfsError::CorruptArchive { detail: "the rebuild lost a solid" })
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
