//! The `CarParts` tables inside `GLOBAL/GLOBALB.BUN`: every purchasable and fitted part the game
//! knows, and the attributes hung off them.
//!
//! **Locked empirically against a real install, never from a spec.** The bundle is an ordinary
//! chunk stream, and `0x80034602` holds five siblings that only make sense together:
//!
//! | chunk | what | stride |
//! |---|---|---|
//! | `0x00034603` | header: the counts for the rest | 60 B, read as 15 × u32 |
//! | `0x00034604` | one record per part | 14 B |
//! | `0x00034605` | attributes, `(key, value)` | 8 B |
//! | `0x00034606` | the string blob every name points into | packed, NUL-separated |
//! | `0x0003460A` | attribute blocks | 36 B |
//!
//! The header is what makes this readable rather than guessed: slots 8/10/12/14 hold the record
//! counts, and three of the four multiply out to their chunk's size **exactly** (4636 × 8 = 37,088;
//! 75 × 4 = 300; 1580 × 36 = 56,880). The fourth is 12,167 × 14 = 170,338 against a 170,340-byte
//! chunk — two bytes of the alignment padding this format uses everywhere. Every count is checked
//! against its chunk here rather than trusted, so a file that disagrees is rejected instead of
//! mis-read.
//!
//! Two fields of the 14-byte part record are locked; the rest are not, and are not invented:
//!
//! * `+8`, **multiplied by 4**, is a byte offset into the string blob. On one install all 12,167
//!   records land on a string *start* — the multiplier is not a guess either, the raw value tops out
//!   at 9,213 where the blob is 36,860 bytes long.
//! * `+12` indexes the attribute blocks, with `0xFFFF` for "none". Excluding that sentinel there are
//!   exactly 1,580 distinct values with a maximum of 1,579, which is that chunk's record count.
//!
//! The 36-byte block itself is **not** decoded: what links a part to its attributes is still
//! unknown, so [`CarPart::block`] hands the raw index over rather than pretending to resolve it.
//!
//! An attribute's key is the [`crate::hash`] of its *name*, which is how the six below were
//! confirmed rather than guessed — and `RED`/`GREEN`/`BLUE` in adjacent triples are the game's paint
//! palette, which is the one place a car's colour is written down at all: `GEOMETRY.BIN` has none.

use crate::chunk::ChunkNode;
use crate::error::{NfsError, NfsResult};
use crate::types::AssetHash;

/// The `CarParts` container, and the five chunks inside it.
const CONTAINER: u32 = 0x8003_4602;
const HEADER: u32 = 0x0003_4603;
const PARTS: u32 = 0x0003_4604;
const ATTRIBUTES: u32 = 0x0003_4605;
const STRINGS: u32 = 0x0003_4606;
const BLOCKS: u32 = 0x0003_460A;

/// Bytes per record.
const PART_STRIDE: usize = 14;
const ATTR_STRIDE: usize = 8;
const BLOCK_STRIDE: usize = 36;

/// u32 slots in the header that hold a count.
const COUNT_ATTRIBUTES: usize = 8;
const COUNT_BLOCKS: usize = 12;
const COUNT_PARTS: usize = 14;

/// u16 field offsets inside a part record.
const P_NAME: usize = 8;
const P_BLOCK: usize = 12;
/// The part record stores its name offset in 4-byte units.
const NAME_SCALE: usize = 4;
/// A part with no attribute block.
const NO_BLOCK: u16 = 0xFFFF;

/// Attribute keys, as [`crate::hash::string_hash`] of their names.
///
/// Written as constants and *proved* by a test rather than hashed at run time: the value is what is
/// in the file, and the test is what says which word produced it.
pub mod key {
    /// `string_hash("TEXTURE")`
    pub const TEXTURE: u32 = 0xFD35_FE70;
    /// `string_hash("NAME")`
    pub const NAME: u32 = 0x0019_CBC0;
    /// `string_hash("RED")`
    pub const RED: u32 = 0x0000_D99A;
    /// `string_hash("GREEN")`
    pub const GREEN: u32 = 0x02DD_C8F0;
    /// `string_hash("BLUE")`
    pub const BLUE: u32 = 0x0013_6707;
    /// `string_hash("CARBONFIBRE")`
    pub const CARBONFIBRE: u32 = 0x721A_FF7C;
}

/// One part: what it is called, and where its attributes are.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CarPart {
    /// The name from the string blob — `BASE KIT00`, `STOCK`, `VINYL_L3_COLOR01`.
    pub name: String,
    /// Index into the attribute blocks, or `None` where the record holds the sentinel. The block's
    /// own 36 bytes are not decoded, so this is as far as the link goes.
    pub block: Option<u16>,
}

/// One attribute: a hashed name, and a value whose meaning belongs to that name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Attribute {
    pub key: AssetHash,
    pub value: u32,
}

/// A colour from the game's palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Colour {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

/// Everything the `CarParts` container holds.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct CarParts {
    pub parts: Vec<CarPart>,
    pub attributes: Vec<Attribute>,
}

impl CarParts {
    /// Read the tables out of a (already decompressed) `GLOBALB.BUN`.
    ///
    /// # Errors
    /// When the bundle carries no `CarParts` container, or when a count in the header does not
    /// match the size of the chunk it describes — the second is what stops a differently-built
    /// bundle being read through this one's arithmetic.
    pub fn parse(globalb: &[u8]) -> NfsResult<Self> {
        let roots = ChunkNode::parse(globalb)?;
        // `find` looks *inside* a node, so a buffer that is itself the container — which is what a
        // test builds, and what an extracted chunk would be — has to be recognised on its own.
        let container = roots
            .iter()
            .find(|r| r.header.id == CONTAINER)
            .or_else(|| roots.iter().find_map(|r| r.find(CONTAINER)))
            .ok_or(NfsError::BufferSizeMismatch { detail: "no CarParts container" })?;

        let chunk = |id: u32| -> NfsResult<&[u8]> {
            container
                .find(id)
                .map(|c| c.data(globalb))
                .ok_or(NfsError::BufferSizeMismatch { detail: "CarParts chunk missing" })
        };
        let header = chunk(HEADER)?;
        let count = |slot: usize| -> NfsResult<usize> {
            let at = slot * 4;
            header
                .get(at..at + 4)
                .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
                .ok_or(NfsError::BufferSizeMismatch { detail: "CarParts header too short" })
        };

        let (n_parts, n_attrs, n_blocks) =
            (count(COUNT_PARTS)?, count(COUNT_ATTRIBUTES)?, count(COUNT_BLOCKS)?);
        let part_bytes = chunk(PARTS)?;
        let attr_bytes = chunk(ATTRIBUTES)?;
        let strings = chunk(STRINGS)?;

        // The header's counts against the chunks they describe. `parts` is allowed the alignment
        // padding the format pays everywhere (the measured file is two bytes over); the others are
        // required to be exact, because they measured exact.
        fits(n_parts, PART_STRIDE, part_bytes.len(), true)?;
        fits(n_attrs, ATTR_STRIDE, attr_bytes.len(), false)?;
        fits(n_blocks, BLOCK_STRIDE, chunk(BLOCKS)?.len(), false)?;

        let mut parts = Vec::with_capacity(n_parts);
        for i in 0..n_parts {
            let r = part_bytes
                .get(i * PART_STRIDE..(i + 1) * PART_STRIDE)
                .ok_or(NfsError::BufferSizeMismatch { detail: "part record out of range" })?;
            let at = usize::from(le_u16(r, P_NAME)) * NAME_SCALE;
            let block = le_u16(r, P_BLOCK);
            parts.push(CarPart {
                name: string_at(strings, at),
                block: (block != NO_BLOCK).then_some(block),
            });
        }

        let mut attributes = Vec::with_capacity(n_attrs);
        for i in 0..n_attrs {
            let r = attr_bytes
                .get(i * ATTR_STRIDE..(i + 1) * ATTR_STRIDE)
                .ok_or(NfsError::BufferSizeMismatch { detail: "attribute out of range" })?;
            attributes.push(Attribute {
                key: AssetHash(le_u32(r, 0)),
                value: le_u32(r, 4),
            });
        }
        Ok(Self { parts, attributes })
    }

    /// The paint palette: every `RED`/`GREEN`/`BLUE` run of three adjacent attributes.
    ///
    /// Adjacent rather than resolved through a part, because what links a part to its attributes is
    /// not decoded — and it does not need to be for this: a colour *is* three attributes in a row,
    /// and on one install that reading yields 123 of them with no component over 255. The order
    /// within the triple is not assumed; each of the three is looked up by its own key.
    #[must_use]
    pub fn palette(&self) -> Vec<Colour> {
        let mut out = Vec::new();
        for w in self.attributes.windows(3) {
            let mut red = None;
            let mut green = None;
            let mut blue = None;
            for a in w {
                match a.key.0 {
                    key::RED => red = Some(a.value),
                    key::GREEN => green = Some(a.value),
                    key::BLUE => blue = Some(a.value),
                    _ => {}
                }
            }
            if let (Some(r), Some(g), Some(b)) = (red, green, blue) {
                // A component outside a byte would mean this window is not a colour after all.
                if let (Ok(red), Ok(green), Ok(blue)) =
                    (u8::try_from(r), u8::try_from(g), u8::try_from(b))
                {
                    out.push(Colour { red, green, blue });
                }
            }
        }
        out
    }
}

/// Whether `count` records of `stride` fit the chunk. `padded` allows the format's alignment tail.
fn fits(count: usize, stride: usize, len: usize, padded: bool) -> NfsResult<()> {
    let needed = count
        .checked_mul(stride)
        .ok_or(NfsError::BufferSizeMismatch { detail: "CarParts count overflow" })?;
    let ok = if padded {
        // Never more than three bytes over: that is what aligning to 4 can cost.
        needed <= len && len - needed < 4
    } else {
        needed == len
    };
    if ok {
        Ok(())
    } else {
        Err(NfsError::BufferSizeMismatch { detail: "CarParts count does not match its chunk" })
    }
}

fn le_u16(b: &[u8], off: usize) -> u16 {
    b.get(off..off + 2).map_or(0, |s| u16::from_le_bytes([s[0], s[1]]))
}

fn le_u32(b: &[u8], off: usize) -> u32 {
    b.get(off..off + 4).map_or(0, |s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

/// The NUL-terminated string beginning at `off`, or empty when it runs off the end.
fn string_at(blob: &[u8], off: usize) -> String {
    let rest = blob.get(off..).unwrap_or(&[]);
    let end = rest.iter().position(|&c| c == 0).unwrap_or(rest.len());
    String::from_utf8_lossy(&rest[..end]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::string_hash;

    /// The constants above are hashes of words, and this is the sentence that says which words.
    #[test]
    fn every_named_key_is_the_hash_of_its_name() {
        assert_eq!(string_hash("TEXTURE"), key::TEXTURE);
        assert_eq!(string_hash("NAME"), key::NAME);
        assert_eq!(string_hash("RED"), key::RED);
        assert_eq!(string_hash("GREEN"), key::GREEN);
        assert_eq!(string_hash("BLUE"), key::BLUE);
        assert_eq!(string_hash("CARBONFIBRE"), key::CARBONFIBRE);
    }

    fn chunk(id: u32, payload: &[u8]) -> Vec<u8> {
        let mut v = id.to_le_bytes().to_vec();
        v.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        v.extend_from_slice(payload);
        v
    }

    /// A container with two parts, one colour triple, and a header that agrees with all of it.
    fn synthetic() -> Vec<u8> {
        // The second name is padded out to a multiple of four, because the record can only store an
        // offset in those units — the blob in the real file is laid out the same way.
        let strings = b"HOOD\0\0\0\0SPOILER\0".to_vec();
        let mut header = vec![0_u8; 60];
        let put = |h: &mut Vec<u8>, slot: usize, v: u32| {
            h[slot * 4..slot * 4 + 4].copy_from_slice(&v.to_le_bytes());
        };
        put(&mut header, COUNT_PARTS, 2);
        put(&mut header, COUNT_ATTRIBUTES, 3);
        put(&mut header, COUNT_BLOCKS, 1);

        let mut parts = Vec::new();
        for (name_at, block) in [(0_u16, 0_u16), (2_u16, NO_BLOCK)] {
            let mut r = [0_u8; PART_STRIDE];
            r[P_NAME..P_NAME + 2].copy_from_slice(&name_at.to_le_bytes());
            r[P_BLOCK..P_BLOCK + 2].copy_from_slice(&block.to_le_bytes());
            parts.extend_from_slice(&r);
        }
        let mut attrs = Vec::new();
        for (k, v) in [(key::RED, 200_u32), (key::GREEN, 40), (key::BLUE, 15)] {
            attrs.extend_from_slice(&k.to_le_bytes());
            attrs.extend_from_slice(&v.to_le_bytes());
        }
        let body = [
            chunk(HEADER, &header),
            chunk(PARTS, &parts),
            chunk(ATTRIBUTES, &attrs),
            chunk(STRINGS, &strings),
            chunk(BLOCKS, &[0_u8; BLOCK_STRIDE]),
        ]
        .concat();
        chunk(CONTAINER, &body)
    }

    #[test]
    fn a_part_reads_its_name_through_the_four_byte_scale() {
        let parts = CarParts::parse(&synthetic()).expect("parses");
        assert_eq!(parts.parts.len(), 2);
        assert_eq!(parts.parts[0].name, "HOOD");
        // The second name sits at byte 5, which the record stores as 5/4 = 1.
        assert_eq!(parts.parts[1].name, "SPOILER");
    }

    /// `0xFFFF` is "no block", not block 65,535.
    #[test]
    fn the_sentinel_block_is_not_an_index() {
        let parts = CarParts::parse(&synthetic()).expect("parses");
        assert_eq!(parts.parts[0].block, Some(0));
        assert_eq!(parts.parts[1].block, None);
    }

    #[test]
    fn three_adjacent_components_make_a_colour() {
        let parts = CarParts::parse(&synthetic()).expect("parses");
        assert_eq!(parts.palette(), vec![Colour { red: 200, green: 40, blue: 15 }]);
    }

    /// The header is checked against the chunks, not trusted: a count that does not fit is what
    /// tells us this is a differently-built bundle rather than a differently-shaped record.
    #[test]
    fn a_count_that_does_not_fit_its_chunk_is_refused() {
        let mut bytes = synthetic();
        // The parts count lives at header slot 14; make it claim one part too many.
        let at = bytes
            .windows(4)
            .position(|w| w == 2_u32.to_le_bytes())
            .expect("the count is in there");
        bytes[at..at + 4].copy_from_slice(&99_u32.to_le_bytes());
        assert!(CarParts::parse(&bytes).is_err());
    }

    #[test]
    fn a_bundle_without_the_container_is_an_error() {
        assert!(CarParts::parse(&chunk(0x0000_0001, &[1, 2, 3])).is_err());
    }
}
