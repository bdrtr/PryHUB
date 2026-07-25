//! JDLZ decompression (EA Black Box's LZ variant).
//!
//! JDLZ streams start with the ASCII magic `"JDLZ"`, a version/flags pair, then the
//! uncompressed and compressed sizes (both little-endian `u32`), for a 16-byte header.
//! The body interleaves two 1-bit flag streams (`flags1` selects literal vs. copy;
//! `flags2` selects the copy token's short/long form). Copies are byte-by-byte so
//! overlapping back-references work.
//!
//! The bit-layout below was **validated byte-for-byte** against a real golden pair from
//! an NFSU2 install (`GLOBAL/InGameCommon.lzc` decompresses exactly to
//! `GLOBAL/InGameCommon.bun`); see the env-gated `tests/golden_assets.rs`.
//!
//! [`compress`] is the same layout run backwards. It does **not** aim to reproduce EA's own byte
//! stream — two LZ encoders that disagree about which match to take both produce valid files — so
//! the test that matters is that [`decompress`] returns the input for anything thrown at it,
//! including a real bundle, plus a proptest over arbitrary bytes.
//!
//! Ratio matters here for one concrete reason: a texture goes back into a TPK **in place**, so it
//! only fits if the encoder is at least as good as the one that made the slot. On the golden bundle
//! this packs to 29.8% where EA's own encoder gets 30.1%, and across a whole install 66% of texture
//! blobs recompress small enough to be written back where they came from.

use crate::error::{NfsError, NfsResult};
use crate::reader::ByteReader;

/// Refuse to allocate an output larger than this from the (attacker-controlled) size field.
const MAX_OUTPUT: usize = 256 * 1024 * 1024;

/// The 16-byte JDLZ header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JdlzHeader {
    /// Declared size of the decompressed output.
    pub uncompressed_size: u32,
    /// Declared size of the compressed stream (including this header).
    pub compressed_size: u32,
}

/// Parse the JDLZ header. Layout: `"JDLZ"`, two bytes (version/flags), a 2-byte gap,
/// then `uncompressed_size` (LE) and `compressed_size` (LE).
pub fn parse_header(buf: &[u8]) -> NfsResult<JdlzHeader> {
    let mut r = ByteReader::new(buf);
    let magic = r.take(4)?;
    if magic != b"JDLZ" {
        return Err(NfsError::BadMagic { context: "jdlz", found: first_four(buf) });
    }
    let _version = r.u8()?;
    let _flags = r.u8()?;
    let _reserved = r.u16_le()?;
    let uncompressed_size = r.u32_le()?;
    let compressed_size = r.u32_le()?;
    Ok(JdlzHeader { uncompressed_size, compressed_size })
}

/// Decompress a JDLZ stream into its original bytes.
pub fn decompress(buf: &[u8]) -> NfsResult<Vec<u8>> {
    let header = parse_header(buf)?;
    let out_len = header.uncompressed_size as usize;
    if out_len > MAX_OUTPUT {
        return Err(NfsError::Allocation { requested: out_len });
    }

    let mut out: Vec<u8> = Vec::new();
    out.try_reserve(out_len).map_err(|_| NfsError::Allocation { requested: out_len })?;

    let read = |p: usize| -> NfsResult<usize> {
        buf.get(p)
            .map(|&b| b as usize)
            .ok_or(NfsError::UnexpectedEof { offset: p, needed: 1, remaining: 0 })
    };

    let mut in_pos = 16usize; // past the header
    let mut flags1: u32 = 1;
    let mut flags2: u32 = 1;

    // The `| 0x100` sentinel makes a flag word reach the value 1 exactly when its 8 real
    // bits are spent, which is the signal to reload the next flag byte.
    while in_pos < buf.len() && out.len() < out_len {
        if flags1 == 1 {
            flags1 = read(in_pos)? as u32 | 0x100;
            in_pos += 1;
        }
        if flags2 == 1 {
            flags2 = read(in_pos)? as u32 | 0x100;
            in_pos += 1;
        }

        if flags1 & 1 == 1 {
            // A copy token: two bytes, interpreted per the current `flags2` bit.
            let b0 = read(in_pos)?;
            let b1 = read(in_pos + 1)?;
            in_pos += 2;
            let (length, dist) = if flags2 & 1 == 1 {
                // "near" form: length 3..=4098, distance 1..=16
                (((b0 & 0xF0) << 4 | b1) + 3, (b0 & 0x0F) + 1)
            } else {
                // "far" form: length 3..=34, distance 17..~2064
                ((b0 & 0x1F) + 3, (b1 | (b0 & 0xE0) << 3) + 17)
            };
            if dist > out.len() {
                return Err(NfsError::Decompression {
                    codec: "jdlz",
                    detail: "back-reference points before the start of the output",
                });
            }
            let start = out.len() - dist;
            let room = out_len - out.len();
            let take = length.min(room);
            if dist >= take {
                // The source does not overlap what is being written, so this is a memcpy rather than
                // a byte loop — and most copies in a real stream are this kind. `extend_from_within`
                // is the safe way to say it.
                out.extend_from_within(start..start + take);
            } else {
                // An overlapping run (`dist` of 1 means "repeat this byte"), which has to be copied
                // one byte at a time because each byte reads one just written.
                for i in 0..take {
                    let byte = *out.get(start + i).ok_or(NfsError::Decompression {
                        codec: "jdlz",
                        detail: "internal copy index out of range",
                    })?;
                    out.push(byte);
                }
            }
            flags2 >>= 1;
        } else {
            // A literal byte.
            let byte = read(in_pos)? as u8;
            in_pos += 1;
            out.push(byte);
        }
        flags1 >>= 1;
    }

    Ok(out)
}

/// The version and flags bytes a real NFSU2 `.lzc` carries (`02 10`), reproduced so a file this
/// crate writes looks to the game exactly like one it shipped.
const VERSION_FLAGS: [u8; 2] = [0x02, 0x10];

/// Shortest match worth encoding: a copy token is two bytes, so three literals is the break-even.
const MIN_MATCH: usize = 3;
/// The "near" token form carries a 4-bit distance and a 12-bit length.
const NEAR_MAX_DIST: usize = 16;
const NEAR_MAX_LEN: usize = 4098;
/// The "far" form carries an 11-bit distance (biased by 17) and a 5-bit length.
const FAR_MAX_DIST: usize = 2064;
const FAR_MAX_LEN: usize = 34;
/// How many earlier positions with the same 3-byte prefix to try before settling. The window is
/// only 2 KB, so this is close to exhaustive.
const MAX_CHAIN: usize = 256;
const HASH_BITS: u32 = 13;

/// The two interleaved flag streams, as the encoder's side of the decoder's state machine.
///
/// The decoder reloads a flag byte when its word reaches 1 — i.e. after eight bits — and it checks
/// `flags1` before `flags2` at the top of *every* token, literal or copy. So the encoder reserves
/// its bytes in the same order at the same moments, or the two disagree about which byte is which.
struct Flags {
    /// Where the byte being filled sits in the output.
    slot: usize,
    /// Bits of it already spoken for.
    used: u32,
}

impl Flags {
    /// Reserve a fresh flag byte if the current one is spent, and return the bit index to write.
    fn next_bit(&mut self, out: &mut Vec<u8>, first: bool) -> u32 {
        if first || self.used == 8 {
            self.slot = out.len();
            self.used = 0;
            out.push(0);
        }
        let bit = self.used;
        self.used += 1;
        bit
    }

    fn set(&self, out: &mut [u8], bit: u32) {
        out[self.slot] |= 1 << bit;
    }
}

/// Compress `input` into a JDLZ stream that [`decompress`] — and the game — will read back.
///
/// # Errors
/// [`NfsError::Allocation`] when the input is larger than the 32-bit size fields can describe.
pub fn compress(input: &[u8]) -> NfsResult<Vec<u8>> {
    if input.len() > u32::MAX as usize {
        return Err(NfsError::Allocation { requested: input.len() });
    }
    let mut out = Vec::with_capacity(input.len() / 2 + 32);
    out.extend_from_slice(b"JDLZ");
    out.extend_from_slice(&VERSION_FLAGS);
    out.extend_from_slice(&[0, 0]); // reserved
    out.extend_from_slice(&(input.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // compressed size, once it is known

    // Hash chains over 3-byte prefixes: `head` is the most recent position per hash, `prev` links
    // each position to the one before it with the same hash.
    const NONE: u32 = u32::MAX;
    let mut head = vec![NONE; 1 << HASH_BITS];
    let mut prev = vec![NONE; input.len()];

    let mut flags1 = Flags { slot: 0, used: 8 };
    let mut flags2 = Flags { slot: 0, used: 8 };
    let mut first = true;
    let mut pos = 0usize;

    // A match the lazy probe already found for the position we are about to reach, so the search
    // is not repeated for it.
    let mut pending: Option<(usize, usize)> = None;

    while pos < input.len() {
        let (mut len, dist) = match pending.take() {
            Some(found) => found,
            None => longest_match(input, pos, &head, &prev),
        };
        // Insert before the lazy probe, so a match at pos + 1 may legitimately point at pos. Each
        // position is inserted exactly once — twice would make its chain point at itself.
        insert(input, pos, &mut head, &mut prev);

        // Lazy matching: if starting one byte later does better, this position is worth spending a
        // literal on. One extra search per position, and a few percent smaller — which here is not
        // a nicety, since a blob only goes back into a TPK in place if it fits the old slot.
        if len >= MIN_MATCH && pos + 1 < input.len() {
            let next = longest_match(input, pos + 1, &head, &prev);
            if next.0 > len {
                len = 0;
                pending = Some(next);
            }
        }

        // Both flag words are checked before the token is read, `flags1` first.
        let bit1 = flags1.next_bit(&mut out, first);
        let bit2 = flags2.next_bit(&mut out, first);
        first = false;

        if len >= MIN_MATCH {
            flags1.set(&mut out, bit1);
            let (b0, b1) = if dist <= NEAR_MAX_DIST {
                flags2.set(&mut out, bit2);
                let l = len - 3;
                ((((l >> 8) << 4) | (dist - 1)) as u8, (l & 0xFF) as u8)
            } else {
                // The far form's bit stays clear, so its `flags2` bit is spent by being zero.
                let d = dist - 17;
                (((((d >> 8) << 5) | (len - 3)) as u8), (d & 0xFF) as u8)
            };
            out.push(b0);
            out.push(b1);
            for i in pos + 1..pos + len {
                insert(input, i, &mut head, &mut prev);
            }
            pos += len;
            pending = None;
        } else {
            // A literal leaves `flags1`'s bit clear; `flags2`'s bit is *not* consumed by the
            // decoder here, so the encoder must not consume it either.
            flags2.used -= 1;
            out.push(input[pos]);
            pos += 1;
        }
    }

    let total = out.len() as u32;
    out[12..16].copy_from_slice(&total.to_le_bytes());
    Ok(out)
}

fn hash_at(input: &[u8], pos: usize) -> Option<usize> {
    let w = input.get(pos..pos + MIN_MATCH)?;
    let key = u32::from(w[0]) << 16 | u32::from(w[1]) << 8 | u32::from(w[2]);
    // Knuth's multiplicative hash, folded to the table width.
    Some(((key.wrapping_mul(2_654_435_761)) >> (32 - HASH_BITS)) as usize)
}

fn insert(input: &[u8], pos: usize, head: &mut [u32], prev: &mut [u32]) {
    if let Some(h) = hash_at(input, pos) {
        prev[pos] = head[h];
        head[h] = pos as u32;
    }
}

/// The longest match at `pos` the token forms can actually encode, and its distance.
///
/// A distance beyond 2064 is unrepresentable, and the two forms cap length differently — 4098 when
/// the source is within 16 bytes, 34 otherwise — so the search asks for what each candidate can
/// hold rather than for the longest match in the abstract.
fn longest_match(input: &[u8], pos: usize, head: &[u32], prev: &[u32]) -> (usize, usize) {
    let Some(h) = hash_at(input, pos) else { return (0, 0) };
    let (mut best_len, mut best_dist) = (0usize, 0usize);
    let mut candidate = head[h];
    let mut tried = 0;
    while candidate != u32::MAX && tried < MAX_CHAIN {
        let at = candidate as usize;
        let dist = pos - at;
        if dist == 0 || dist > FAR_MAX_DIST {
            break;
        }
        let cap = if dist <= NEAR_MAX_DIST { NEAR_MAX_LEN } else { FAR_MAX_LEN };
        let len = match_len(input, at, pos, cap);
        if len > best_len {
            best_len = len;
            best_dist = dist;
            if best_len == NEAR_MAX_LEN {
                break;
            }
        }
        candidate = prev[at];
        tried += 1;
    }
    (best_len, best_dist)
}

/// How far the bytes at `a` and `b` agree, up to `cap` and the end of the input.
///
/// `a < b` always, and the run may overlap `b` — a copy in the decoder is byte-by-byte, so a
/// distance of 1 legitimately means "repeat this byte".
fn match_len(input: &[u8], a: usize, b: usize, cap: usize) -> usize {
    let limit = cap.min(input.len() - b);
    // Eight bytes at a time while there is room for eight. This is the innermost loop of the
    // compressor — every candidate in every chain runs it — and comparing words instead of bytes is
    // most of the difference between 27 MB/s and something usable. The tail finishes byte-wise.
    let mut n = 0;
    while n + 8 <= limit {
        let (x, y) = (input.get(a + n..a + n + 8), input.get(b + n..b + n + 8));
        let (Some(x), Some(y)) = (x, y) else { break };
        let (Ok(x), Ok(y)) = (<[u8; 8]>::try_from(x), <[u8; 8]>::try_from(y)) else { break };
        let (x, y) = (u64::from_le_bytes(x), u64::from_le_bytes(y));
        if x != y {
            // The first differing byte is where the low-order bits diverge; on little-endian that is
            // the trailing-zero count of the difference, rounded down to a byte.
            return n + ((x ^ y).trailing_zeros() / 8) as usize;
        }
        n += 8;
    }
    while n < limit && input[a + n] == input[b + n] {
        n += 1;
    }
    n
}

fn first_four(buf: &[u8]) -> [u8; 4] {
    let mut out = [0u8; 4];
    for (dst, src) in out.iter_mut().zip(buf.iter()) {
        *dst = *src;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_header_sizes() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"JDLZ");
        buf.push(0x02); // version
        buf.push(0x10); // flags
        buf.extend_from_slice(&[0, 0]); // reserved
        buf.extend_from_slice(&1000u32.to_le_bytes()); // uncompressed
        buf.extend_from_slice(&320u32.to_le_bytes()); // compressed
        let h = parse_header(&buf).unwrap();
        assert_eq!(h.uncompressed_size, 1000);
        assert_eq!(h.compressed_size, 320);
    }

    #[test]
    fn rejects_bad_magic() {
        // 16+ bytes but the wrong magic must be a clean BadMagic, not a panic.
        assert!(matches!(decompress(b"XXXX_not_jdlz_hdr!!"), Err(NfsError::BadMagic { .. })));
    }

    #[test]
    fn garbage_after_valid_header_does_not_panic() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"JDLZ");
        buf.extend_from_slice(&[0x02, 0x10, 0, 0]);
        buf.extend_from_slice(&64u32.to_le_bytes()); // claims 64 output bytes
        buf.extend_from_slice(&40u32.to_le_bytes());
        buf.extend_from_slice(&[0xAB; 24]); // arbitrary token bytes
        let _ = decompress(&buf); // must return (Ok or Err), never panic
    }

    #[test]
    fn near_copy_round_trips() {
        // Emit literals "ABC" then a near copy (flags2 bit set) of dist=3, length=3 → "ABCABC".
        // flags1 = 0x08: tokens 1-3 literals (bits 0-2 = 0), token 4 copy (bit 3 = 1).
        // flags2 = 0x01: first copy uses the near form (bit 0 = 1).
        // near copy operands: length=((b0&0xF0)<<4|b1)+3=3 and dist=(b0&0x0F)+1=3 → b0=0x02, b1=0x00.
        let mut buf = Vec::new();
        buf.extend_from_slice(b"JDLZ");
        buf.extend_from_slice(&[0x02, 0x10, 0, 0]);
        buf.extend_from_slice(&6u32.to_le_bytes()); // uncompressed
        buf.extend_from_slice(&(buf.len() as u32 + 4 + 7).to_le_bytes()); // compressed (informational)
        buf.push(0x08); // flags1
        buf.push(0x01); // flags2
        buf.extend_from_slice(b"ABC");
        buf.extend_from_slice(&[0x02, 0x00]); // near copy: dist=3 len=3
        assert_eq!(decompress(&buf).unwrap(), b"ABCABC");
    }

    /// The encoder's side of the contract: whatever it writes, `decompress` gives back.
    fn round_trip(input: &[u8], what: &str) -> Vec<u8> {
        let packed = compress(input).expect("compress");
        assert_eq!(&packed[..4], b"JDLZ", "{what}: magic");
        assert_eq!(
            u32::from_le_bytes(packed[8..12].try_into().unwrap()) as usize,
            input.len(),
            "{what}: declared uncompressed size"
        );
        assert_eq!(
            u32::from_le_bytes(packed[12..16].try_into().unwrap()) as usize,
            packed.len(),
            "{what}: declared compressed size counts the header, as the game's own files do"
        );
        let back = decompress(&packed).expect("decompress");
        assert_eq!(back, input, "{what}: round trip");
        packed
    }

    #[test]
    fn round_trips_the_shapes_that_break_lz_encoders() {
        round_trip(b"", "empty");
        round_trip(b"A", "one byte");
        round_trip(b"AB", "too short to match");
        round_trip(&[0x5A; 5000], "a long run — distance 1, overlapping copy");
        round_trip(b"ABCABCABCABCABC", "short period");
        // Every token form gets exercised: a near match (distance 3), then a far one.
        let mut mixed = Vec::new();
        mixed.extend_from_slice(b"the quick brown fox jumps over the lazy dog. ");
        mixed.extend_from_slice(&[0u8; 900]);
        mixed.extend_from_slice(b"the quick brown fox jumps over the lazy dog. ");
        round_trip(&mixed, "a far match across a kilobyte of zeroes");
    }

    /// The boundaries between the two token forms, and past the far form's reach.
    #[test]
    fn round_trips_at_the_edges_of_both_token_forms() {
        for dist in [1usize, 15, 16, 17, 18, 2063, 2064, 2065, 4000] {
            // `dist` bytes of noise, then a repeat of the first 40 of them.
            let mut data: Vec<u8> = (0..dist).map(|i| (i * 37 % 251) as u8).collect();
            let head: Vec<u8> = data.iter().take(40.min(dist)).copied().collect();
            data.extend_from_slice(&head);
            round_trip(&data, &format!("distance {dist}"));
        }
        // A match longer than the near form can carry (4098) must be split, not truncated.
        round_trip(&[7u8; 9000], "a run longer than the maximum token length");
    }

    #[test]
    fn compression_actually_compresses_repetitive_data() {
        let data = b"nfsu2 asset toolkit ".repeat(400);
        let packed = round_trip(&data, "repetitive");
        assert!(
            packed.len() * 8 < data.len(),
            "8000 bytes of one repeated phrase should pack to well under an eighth, got {}",
            packed.len()
        );
    }

    #[test]
    fn literal_only_stream_round_trips() {
        // flags1 = 0x00 -> all eight tokens are literals; flags2 unused. Emit 8 literals.
        let mut buf = Vec::new();
        buf.extend_from_slice(b"JDLZ");
        buf.extend_from_slice(&[0x02, 0x10, 0, 0]);
        buf.extend_from_slice(&8u32.to_le_bytes()); // uncompressed
        buf.extend_from_slice(&26u32.to_le_bytes()); // compressed (informational)
        buf.push(0x00); // flags1: 8 literal bits
        buf.push(0x00); // flags2: consumed once at start, unused thereafter
        buf.extend_from_slice(b"ABCDEFGH");
        assert_eq!(decompress(&buf).unwrap(), b"ABCDEFGH");
    }
}
