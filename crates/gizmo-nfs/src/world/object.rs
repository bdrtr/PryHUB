//! The city's meshes: vertices, indices and the runs that split them by texture.
//!
//! Where [`super::manifest`] answers "what is in here" without touching a buffer, this reads the
//! buffers. The two are the crate's usual cheap/expensive split — a region is up to 120 MB and
//! 11,135 objects, and most questions do not need a vertex.
//!
//! Almost nothing here is new. The city's records are the same ones `GEOMETRY.BIN` uses, so
//! [`crate::geometry`]'s readers do the work unchanged: the vertex record, the `u16` triangle list,
//! and the 60-byte run table. What this module adds is the three places the city is not a car.
//!
//! **The stride is matched exactly, not inferred.** [`crate::geometry::layout_for`] picks between
//! the two layouts by asking which *fits*, trying 36 first because "a buffer big enough for 36 is
//! not evidence of 24". That is the right rule for a car and the wrong one to lean on here, where
//! a buffer's size is known to the byte — so [`world_layout`] demands exact equality with one
//! stride or the other and reports anything else rather than quietly reading it as something.
//!
//! This module used to say something stronger and wrong: that *every* city buffer is exactly
//! `vertices × 24`, "in every solid of every region". Measured over all eight `STREAM*.BUN`, every
//! solid that declares vertices is at exactly one of two sizes — **28,672 at 24 B and 234 at 36 B**,
//! with no third size anywhere. The 234 are `XS_*`/`XO_*` street furniture: road signs, median
//! poles, chevrons, airport taxi signs. Demanding 24 returned an empty object for every one of
//! them, and because an empty object is skipped without comment by everything downstream, the
//! signs simply were not in the world. (The old claim was checked against L4RH, L4RR and L4RC —
//! three of the four bundles that contain no 36-byte solid at all, so the evidence could not have
//! contradicted it. The 234 live in L4RA, L4RD, L4RF and L4RB.)
//!
//! **Normals are in the file only for the wider record.** The 24-byte record has position, colour
//! and uv and stops; `parse_packed` returns zeroed normals rather than inventing any, and they are
//! derived here from the triangles, where the winding is — the same thing the car path does for its
//! packed solids. The 36-byte record carries real normals, and those are used as they are, because
//! a normal that was authored beats one re-derived from a triangle soup.
//!
//! **There is no shader.** A run's `+0x20` word is the `0x00134013` index, and `0x00134013` exists
//! on 3,440 of the city's 28,985 solids; 62,270 of 70,548 runs carry a `0xFF` sentinel instead. So
//! [`crate::types::NfsMaterialRange::shader`] comes back as `AssetHash(0)` for the city rather than
//! reporting a hash the file does not contain.

use crate::chunk::{walk_positions, Visit, WalkOptions};
use crate::error::{NfsError, NfsResult};
use crate::geometry::format::{
    INDEX_BUFFER, MATERIAL_LIST, MATERIAL_RANGES, MESH_HEADER, SOLID_HEADER, VERTEX_BUFFER,
    VERTEX_STRIDE,
};
use crate::geometry::index::parse_indices;
use crate::geometry::material::{material_ranges, ordered_hashes};
use crate::geometry::vertex::{normals_from_triangles, parse_vertices_with, Layout};
use crate::geometry::{mesh_field, skip_leading_filler, PACKED_VERTEX_STRIDE};
use crate::types::{AssetHash, NfsMaterialRange};

use super::header::{read_header, WorldSolidHeader};
use super::manifest::{MESH_TRIANGLES, MESH_VERTICES};

/// One city object with its geometry read.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct WorldMesh {
    /// The `0x00134011` record: name, key, bounding box and placement.
    pub header: WorldSolidHeader,
    /// Positions, in the file's own frame (Z-up) and, for a road or terrain chunk, already in
    /// world space — see [`WorldSolidHeader::is_placed`].
    pub positions: Vec<[f32; 3]>,
    /// The file's own for a solid at the 36-byte stride; derived from the triangle winding for one
    /// at the 24-byte stride, whose record carries none. See [`world_layout`].
    pub normals: Vec<[f32; 3]>,
    /// Per-vertex RGBA8. For the city this is the baked lighting: a static world stores its light
    /// here rather than recomputing it, which is why the scenery needs no normal to look right.
    pub colours: Vec<[u8; 4]>,
    pub uvs: Vec<[f32; 2]>,
    /// Triangle-list indices.
    pub indices: Vec<u32>,
    /// The runs that split [`Self::indices`] by texture. Each names its texture by *position* in
    /// [`Self::texture_slots`], which is why that list keeps file order.
    pub groups: Vec<NfsMaterialRange>,
    /// The solid's `0x00134012` list, as [`super::WorldObjectInfo::texture_slots`] reports it.
    pub texture_slots: Vec<AssetHash>,
}

impl WorldMesh {
    /// Triangles, i.e. `indices.len() / 3`.
    #[must_use]
    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }
}

/// Read every object's geometry from a bundle.
///
/// **This is the expensive half.** `STREAML4RD` is 11,135 objects and 1.37 M vertices; expanded to
/// parallel `f32` arrays that is on the order of 50 MB before indices. Use [`super::manifest`] for
/// anything that does not need the buffers.
///
/// An object whose buffers are missing or unreadable is returned with empty arrays rather than
/// dropped, so the result still lines up one-to-one with the manifest's — a reader that silently
/// loses objects is one that cannot be checked against a chunk census.
///
/// # Errors
///
/// Propagates the walk. Per-object failures do not fail the bundle.
pub fn meshes(bundle: &[u8]) -> NfsResult<Vec<WorldMesh>> {
    // Payloads are gathered per solid as **ranges** and decoded when the next solid starts: a run
    // table cannot be read before the index buffer it splits, and a leaf's slice carries the
    // callback's lifetime rather than the bundle's — which is what `walk_positions` exists for.
    #[derive(Default)]
    struct Pending {
        header: Option<WorldSolidHeader>,
        slots: Vec<AssetHash>,
        mesh_header: (usize, usize),
        vbuf: (usize, usize),
        ibuf: (usize, usize),
        runs: (usize, usize),
    }

    let mut done: Vec<Pending> = Vec::new();

    walk_positions(
        bundle,
        WalkOptions::default(),
        |_, _| Ok(Visit::Descend),
        |_, chunk_header, at, data| {
            let span = (at, at + data.len());
            match chunk_header.id {
                SOLID_HEADER => {
                    done.push(Pending { header: Some(read_header(data)?), ..Default::default() });
                }
                MATERIAL_LIST => {
                    if let Some(p) = done.last_mut() {
                        p.slots = ordered_hashes(data);
                    }
                }
                MESH_HEADER => {
                    if let Some(p) = done.last_mut() {
                        p.mesh_header = span;
                    }
                }
                VERTEX_BUFFER => {
                    if let Some(p) = done.last_mut() {
                        p.vbuf = span;
                    }
                }
                INDEX_BUFFER => {
                    if let Some(p) = done.last_mut() {
                        p.ibuf = span;
                    }
                }
                MATERIAL_RANGES => {
                    if let Some(p) = done.last_mut() {
                        p.runs = span;
                    }
                }
                _ => {}
            }
            Ok(())
        },
    )?;

    let at = |(from, to): (usize, usize)| bundle.get(from..to).unwrap_or_default();
    Ok(done
        .into_iter()
        .filter_map(|p| {
            let header = p.header?;
            Some(build(&header, &p.slots, at(p.mesh_header), at(p.vbuf), at(p.ibuf), at(p.runs)))
        })
        .collect())
}

/// Turn one solid's payloads into a mesh, or into an empty one if they do not hold together.
fn build(
    header: &WorldSolidHeader,
    slots: &[AssetHash],
    mesh_header: &[u8],
    vbuf: &[u8],
    ibuf: &[u8],
    runs: &[u8],
) -> WorldMesh {
    let empty = || WorldMesh {
        header: header.clone(),
        positions: Vec::new(),
        normals: Vec::new(),
        colours: Vec::new(),
        uvs: Vec::new(),
        indices: Vec::new(),
        groups: Vec::new(),
        texture_slots: slots.to_vec(),
    };

    let counters = skip_leading_filler(mesh_header);
    let (Ok(tris), Ok(verts)) =
        (mesh_field(counters, MESH_TRIANGLES), mesh_field(counters, MESH_VERTICES))
    else {
        return empty();
    };
    let (tris, verts) = (tris as usize, verts as usize);
    if verts == 0 {
        return empty();
    }

    let body = skip_leading_filler(vbuf);
    let Ok(layout) = world_layout(verts, vbuf) else { return empty() };

    let Ok((positions, file_normals, colours, uvs)) = parse_vertices_with(body, verts, layout)
    else {
        return empty();
    };
    let Ok(indices) = parse_indices(skip_leading_filler(ibuf), tris, verts) else {
        return empty();
    };
    // Only the packed record has no normal to read, and then the winding supplies one.
    let normals = match layout {
        Layout::Standard => file_normals,
        Layout::Packed => normals_from_triangles(&positions, &indices),
    };
    // No shaders in the city, so nothing is handed one: `material_ranges` dereferences the shader
    // list by index and an empty list gives every run `AssetHash(0)`, which is the honest answer.
    let groups = material_ranges(skip_leading_filler(runs), slots, &[], indices.len());

    WorldMesh {
        header: header.clone(),
        positions,
        normals,
        colours,
        uvs,
        indices,
        groups,
        texture_slots: slots.to_vec(),
    }
}

/// Which vertex layout a city solid's buffer is, by exact size.
///
/// This is the check that decides whether [`meshes`] can read a solid: anything it rejects comes
/// back as an object with empty arrays. It asks for **exact** equality with one stride or the
/// other rather than using [`crate::geometry::layout_for`], which asks which layout a buffer is
/// merely large enough for — on a 24-byte buffer carrying trailing slack that would answer 36 and
/// read the wrong record.
///
/// Both layouts are real here. Across all eight `STREAM*.BUN`, 28,672 solids are `vertices × 24`
/// and 234 are `vertices × 36`; nothing is anything else.
///
/// # Errors
///
/// [`NfsError::BufferSizeMismatch`] if the buffer is neither stride at the declared count, or if
/// the count times a stride overflows.
pub fn world_layout(vertices: usize, vbuf: &[u8]) -> NfsResult<Layout> {
    let body = skip_leading_filler(vbuf);
    let fits = |stride: usize| vertices.checked_mul(stride).is_some_and(|n| n == body.len());
    if fits(PACKED_VERTEX_STRIDE) {
        Ok(Layout::Packed)
    } else if fits(VERTEX_STRIDE) {
        Ok(Layout::Standard)
    } else {
        Err(NfsError::BufferSizeMismatch {
            detail: "world vertex buffer is not the declared count at either the 24- or 36-byte stride",
        })
    }
}

/// Whether a solid's vertex buffer is the city's 24-byte layout at the declared count.
///
/// The narrow question, kept because "is this the packed record" is worth being able to ask on its
/// own. It is **not** the readability check — a solid at the 36-byte stride is perfectly readable
/// and this returns an error for it. Use [`world_layout`] for that.
///
/// # Errors
///
/// [`NfsError::BufferSizeMismatch`] if the count and the buffer cannot both be true.
pub fn check_packed_stride(vertices: usize, vbuf: &[u8]) -> NfsResult<()> {
    let body = skip_leading_filler(vbuf);
    let needed = vertices
        .checked_mul(PACKED_VERTEX_STRIDE)
        .ok_or(NfsError::BufferSizeMismatch { detail: "world vertex count overflow" })?;
    if body.len() == needed {
        Ok(())
    } else {
        Err(NfsError::BufferSizeMismatch {
            detail: "world vertex buffer is not the declared count at the 24-byte stride",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::IDENTITY;

    fn chunk(id: u32, payload: &[u8]) -> Vec<u8> {
        let mut v = id.to_le_bytes().to_vec();
        v.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        v.extend_from_slice(payload);
        v
    }

    fn solid_header(name: &str) -> Vec<u8> {
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
        body
    }

    fn mesh_header(tris: u32, verts: u32) -> Vec<u8> {
        let mut b = vec![0u8; 68];
        b[MESH_TRIANGLES * 4..MESH_TRIANGLES * 4 + 4].copy_from_slice(&tris.to_le_bytes());
        b[MESH_VERTICES * 4..MESH_VERTICES * 4 + 4].copy_from_slice(&verts.to_le_bytes());
        b
    }

    /// Three vertices of the 24-byte record: pos f32[3], BGRA u32, uv f32[2].
    fn vbuf(filler: usize) -> Vec<u8> {
        let mut v = vec![0x11u8; filler];
        let verts: [([f32; 3], [u8; 4], [f32; 2]); 3] = [
            ([0.0, 0.0, 0.0], [10, 20, 30, 255], [0.0, 0.0]),
            ([1.0, 0.0, 0.0], [11, 21, 31, 255], [1.0, 0.0]),
            ([0.0, 1.0, 0.0], [12, 22, 32, 255], [0.0, 1.0]),
        ];
        for (p, c, uv) in verts {
            for f in p {
                v.extend_from_slice(&f.to_le_bytes());
            }
            v.extend_from_slice(&c); // stored B,G,R,A
            for f in uv {
                v.extend_from_slice(&f.to_le_bytes());
            }
        }
        v
    }

    fn ibuf() -> Vec<u8> {
        [0u16, 1, 2].iter().flat_map(|i| i.to_le_bytes()).collect()
    }

    /// One 60-byte run covering all three indices, naming slot `slot`.
    fn runs(index_count: u32, slot: u32) -> Vec<u8> {
        use crate::geometry::format::{MAT_RANGE_COUNT, MAT_RANGE_MATIDX, MAT_RANGE_OFFSET};
        let mut r = vec![0u8; 60];
        r[MAT_RANGE_COUNT..MAT_RANGE_COUNT + 4].copy_from_slice(&index_count.to_le_bytes());
        r[MAT_RANGE_MATIDX..MAT_RANGE_MATIDX + 4].copy_from_slice(&slot.to_le_bytes());
        r[MAT_RANGE_OFFSET..MAT_RANGE_OFFSET + 4].copy_from_slice(&0u32.to_le_bytes());
        r
    }

    fn slots(keys: &[u32]) -> Vec<u8> {
        keys.iter()
            .flat_map(|k| k.to_le_bytes().into_iter().chain(0u32.to_le_bytes()))
            .collect()
    }

    fn solid(name: &str, filler: usize, keys: &[u32]) -> Vec<u8> {
        let mut inner = chunk(SOLID_HEADER, &solid_header(name));
        inner.extend_from_slice(&chunk(MATERIAL_LIST, &slots(keys)));
        let mut geom = chunk(MESH_HEADER, &mesh_header(1, 3));
        geom.extend_from_slice(&chunk(VERTEX_BUFFER, &vbuf(filler)));
        geom.extend_from_slice(&chunk(MATERIAL_RANGES, &runs(3, 0)));
        geom.extend_from_slice(&chunk(INDEX_BUFFER, &ibuf()));
        inner.extend_from_slice(&chunk(0x8013_4100, &geom));
        chunk(0x8013_4010, &inner)
    }

    /// The same three vertices at the 36-byte record: pos, normal, BGRA, uv.
    ///
    /// The normals are `+Y`, which the winding of this triangle could never produce — it lies in
    /// the XY plane, so a derived normal is `±Z`. That is what lets a test tell "read from the
    /// file" apart from "re-derived".
    fn vbuf_wide(filler: usize) -> Vec<u8> {
        /// One wide vertex as a test writes it: position, normal, BGRA, uv.
        type WideVertex = ([f32; 3], [f32; 3], [u8; 4], [f32; 2]);

        let mut v = vec![0x11u8; filler];
        let verts: [WideVertex; 3] = [
            ([0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [10, 20, 30, 255], [0.0, 0.0]),
            ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [11, 21, 31, 255], [1.0, 0.0]),
            ([0.0, 1.0, 0.0], [0.0, 1.0, 0.0], [12, 22, 32, 255], [0.0, 1.0]),
        ];
        for (p, n, c, uv) in verts {
            for f in p.into_iter().chain(n) {
                v.extend_from_slice(&f.to_le_bytes());
            }
            v.extend_from_slice(&c); // stored B,G,R,A
            for f in uv {
                v.extend_from_slice(&f.to_le_bytes());
            }
        }
        v
    }

    /// A solid whose vertex buffer is the wide record — the `XS_*`/`XO_*` street-furniture shape.
    fn solid_wide(name: &str, filler: usize, keys: &[u32]) -> Vec<u8> {
        let mut inner = chunk(SOLID_HEADER, &solid_header(name));
        inner.extend_from_slice(&chunk(MATERIAL_LIST, &slots(keys)));
        let mut geom = chunk(MESH_HEADER, &mesh_header(1, 3));
        geom.extend_from_slice(&chunk(VERTEX_BUFFER, &vbuf_wide(filler)));
        geom.extend_from_slice(&chunk(MATERIAL_RANGES, &runs(3, 0)));
        geom.extend_from_slice(&chunk(INDEX_BUFFER, &ibuf()));
        inner.extend_from_slice(&chunk(0x8013_4100, &geom));
        chunk(0x8013_4010, &inner)
    }

    #[test]
    fn a_city_solid_reads_at_the_packed_stride_whatever_its_filler() {
        for filler in [0usize, 4, 8, 12] {
            let bundle = chunk(0x8013_4000, &solid("TRN_ROADA", filler, &[0xAABB_CCDD]));
            let m = meshes(&bundle).unwrap();
            assert_eq!(m.len(), 1, "{filler}: one object");
            let mesh = &m[0];
            assert_eq!(mesh.positions.len(), 3, "{filler}: vertices");
            assert_eq!(mesh.triangles(), 1, "{filler}: triangles");
            assert_eq!(mesh.positions[1], [1.0, 0.0, 0.0], "{filler}: position");
            assert_eq!(mesh.uvs[2], [0.0, 1.0], "{filler}: uv");
            // B,G,R,A on disk comes back R,G,B,A.
            assert_eq!(mesh.colours[0], [30, 20, 10, 255], "{filler}: colour order");
            assert_eq!(mesh.header.name, "TRN_ROADA");
        }
    }

    /// The record has no normal, so one is derived from the winding rather than left at zero.
    #[test]
    fn normals_are_derived_because_the_record_has_none() {
        let bundle = chunk(0x8013_4000, &solid("TRN_TERRAINA", 8, &[1]));
        let m = meshes(&bundle).unwrap();
        assert_eq!(m[0].normals.len(), 3);
        // The triangle lies in the XY plane, so every normal is ±Z and none is zero.
        for n in &m[0].normals {
            assert!((n[2].abs() - 1.0).abs() < 1e-5, "{n:?} is not a unit Z normal");
        }
    }

    /// A run names its texture by position in the slot list — the link the city uses.
    #[test]
    fn a_run_names_its_texture_by_position_in_the_slot_list() {
        let keys = [0x1111_1111u32, 0x2222_2222];
        let bundle = chunk(0x8013_4000, &solid("OBJ_PYLON", 0, &keys));
        let m = meshes(&bundle).unwrap();
        assert_eq!(m[0].texture_slots.iter().map(|h| h.0).collect::<Vec<_>>(), keys);
        assert_eq!(m[0].groups.len(), 1);
        // Slot 0 was named, so the run resolves to the first key and not to whatever came first.
        assert_eq!(m[0].groups[0].hash.0, keys[0]);
        assert_eq!(m[0].groups[0].index_count, 3);
        // The city ships no shaders, so none is claimed.
        assert_eq!(m[0].groups[0].shader, AssetHash(0));
    }

    /// A buffer that is not the declared count at 24 bytes is reported, not read as something else
    /// — the one thing `layout_for` would do differently by trying 36 first.
    #[test]
    fn a_buffer_that_is_not_the_declared_count_is_refused() {
        assert!(check_packed_stride(3, &vbuf(8)).is_ok());
        assert!(check_packed_stride(4, &vbuf(8)).is_err(), "too few bytes for four vertices");
        assert!(check_packed_stride(2, &vbuf(8)).is_err(), "slack is not silently ignored");
        assert!(check_packed_stride(usize::MAX, &vbuf(0)).is_err(), "overflow is caught");
    }

    /// The city ships **both** records. 234 of its 28,985 solids are the 36-byte one — the
    /// `XS_*`/`XO_*` signs, poles and chevrons — and demanding 24 returned an empty object for
    /// every one of them, which everything downstream then skipped without a word.
    #[test]
    fn a_city_solid_at_the_wide_stride_is_read_rather_than_refused() {
        for filler in [0usize, 4, 8, 12] {
            let bundle = chunk(0x8013_4000, &solid_wide("XS_WARNWINDINGNP_1A_00", filler, &[3]));
            let m = meshes(&bundle).unwrap();
            assert_eq!(m.len(), 1, "{filler}: one object");
            let mesh = &m[0];
            assert_eq!(mesh.positions.len(), 3, "{filler}: the wide record is read, not refused");
            assert_eq!(mesh.triangles(), 1, "{filler}: triangles");
            assert_eq!(mesh.positions[1], [1.0, 0.0, 0.0], "{filler}: position");
            assert_eq!(mesh.uvs[2], [0.0, 1.0], "{filler}: uv");
            assert_eq!(mesh.colours[0], [30, 20, 10, 255], "{filler}: colour order");
        }
    }

    /// And the wide record's normals are the file's, not re-derived: the triangle lies in the XY
    /// plane, so anything derived from its winding is ±Z, and the file says +Y.
    #[test]
    fn the_wide_record_keeps_the_normals_the_file_carries() {
        let bundle = chunk(0x8013_4000, &solid_wide("XO_SIGN", 8, &[1]));
        let m = meshes(&bundle).unwrap();
        assert_eq!(m[0].normals, vec![[0.0, 1.0, 0.0]; 3], "the file's normals, not the winding's");
    }

    #[test]
    fn world_layout_names_both_strides_and_refuses_anything_else() {
        // `vbuf` is three 24-byte vertices: 72 bytes once its filler is skipped.
        assert_eq!(world_layout(3, &vbuf(8)).unwrap(), Layout::Packed);
        assert_eq!(world_layout(3, &vbuf_wide(8)).unwrap(), Layout::Standard);
        assert!(world_layout(4, &vbuf(8)).is_err(), "72 bytes is neither 4×24 nor 4×36");
        assert!(world_layout(1, &vbuf(8)).is_err(), "slack is not silently ignored");
        // 72 bytes is 3 × 24 **and** 2 × 36, so the declared count is the only thing separating
        // them — which is why this matches exactly instead of asking which layout merely fits.
        assert_eq!(world_layout(2, &vbuf(8)).unwrap(), Layout::Standard);
        assert!(world_layout(usize::MAX, &vbuf(0)).is_err(), "overflow is caught");
    }

    /// An object whose geometry does not hold together still comes back, so the result lines up
    /// with the manifest's one-to-one.
    #[test]
    fn an_object_with_no_geometry_is_kept_with_empty_arrays() {
        let bundle = chunk(0x8013_4010, &chunk(SOLID_HEADER, &solid_header("ZCS_MARKER")));
        let m = meshes(&bundle).unwrap();
        assert_eq!(m.len(), 1);
        assert!(m[0].positions.is_empty());
        assert_eq!(m[0].triangles(), 0);
        assert_eq!(m[0].header.name, "ZCS_MARKER");
    }

    #[test]
    fn objects_come_back_in_file_order() {
        let mut bundle = solid("A_FIRST", 0, &[1]);
        bundle.extend_from_slice(&solid("B_SECOND", 8, &[2]));
        let m = meshes(&bundle).unwrap();
        assert_eq!(
            m.iter().map(|o| o.header.name.as_str()).collect::<Vec<_>>(),
            ["A_FIRST", "B_SECOND"]
        );
    }
}
