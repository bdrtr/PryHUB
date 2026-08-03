//! Every object a bundle declares, without decoding one vertex.
//!
//! A `STREAM*.BUN` is up to 120 MB and `STREAML4RA` holds 10,735 objects. Reading all of it to
//! answer "what is in here" is the wrong shape, and it is a shape this crate has been in before:
//! [`crate::texture::Tpk::parse`] against `Tpk::directory` exists because a car's pack is 8.7 MB and
//! a `VINYLS.BIN` is 1.87 GB. This is the same split for the city.
//!
//! What makes it cheap is where the counters live. Vertex and triangle counts are in the 68-byte
//! `0x00134900` mesh header, not in the buffers — so a manifest descends into `0x80134100`, reads
//! two `u32`s, and never touches the `0x00134B01` payload that holds all the weight. The walk is
//! [`crate::chunk::walk`], which allocates nothing per chunk; [`crate::chunk::ChunkNode::parse`]
//! would materialise a node per chunk, and L4RA has on the order of 100,000 of them.
//!
//! Both counter fields are locked against the file rather than assumed. Field 13 is the vertex
//! count: it equals `vertex_buffer_bytes` / [`PACKED_VERTEX_STRIDE`] in 175 of 175 L4RH solids,
//! 482 of 482 in L4RR and 772 of 772 in L4RC — the field pinned by a stride this crate had already
//! settled independently, each confirming the other.
//! Field 9 is the triangle count: `index_bytes − triangles × 6` comes out as 0 or 2 and never
//! negative (99 and 76 of L4RH's 175), the 2 being the pad that takes an odd triangle count's
//! `× 6` up to a 4-byte boundary.

use crate::chunk::{walk, Visit};
use crate::error::NfsResult;
use crate::geometry::format::{MESH_HEADER, SOLID_HEADER};
use crate::geometry::{mesh_field, skip_leading_filler, PACKED_VERTEX_STRIDE};

use super::header::{read_header, WorldSolidHeader};

/// u32 field index of the triangle count within a filler-stripped `0x00134900`.
pub const MESH_TRIANGLES: usize = 9;

/// u32 field index of the vertex count.
pub const MESH_VERTICES: usize = 13;

/// One object as the bundle declares it: identity, extent, placement and size, no geometry.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct WorldObjectInfo {
    /// The `0x00134011` record.
    pub header: WorldSolidHeader,
    /// Leading `0x11` filler on that record, in bytes — 0, 4, 8 or 12. Reported because it is the
    /// thing that silently breaks a reader, so a caller can see the distribution rather than
    /// trust that it was handled.
    pub filler: usize,
    /// Vertex count from the mesh header. `0` if the object declared no mesh.
    pub vertices: u32,
    /// Triangle count from the mesh header.
    pub triangles: u32,
}

impl WorldObjectInfo {
    /// Bytes this object's vertices occupy on disk.
    ///
    /// At [`PACKED_VERTEX_STRIDE`], which is the city's layout: position, BGRA colour, uv and no
    /// normal — a static world carries its lighting in that colour. This crate already settled
    /// that on `STREAML4RA.BUN` (1,924 of 1,925 buffers divide by 24); the manifest re-confirms it
    /// from the other side, since field 13 equals `buffer_bytes / 24` in every solid of L4RH,
    /// L4RR and L4RC.
    #[must_use]
    pub fn vertex_bytes(&self) -> u64 {
        u64::from(self.vertices) * PACKED_VERTEX_STRIDE as u64
    }
}

/// Read every object a bundle declares.
///
/// Objects come back in file order. An object with a `0x00134011` but no `0x00134900` is still
/// reported, with zero counts, rather than dropped — a bundle holds skydome and marker objects and
/// a manifest that quietly loses them is a manifest that cannot be checked against a chunk census.
///
/// # Errors
///
/// Propagates [`crate::NfsError`] from the walk. Note that a `STREAM*.BUN` is sector-padded, so the
/// walk resyncs past the gaps; an early stop is not the end of the file and this does not treat it
/// as one.
pub fn manifest(bundle: &[u8]) -> NfsResult<Vec<WorldObjectInfo>> {
    let mut objects: Vec<WorldObjectInfo> = Vec::new();

    walk(
        bundle,
        // Everything is worth descending into: the counters are two levels down and the cost of
        // reaching them is a header read, not a payload read.
        |_, _| Ok(Visit::Descend),
        |_, header, data| {
            match header.id {
                SOLID_HEADER => {
                    // One `0x00134011` per object, and it precedes that object's mesh header, so
                    // its arrival is what starts a new record.
                    let hdr = read_header(data)?;
                    objects.push(WorldObjectInfo {
                        header: hdr,
                        filler: data.len() - skip_leading_filler(data).len(),
                        vertices: 0,
                        triangles: 0,
                    });
                }
                MESH_HEADER => {
                    if let Some(last) = objects.last_mut() {
                        let body = skip_leading_filler(data);
                        last.triangles = mesh_field(body, MESH_TRIANGLES).unwrap_or(0);
                        last.vertices = mesh_field(body, MESH_VERTICES).unwrap_or(0);
                    }
                }
                _ => {}
            }
            Ok(())
        },
    )?;

    Ok(objects)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::IDENTITY;

    /// Wrap `payload` in an 8-byte chunk header.
    fn chunk(id: u32, payload: &[u8]) -> Vec<u8> {
        let mut v = id.to_le_bytes().to_vec();
        v.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        v.extend_from_slice(payload);
        v
    }

    /// A `0x00134011` payload: `filler` bytes of `0x11`, a 192-byte body, `name` 28 from the end.
    fn solid_header(name: &str, filler: usize) -> Vec<u8> {
        let mut body = vec![0u8; 192];
        body[0x10..0x14].copy_from_slice(&crate::hash::string_hash(name).to_le_bytes());
        let mut at = 64;
        for row in &IDENTITY {
            for cell in row {
                body[at..at + 4].copy_from_slice(&cell.to_le_bytes());
                at += 4;
            }
        }
        let name_at = 192 - 28;
        let fits = name.len().min(28);
        body[name_at..name_at + fits].copy_from_slice(&name.as_bytes()[..fits]);
        let mut p = vec![0x11u8; filler];
        p.extend_from_slice(&body);
        p
    }

    /// A `0x00134900` payload carrying `tris` and `verts` at their measured field indices.
    fn mesh_header(tris: u32, verts: u32, filler: usize) -> Vec<u8> {
        let mut body = vec![0u8; 68];
        body[MESH_TRIANGLES * 4..MESH_TRIANGLES * 4 + 4].copy_from_slice(&tris.to_le_bytes());
        body[MESH_VERTICES * 4..MESH_VERTICES * 4 + 4].copy_from_slice(&verts.to_le_bytes());
        let mut p = vec![0x11u8; filler];
        p.extend_from_slice(&body);
        p
    }

    /// One object, built the way a bundle nests it: solid → header + (geometry → mesh header).
    fn solid(name: &str, filler: usize, tris: u32, verts: u32) -> Vec<u8> {
        let mut inner = chunk(SOLID_HEADER, &solid_header(name, filler));
        inner.extend_from_slice(&chunk(0x8013_4100, &chunk(MESH_HEADER, &mesh_header(tris, verts, filler))));
        chunk(0x8013_4010, &inner)
    }

    #[test]
    fn counts_come_from_the_mesh_header_at_every_filler_width() {
        for filler in [0usize, 4, 8, 12] {
            let bundle = chunk(0x8013_4000, &solid("TRN_GRASSC", filler, 40, 100));
            let m = manifest(&bundle).unwrap();
            assert_eq!(m.len(), 1, "{filler}: one object");
            assert_eq!((m[0].triangles, m[0].vertices), (40, 100), "{filler}: counts");
            assert_eq!(m[0].filler, filler, "{filler}: filler reported");
            assert_eq!(m[0].vertex_bytes(), 100 * 24, "{filler}: vertex bytes at stride 24");
            assert_eq!(m[0].header.name, "TRN_GRASSC");
        }
    }

    #[test]
    fn objects_come_back_in_file_order_and_keep_their_own_counts() {
        let mut bundle = solid("OBJ_PYLON", 0, 2, 8);
        bundle.extend_from_slice(&solid("TRN_TERRAINA", 8, 300, 700));
        let m = manifest(&bundle).unwrap();
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].header.name, "OBJ_PYLON");
        assert_eq!((m[0].triangles, m[0].vertices), (2, 8));
        assert_eq!(m[1].header.name, "TRN_TERRAINA");
        assert_eq!((m[1].triangles, m[1].vertices), (300, 700));
    }

    /// A header with no mesh is still an object — losing it would make the manifest disagree with
    /// a chunk census of the same file.
    #[test]
    fn an_object_without_a_mesh_is_reported_with_zero_counts() {
        let bundle = chunk(0x8013_4010, &chunk(SOLID_HEADER, &solid_header("ZCS_MARKER", 4)));
        let m = manifest(&bundle).unwrap();
        assert_eq!(m.len(), 1);
        assert_eq!((m[0].triangles, m[0].vertices), (0, 0));
        assert_eq!(m[0].vertex_bytes(), 0);
    }
}
