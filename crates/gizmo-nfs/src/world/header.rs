//! The `0x00134011` solid header, as a `STREAM*.BUN` writes it.
//!
//! Structurally this is the same 192-byte record `GEOMETRY.BIN` uses — asset hash, bounding box,
//! 4x4 matrix, name — with one difference that moves every field: **the city prefixes it with
//! `0x11` alignment filler.** Measured over `STREAML4RH.BUN`'s 175 headers the payload is 192 bytes
//! plus 0, 4, 8 or 12 bytes of it (46 / 36 / 49 / 44 respectively), so 129 of 175 — 74 % — are
//! shifted. Every solid header in the three cars checked is exactly 192 bytes with no filler at
//! all, which is how [`crate::geometry::read_matrix`] can apply [`MATRIX_OFFSET`] absolutely and be
//! right about cars while being wrong about the city.
//!
//! What makes that worth its own module rather than a shared helper is the failure mode. Reading a
//! shifted header at the nominal offsets does not fail: it returns a matrix built from the wrong
//! floats, so a prop lands *somewhere* rather than nowhere and nothing downstream complains. The
//! discriminator is the matrix's own last element, which the format fixes at `1.0`. With the filler
//! skipped, `m[15] == 1.0` in **175 of 175** L4RH headers; without, in **46** — exactly the 46 that
//! had no filler to skip in the first place, and their translations come out as
//! `(1.0, 0.0, 1026.5)` where the correct read gives `(1026.5, -1703.1, 0.0)`. Hence the four
//! filler-width tests below: they are the only thing standing between this reader and a city whose
//! buildings are all in believable, wrong places.
//!
//! The name is the one field the shift cannot reach. It sits a fixed [`NAME_FROM_END`] bytes from
//! the payload's *end*, and the filler is at the front, so it reads identically either way. That
//! makes it the anchor the rest is checked against: `bStringHash(name)` equals the hash stored at
//! [`HASH_OFFSET`] in 175 of 175 L4RH headers and 482 of 482 in `STREAML4RR`, which is what locks
//! both offsets at once. Where it does *not* match, the name was truncated by the fixed-width field
//! rather than the read being wrong — 133 of `STREAML4RC`'s 772, 537 of `STREAML4RG`'s 2271 — so
//! [`WorldSolidHeader::name_is_whole`] reports it the way [`crate::types::NfsTexture::name_is_whole`]
//! does for textures, instead of this module pretending to a name it only half has.

use crate::error::{NfsError, NfsResult};
use crate::geometry::format::MATRIX_OFFSET;
use crate::geometry::skip_leading_filler;
use crate::reader::ByteReader;
use crate::types::{AssetHash, Mat4, IDENTITY};

/// The header body's size once its leading `0x11` filler is gone. Payloads are this plus 0, 4, 8
/// or 12 bytes; the filler is what varies, not the record.
pub const BODY_LEN: usize = 192;

/// Offset of the part-name hash within the filler-stripped body.
pub const HASH_OFFSET: usize = 0x10;

/// Offset of the axis-aligned bounding box's minimum corner (3 × f32).
pub const BBOX_MIN_OFFSET: usize = 0x20;

/// Offset of the bounding box's maximum corner (3 × f32).
pub const BBOX_MAX_OFFSET: usize = 0x30;

/// How far the name field sits from the **end** of the payload. Measured from the end on purpose:
/// the leading filler shifts every front-anchored offset and leaves this one alone.
pub const NAME_FROM_END: usize = 28;

/// One `0x00134011` record: what the file says a city object is, before any of its geometry is
/// read. A whole bundle's worth of these is a manifest — every object's identity, extent and
/// placement — at no cost in vertex memory.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct WorldSolidHeader {
    /// The object's name hash, and the key the rest of the bundle refers to it by.
    pub hash: AssetHash,
    /// The name as stored, which the fixed-width field may have cut short — ask
    /// [`WorldSolidHeader::name_is_whole`] before trusting it as an identifier.
    pub name: String,
    /// Local bounding box, minimum corner, in the file's own frame (Z-up).
    pub bbox_min: [f32; 3],
    /// Local bounding box, maximum corner.
    pub bbox_max: [f32; 3],
    /// The object's 4x4 transform, **as in the file**: row-major, original handedness, no fixup.
    /// Identity means the vertices are already in world space (road and terrain are authored that
    /// way); anything else is a placement to apply (props and buildings).
    pub matrix: Mat4,
}

impl WorldSolidHeader {
    /// Whether [`Self::name`] is the whole name rather than a truncated prefix.
    ///
    /// The field is fixed-width, so a long name loses its tail. The stored hash is of the *whole*
    /// name, so hashing what survived and comparing is a proof rather than a guess — the same trick
    /// `crate::hash` uses to recover truncated part names.
    #[must_use]
    pub fn name_is_whole(&self) -> bool {
        !self.name.is_empty() && crate::hash::string_hash(&self.name) == self.hash.0
    }

    /// Whether this object carries a real placement, as opposed to vertices already in world space.
    ///
    /// Exact equality is deliberate: the file stores literal `1.0`/`0.0` for the baked case, so
    /// "not exactly identity" is the file's own statement that a transform is meant, and an
    /// almost-identity matrix is a placement that happens to be small rather than an absence of one.
    #[must_use]
    pub fn is_placed(&self) -> bool {
        self.matrix != IDENTITY
    }

    /// Determinant of the matrix's 3x3 basis.
    ///
    /// `+1` for a rotation, `−1` for one with a reflection in it. Worth having separately from the
    /// matrix because a negative determinant means something different here than it does for a car:
    /// [`crate::placement::should_place`] refuses every `det < 0` outright, which is right for car
    /// solids (their meshes are pre-mirrored) and would drop a legitimately mirrored building on
    /// the world origin. World data is not ambiguous — identity means baked, anything else means
    /// apply — so this reports the sign and leaves the decision to the caller.
    #[must_use]
    pub fn basis_determinant(&self) -> f32 {
        let m = &self.matrix;
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    }
}

/// Read one `0x00134011` payload.
///
/// `payload` is the chunk body — the bytes after the 8-byte chunk header, filler included. Reading
/// it is: skip the filler, then take every field at its offset in the body that remains.
///
/// # Errors
///
/// [`NfsError::CorruptArchive`] if the payload is shorter than one header body, which means it is
/// not a `0x00134011` record whatever its chunk id says.
pub fn read_header(payload: &[u8]) -> NfsResult<WorldSolidHeader> {
    // The filler is a whole number of `0x11` words at the front; everything below is relative to
    // what it leaves. Doing this first is the entire difference between this reader and the car
    // one — see the module docs for what reading without it produces.
    let body = skip_leading_filler(payload);
    if body.len() < BODY_LEN {
        return Err(NfsError::CorruptArchive {
            detail: "0x00134011 payload is shorter than one solid header",
        });
    }

    let hash = AssetHash(ByteReader::at(body, HASH_OFFSET)?.u32_le()?);
    let bbox_min = read_vec3(body, BBOX_MIN_OFFSET)?;
    let bbox_max = read_vec3(body, BBOX_MAX_OFFSET)?;

    let mut matrix = IDENTITY;
    let mut r = ByteReader::at(body, MATRIX_OFFSET)?;
    for row in &mut matrix {
        for cell in row.iter_mut() {
            *cell = r.f32_le()?;
        }
    }

    // Anchored to the end of the *payload*, not the body: the filler is at the front, so the two
    // agree, and measuring from the end is what makes this field immune to the shift.
    let name_at = payload.len().checked_sub(NAME_FROM_END).ok_or(NfsError::CorruptArchive {
        detail: "0x00134011 payload has no room for a name field",
    })?;
    let field = &payload[name_at..];
    let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    let name = String::from_utf8_lossy(&field[..end]).into_owned();

    Ok(WorldSolidHeader { hash, name, bbox_min, bbox_max, matrix })
}

fn read_vec3(body: &[u8], at: usize) -> NfsResult<[f32; 3]> {
    let mut r = ByteReader::at(body, at)?;
    Ok([r.f32_le()?, r.f32_le()?, r.f32_le()?])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::format::FILLER_BYTE;

    /// Build one header payload: `filler` bytes of `0x11`, then a 192-byte body carrying `name`'s
    /// hash, a bounding box, `matrix`, and the name itself 28 bytes from the end.
    fn synth(name: &str, filler: usize, matrix: Mat4) -> Vec<u8> {
        let mut body = vec![0u8; BODY_LEN];
        body[HASH_OFFSET..HASH_OFFSET + 4]
            .copy_from_slice(&crate::hash::string_hash(name).to_le_bytes());
        for (i, v) in [-1.0f32, -2.0, -3.0].iter().enumerate() {
            body[BBOX_MIN_OFFSET + i * 4..BBOX_MIN_OFFSET + i * 4 + 4]
                .copy_from_slice(&v.to_le_bytes());
        }
        for (i, v) in [4.0f32, 5.0, 6.0].iter().enumerate() {
            body[BBOX_MAX_OFFSET + i * 4..BBOX_MAX_OFFSET + i * 4 + 4]
                .copy_from_slice(&v.to_le_bytes());
        }
        let mut at = MATRIX_OFFSET;
        for row in &matrix {
            for cell in row {
                body[at..at + 4].copy_from_slice(&cell.to_le_bytes());
                at += 4;
            }
        }
        // The name field, written from the end so the test builds it the way the reader finds it.
        // A name longer than the field loses its tail here exactly as it does in the file — the
        // hash above is of the whole name either way, which is what makes the loss detectable.
        let name_at = BODY_LEN - NAME_FROM_END;
        let fits = name.len().min(NAME_FROM_END);
        body[name_at..name_at + fits].copy_from_slice(&name.as_bytes()[..fits]);

        let mut payload = vec![FILLER_BYTE; filler];
        payload.extend_from_slice(&body);
        payload
    }

    fn placed() -> Mat4 {
        let mut m = IDENTITY;
        m[3] = [1026.5, -1703.1, 0.0, 1.0];
        m
    }

    /// The one that matters: every filler width the city uses must read identically.
    ///
    /// 0, 4, 8 and 12 are not arbitrary — they are the four widths measured across
    /// `STREAML4RH.BUN`'s 175 headers (46 / 36 / 49 / 44). A reader that handles only the first
    /// silently mis-places 74 % of that region.
    #[test]
    fn every_filler_width_reads_the_same_header() {
        let reference = read_header(&synth("TRN_ROADA_CHOP_A", 0, placed())).unwrap();
        for filler in [4usize, 8, 12] {
            let got = read_header(&synth("TRN_ROADA_CHOP_A", filler, placed())).unwrap();
            assert_eq!(got, reference, "{filler} bytes of filler changed the header");
        }
        // And the reference is right, not merely stable.
        assert_eq!(reference.name, "TRN_ROADA_CHOP_A");
        assert!(reference.name_is_whole(), "the hash is of the name we wrote");
        assert_eq!(reference.bbox_min, [-1.0, -2.0, -3.0]);
        assert_eq!(reference.bbox_max, [4.0, 5.0, 6.0]);
        assert_eq!(reference.matrix[3], [1026.5, -1703.1, 0.0, 1.0]);
    }

    /// `m[15] == 1.0` is what tells a correct read from a shifted one, so it must survive the shift.
    #[test]
    fn the_matrixs_last_element_is_one_at_every_filler_width() {
        for filler in [0usize, 4, 8, 12] {
            let h = read_header(&synth("OBJ_PYLON", filler, placed())).unwrap();
            assert_eq!(h.matrix[3][3], 1.0, "{filler} bytes of filler shifted the matrix");
        }
    }

    /// Identity means the vertices are already in world space; the city says so for road and
    /// terrain (169 of L4RH's 175) and says otherwise for props (the remaining 6).
    #[test]
    fn identity_is_not_a_placement_and_anything_else_is() {
        let baked = read_header(&synth("TRN_TERRAINA_A", 8, IDENTITY)).unwrap();
        assert!(!baked.is_placed());
        let prop = read_header(&synth("XO_LAMPPOST_A", 8, placed())).unwrap();
        assert!(prop.is_placed());
    }

    /// A name cut short by the fixed-width field is reported as such rather than passed off as an
    /// identifier — 133 of `STREAML4RC`'s 772 headers are in this state.
    #[test]
    fn a_truncated_name_fails_its_own_hash() {
        // 29 characters into a 28-byte field: the stored hash is of the whole name, the field
        // holds all but the last letter.
        let whole = "XB_BUILDING_DOWNTOWN_CORNER_A";
        assert!(whole.len() > NAME_FROM_END, "this name must not fit, or the test proves nothing");
        let h = read_header(&synth(whole, 4, IDENTITY)).unwrap();
        assert_eq!(h.name, &whole[..NAME_FROM_END], "the prefix that fit is still returned");
        assert!(!h.name_is_whole(), "but it does not claim to be the whole name");

        // The same name one character shorter fits, and then it does claim it.
        let fits = &whole[..NAME_FROM_END];
        assert!(read_header(&synth(fits, 4, IDENTITY)).unwrap().name_is_whole());
    }

    /// Short payloads are refused rather than read out of whatever follows them.
    #[test]
    fn a_payload_too_short_for_a_header_is_refused() {
        assert!(read_header(&[]).is_err());
        assert!(read_header(&[FILLER_BYTE; 8]).is_err());
        assert!(read_header(&[0u8; BODY_LEN - 1]).is_err());
        // Exactly one body, no filler, is the smallest thing that is a header.
        assert!(read_header(&[0u8; BODY_LEN]).is_ok());
    }
}
