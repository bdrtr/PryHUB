//! `0x0013401A`: a solid's position markers.
//!
//! Named frames the game hangs things off at run time. The file models some parts about the
//! origin and never places them — a car's exhaust tip and brake discs — and says where they go
//! here instead: the rear bumper's `LEFT_EXHAUST` / `RIGHT_EXHAUST`, the wheel's `MARKER_BRAKE`,
//! `BASE`'s `SPOILER`. The names are not stored, only their `bStringHash`; the ones above were
//! recovered by hashing candidates, and `MARKER_BRAKE` is a string in `SPEED2.EXE`.
//!
//! Layout, measured over every car of an install (6,167 chunks, 28,456 records): zero or two
//! `0x11111111` filler words, then 80-byte records with nothing left over. A record is a `u32`
//! hash, three `u32`s that are zero in all 28,456, and a row-major 4×4 matrix whose last column is
//! `0,0,0,1` in all of them.

use super::format::{FILLER_BYTE, MARKER_MATRIX, MARKER_STRIDE};
use crate::types::{AssetHash, Mat4, NfsMarker};

/// Every record in a `0x0013401A` payload. A trailing partial record is ignored rather than
/// guessed at; none occurs in an install.
#[must_use]
pub fn parse_markers(data: &[u8]) -> Vec<NfsMarker> {
    let mut off = 0;
    while data.len() >= off + 4 && data[off..off + 4] == [FILLER_BYTE; 4] {
        off += 4;
    }
    data[off..]
        .as_chunks::<MARKER_STRIDE>().0.iter()
        .map(|rec| {
            let u = |o: usize| u32::from_le_bytes([rec[o], rec[o + 1], rec[o + 2], rec[o + 3]]);
            let mut m: Mat4 = [[0.0; 4]; 4];
            for (i, row) in m.iter_mut().enumerate() {
                for (j, cell) in row.iter_mut().enumerate() {
                    *cell = f32::from_bits(u(MARKER_MATRIX + (i * 4 + j) * 4));
                }
            }
            NfsMarker::new(AssetHash(u(0)), m)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(hash: u32, pos: [f32; 3]) -> Vec<u8> {
        let mut r = hash.to_le_bytes().to_vec();
        r.extend_from_slice(&[0u8; 12]);
        let m = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [pos[0], pos[1], pos[2], 1.0]];
        for row in m {
            for v in row {
                r.extend_from_slice(&f32::to_le_bytes(v));
            }
        }
        r
    }

    #[test]
    fn records_follow_the_filler() {
        let mut data = vec![FILLER_BYTE; 8];
        data.extend(record(crate::hash::string_hash("LEFT_EXHAUST"), [-1.94, 0.47, 0.10]));
        data.extend(record(0xEA8D_FB05, [0.0, 0.09, 0.0]));
        let m = parse_markers(&data);
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].hash.0, 0xBCF8_A18B, "LEFT_EXHAUST's hash, as the files carry it");
        assert_eq!(m[0].position(), [-1.94, 0.47, 0.10]);
        assert_eq!(m[1].hash.0, crate::hash::string_hash("MARKER_BRAKE"));
    }

    #[test]
    fn no_filler_and_a_partial_tail() {
        let mut data = record(1, [1.0, 2.0, 3.0]);
        data.extend_from_slice(&[0u8; 40]);
        let m = parse_markers(&data);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].position(), [1.0, 2.0, 3.0]);
        assert!(parse_markers(&[]).is_empty());
    }

    #[test]
    fn a_part_finds_its_marker_by_name() {
        let part = crate::types::NfsMeshPart {
            markers: parse_markers(&record(crate::hash::string_hash("SPOILER"), [-1.85, 0.0, 0.72])),
            ..Default::default()
        };
        assert_eq!(part.marker("SPOILER").map(NfsMarker::position), Some([-1.85, 0.0, 0.72]));
        assert!(part.marker("LEFT_EXHAUST").is_none());
    }
}
