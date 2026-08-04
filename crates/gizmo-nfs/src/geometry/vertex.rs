//! The `0x00134B01` vertex buffer: two record layouts, told apart by size.
//!
//! | stride | contents | where |
//! |---|---|---|
//! | 36 | position, **normal**, BGRA colour, uv | cars |
//! | 24 | position, BGRA colour, uv | the city, and a few car solids |
//!
//! The 24-byte one has **no normal**, which is what a static world with its lighting baked into
//! per-vertex colour does not need. It was previously unread: a solid whose buffer was too small
//! for stride 36 was skipped, which is why `STREAM*.BUN` — the whole of Bayview — decoded to
//! nothing, and why a handful of car solids (`3000GT_KIT00_ENGINE_B` and its kin) went missing.
//!
//! How the layout was settled, on `STREAML4RA.BUN`: **1,924 of 1,925** vertex buffers divide
//! exactly by 24 after their `0x11` alignment padding; positions then span −8,960 to +8,010, which
//! is city scale; the colour word's top byte is `0xFF` in 3,972 of ~4,000 samples, which is an
//! opaque alpha; and every sampled uv lands in a sensible range. The first four vertices of a
//! wall piece come out as a clean rectangle — `x 9.221 ↔ −9.238`, `z −0.035 ↔ 8.923`, `y` constant.

use super::format::VERTEX_STRIDE;
use crate::error::{NfsError, NfsResult};

/// The packed stride: position + colour + uv, with the normal left out.
pub const PACKED_VERTEX_STRIDE: usize = 24;

/// Which record layout a solid's vertex buffer holds, or `None` when it is too small for either.
///
/// Chosen by **size**, because the file does not say: the mesh header gives a vertex count and the
/// buffer gives a length, and the stride is whichever of the two fits. The standard layout is
/// tried first — a buffer big enough for 36 is not evidence of 24.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// 36 bytes: position, normal, colour, uv.
    Standard,
    /// 24 bytes: position, colour, uv. No normal.
    Packed,
}

impl Layout {
    /// Bytes per vertex.
    #[must_use]
    pub fn stride(self) -> usize {
        match self {
            Self::Standard => VERTEX_STRIDE,
            Self::Packed => PACKED_VERTEX_STRIDE,
        }
    }
}

/// Pick the layout a buffer of `vbuf_len` bytes can hold `vert_count` vertices in.
#[must_use]
pub fn layout_for(vert_count: usize, vbuf_len: usize) -> Option<Layout> {
    if vert_count.saturating_mul(VERTEX_STRIDE) <= vbuf_len {
        Some(Layout::Standard)
    } else if vert_count.saturating_mul(PACKED_VERTEX_STRIDE) <= vbuf_len {
        Some(Layout::Packed)
    } else {
        None
    }
}

/// Whether a solid's vertex buffer is big enough for the standard 36-byte ([`VERTEX_STRIDE`])
/// layout.
pub fn standard_vertex_layout(vert_count: usize, vbuf_len: usize) -> bool {
    vert_count.saturating_mul(VERTEX_STRIDE) <= vbuf_len
}

/// The vertices occupy the last `count * stride` bytes of the buffer (leading bytes are
/// alignment padding). Returns parallel position/normal/colour/uv arrays.
///
/// The packed layout returns **zeroed normals**: the record has none, and inventing one per vertex
/// here would be a claim this file cannot support. Callers that need them derive them from the
/// triangles, where the winding is.
#[allow(clippy::type_complexity)]
pub(crate) fn parse_vertices_with(
    vbuf: &[u8],
    count: usize,
    layout: Layout,
) -> NfsResult<(Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<[u8; 4]>, Vec<[f32; 2]>)> {
    if layout == Layout::Packed {
        return parse_packed(vbuf, count);
    }
    let needed = count
        .checked_mul(VERTEX_STRIDE)
        .ok_or(NfsError::BufferSizeMismatch { detail: "vertex count overflow" })?;
    if needed > vbuf.len() {
        return Err(NfsError::BufferSizeMismatch { detail: "vertex buffer smaller than count*stride" });
    }
    let start = vbuf.len() - needed;
    let mut positions = Vec::with_capacity(count);
    let mut normals = Vec::with_capacity(count);
    let mut colours = Vec::with_capacity(count);
    let mut uvs = Vec::with_capacity(count);
    // One record at a time as a fixed-size array. The range was bounds-checked once above, so
    // reading through a cursor would pay for nine more checks per vertex — 134,000 vertices in a car
    // and it showed: this loop was two thirds of the geometry pass. Copying the record into
    // `[u8; 36]` also means every index below is provably in range, so there is no panic path and
    // no check left to elide.
    let records = vbuf.get(start..start + needed).unwrap_or_default();
    for record in records.chunks_exact(VERTEX_STRIDE) {
        let Ok(v) = <[u8; VERTEX_STRIDE]>::try_from(record) else { continue };
        let f = |o: usize| f32::from_le_bytes([v[o], v[o + 1], v[o + 2], v[o + 3]]);
        positions.push([f(0), f(4), f(8)]);
        normals.push([f(12), f(16), f(20)]);
        // 24: a per-vertex colour, stored B,G,R,A and handed back R,G,B,A. It was read as nothing
        // here — "a constant sentinel (~-1.7e38)" — and the sentinel was `0xFF000000` as an `f32`.
        // See `NfsMeshPart::colours` for what the install says and what is still only convention.
        colours.push([v[26], v[25], v[24], v[27]]);
        uvs.push([f(28), f(32)]);
    }
    Ok((positions, normals, colours, uvs))
}

/// The 24-byte record: position, BGRA colour, uv — and no normal.
#[allow(clippy::type_complexity)]
fn parse_packed(
    vbuf: &[u8],
    count: usize,
) -> NfsResult<(Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<[u8; 4]>, Vec<[f32; 2]>)> {
    let needed = count
        .checked_mul(PACKED_VERTEX_STRIDE)
        .ok_or(NfsError::BufferSizeMismatch { detail: "vertex count overflow" })?;
    if needed > vbuf.len() {
        return Err(NfsError::BufferSizeMismatch {
            detail: "vertex buffer smaller than count*packed stride",
        });
    }
    let start = vbuf.len() - needed;
    let mut positions = Vec::with_capacity(count);
    let mut colours = Vec::with_capacity(count);
    let mut uvs = Vec::with_capacity(count);
    let records = vbuf.get(start..start + needed).unwrap_or_default();
    for record in records.chunks_exact(PACKED_VERTEX_STRIDE) {
        let Ok(v) = <[u8; PACKED_VERTEX_STRIDE]>::try_from(record) else { continue };
        let f = |o: usize| f32::from_le_bytes([v[o], v[o + 1], v[o + 2], v[o + 3]]);
        positions.push([f(0), f(4), f(8)]);
        // Stored B,G,R,A and handed back R,G,B,A — the same convention as the 36-byte record's
        // colour, which is what makes the two interchangeable downstream.
        colours.push([v[14], v[13], v[12], v[15]]);
        uvs.push([f(16), f(20)]);
    }
    let normals = vec![[0.0f32; 3]; positions.len()];
    Ok((positions, normals, colours, uvs))
}

/// Per-vertex normals from the triangles, for a layout that carries none.
///
/// Area-weighted: the cross product of a triangle's two edges is proportional to twice its area,
/// so summing them unnormalised already weights each face by how much of the surface it is, and a
/// large wall stops being outvoted by the slivers meeting it at a corner.
///
/// A vertex no triangle references — and a degenerate triangle, whose cross product is zero —
/// leaves `+Y`. A zero normal is worse than a wrong one: it makes the surface unlit rather than
/// lit oddly, and it propagates as `NaN` through any normalisation downstream.
pub(crate) fn normals_from_triangles(positions: &[[f32; 3]], indices: &[u32]) -> Vec<[f32; 3]> {
    let mut out = vec![[0.0f32; 3]; positions.len()];
    for tri in indices.chunks_exact(3) {
        let (a, b, c) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
        let (Some(pa), Some(pb), Some(pc)) =
            (positions.get(a), positions.get(b), positions.get(c))
        else {
            continue;
        };
        let e1 = [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]];
        let e2 = [pc[0] - pa[0], pc[1] - pa[1], pc[2] - pa[2]];
        let n = [
            e1[1] * e2[2] - e1[2] * e2[1],
            e1[2] * e2[0] - e1[0] * e2[2],
            e1[0] * e2[1] - e1[1] * e2[0],
        ];
        for &i in &[a, b, c] {
            if let Some(acc) = out.get_mut(i) {
                for (slot, axis) in acc.iter_mut().zip(n) {
                    *slot += axis;
                }
            }
        }
    }
    for n in &mut out {
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if len > 1e-20 {
            for axis in n.iter_mut() {
                *axis /= len;
            }
        } else {
            *n = [0.0, 1.0, 0.0];
        }
    }
    out
}

/// Axis-aligned bounds of a position list (zeroed when it is empty).
pub(super) fn bounds(positions: &[[f32; 3]]) -> ([f32; 3], [f32; 3]) {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for p in positions {
        for k in 0..3 {
            min[k] = min[k].min(p[k]);
            max[k] = max[k].max(p[k]);
        }
    }
    if positions.is_empty() {
        (([0.0; 3]), ([0.0; 3]))
    } else {
        (min, max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_vertices_from_the_end_slice() {
        // 2 vertices of stride 36, prefixed with 8 pad bytes.
        let mut vbuf = vec![0x11u8; 8];
        for v in 0..2 {
            let base = v as f32;
            for f in [base, base + 0.1, base + 0.2, 0.0, 1.0, 0.0, -1.0e38, 0.5, 0.25] {
                vbuf.extend_from_slice(&f.to_le_bytes());
            }
        }
        let (pos, nrm, col, uv) = parse_vertices_with(&vbuf, 2, Layout::Standard).unwrap();
        assert_eq!(col.len(), 2);
        assert_eq!(pos.len(), 2);
        assert!((pos[1][0] - 1.0).abs() < 1e-6);
        assert_eq!(nrm[0], [0.0, 1.0, 0.0]);
        assert_eq!(uv[0], [0.5, 0.25]);
    }

    #[test]
    fn the_stride_a_buffer_can_hold_is_the_stride_it_gets() {
        // 3000GT_KIT00_ENGINE_B: 318 verts in a 7700-byte buffer. The standard read (318*36 =
        // 11448) overruns it, the packed one (318*24 = 7632) does not — so it is decoded packed
        // rather than skipped, which is what it used to be.
        assert!(!standard_vertex_layout(318, 7700));
        assert_eq!(layout_for(318, 7700), Some(Layout::Packed));
        // Standard 36-byte solids (with a little leading pad) stay standard.
        assert!(standard_vertex_layout(2, 8 + 2 * 36));
        assert_eq!(layout_for(2, 8 + 2 * 36), Some(Layout::Standard));
        assert_eq!(layout_for(730, 26284), Some(Layout::Standard)); // a real 240SX body LOD
        assert_eq!(layout_for(0, 0), Some(Layout::Standard));
        // Too small for either is neither — the caller still has a skip to report.
        assert_eq!(layout_for(318, 7000), None);
    }

    #[test]
    fn the_packed_record_reads_position_colour_and_uv() {
        // 2 vertices of stride 24, prefixed with 8 pad bytes: pos f32[3], BGRA u32, uv f32[2].
        let mut vbuf = vec![0x11u8; 8];
        for v in 0..2 {
            let base = v as f32;
            for f in [base, base + 0.1, base + 0.2] {
                vbuf.extend_from_slice(&f.to_le_bytes());
            }
            vbuf.extend_from_slice(&[0x40, 0x30, 0x20, 0xff]); // B, G, R, A
            for f in [0.5f32, 0.25] {
                vbuf.extend_from_slice(&f.to_le_bytes());
            }
        }
        let (pos, nrm, col, uv) = parse_vertices_with(&vbuf, 2, Layout::Packed).unwrap();
        assert_eq!(pos.len(), 2);
        assert!((pos[1][0] - 1.0).abs() < 1e-6);
        assert_eq!(col[0], [0x20, 0x30, 0x40, 0xff]); // handed back R, G, B, A
        assert_eq!(uv[0], [0.5, 0.25]);
        assert_eq!(nrm[0], [0.0, 0.0, 0.0]); // the record carries none
    }

    #[test]
    fn normals_come_off_the_triangles_and_never_come_out_zero() {
        // One triangle in the y = 0 plane, wound so its normal is +Y.
        let pos = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [9.0, 9.0, 9.0]];
        let n = normals_from_triangles(&pos, &[0, 1, 2]);
        assert_eq!(n.len(), 4);
        for v in &n[..3] {
            assert!((v[1] - 1.0).abs() < 1e-6, "expected +Y, got {v:?}");
        }
        // Vertex 3 is in no triangle, and a degenerate triangle contributes nothing — both leave
        // +Y rather than a zero normal, which would read as unlit and NaN on normalisation.
        assert_eq!(n[3], [0.0, 1.0, 0.0]);
        assert_eq!(normals_from_triangles(&pos, &[0, 0, 0])[0], [0.0, 1.0, 0.0]);
    }

    #[test]
    fn bounds_of_an_empty_list_are_zero() {
        assert_eq!(bounds(&[]), ([0.0; 3], [0.0; 3]));
        assert_eq!(bounds(&[[1.0, -2.0, 3.0], [-1.0, 2.0, 0.0]]), ([-1.0, -2.0, 0.0], [1.0, 2.0, 3.0]));
    }
}
