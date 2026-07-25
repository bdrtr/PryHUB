//! The `0x00134B01` vertex buffer: stride-36 position/normal/uv records.

use super::format::VERTEX_STRIDE;
use crate::error::{NfsError, NfsResult};

/// Whether a solid's vertex buffer is big enough for the standard 36-byte ([`VERTEX_STRIDE`])
/// layout. A few solids use a smaller packed stride — in practice only hidden engine meshes such
/// as `3000GT_KIT00_ENGINE_B` — and those are skipped rather than mis-decoded (or aborting the
/// parse of the whole car).
pub fn standard_vertex_layout(vert_count: usize, vbuf_len: usize) -> bool {
    vert_count.saturating_mul(VERTEX_STRIDE) <= vbuf_len
}

/// The vertices occupy the last `count * STRIDE` bytes of the buffer (leading bytes are
/// alignment padding). Returns parallel position/normal/uv arrays.
#[allow(clippy::type_complexity)]
pub(super) fn parse_vertices(
    vbuf: &[u8],
    count: usize,
) -> NfsResult<(Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<[f32; 2]>)> {
    let needed = count
        .checked_mul(VERTEX_STRIDE)
        .ok_or(NfsError::BufferSizeMismatch { detail: "vertex count overflow" })?;
    if needed > vbuf.len() {
        return Err(NfsError::BufferSizeMismatch { detail: "vertex buffer smaller than count*stride" });
    }
    let start = vbuf.len() - needed;
    let mut positions = Vec::with_capacity(count);
    let mut normals = Vec::with_capacity(count);
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
        // 24: a constant sentinel (~-1.7e38); unused.
        uvs.push([f(28), f(32)]);
    }
    Ok((positions, normals, uvs))
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
        let (pos, nrm, uv) = parse_vertices(&vbuf, 2).unwrap();
        assert_eq!(pos.len(), 2);
        assert!((pos[1][0] - 1.0).abs() < 1e-6);
        assert_eq!(nrm[0], [0.0, 1.0, 0.0]);
        assert_eq!(uv[0], [0.5, 0.25]);
    }

    #[test]
    fn skips_solids_with_non_standard_vertex_stride() {
        // 3000GT_KIT00_ENGINE_B: 318 verts in a 7700-byte buffer → a ~24-byte packed stride, so the
        // standard 36-byte read (318*36 = 11448) overruns it. Such solids are skipped, not decoded.
        assert!(!standard_vertex_layout(318, 7700));
        // Standard 36-byte solids (with a little leading pad) are supported.
        assert!(standard_vertex_layout(2, 8 + 2 * 36));
        assert!(standard_vertex_layout(730, 26284)); // a real 240SX body LOD
        assert!(standard_vertex_layout(0, 0));
    }

    #[test]
    fn bounds_of_an_empty_list_are_zero() {
        assert_eq!(bounds(&[]), ([0.0; 3], [0.0; 3]));
        assert_eq!(bounds(&[[1.0, -2.0, 3.0], [-1.0, 2.0, 0.0]]), ([-1.0, -2.0, 0.0], [1.0, 2.0, 3.0]));
    }
}
