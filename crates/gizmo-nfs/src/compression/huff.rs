//! EA "HUFF" Huffman decompression (magic ASCII `"HUFF"`, `0x46465548` little-endian).
//!
//! NFSU2 compresses some `TEXTURES.BIN` textures with this codec (others use
//! [`crate::compression::jdlz`]). It is **order-0 Huffman over bytes** — no LZ/back-reference
//! layer — plus a single "clue" escape symbol used for run-length of the previous byte, for
//! literals that have no Huffman code, and for end-of-stream. An optional whole-buffer delta
//! pre-filter (selected by the stream's type word) is undone at the end.
//!
//! The algorithm is a port of EA's `LZCompress` `HUFF_decompress` (the same library NFSU2 calls
//! natively), cross-checked against `dbalatoni13/nfsmw` and `NFSTools/GlobalLib`. It uses the
//! simple, quick-table-free decode path (correctness over speed — texture blobs are small).
//!
//! [`compress`] is the inverse, and it exists because a slot sized by HUFF cannot be refilled by
//! JDLZ: over an install, a HUFF-sourced blob re-packed as JDLZ fits its own slot 60 times in
//! 52,389, and re-packed as HUFF 4,863 times. It is **not** a reproduction of EA's encoder — two
//! Huffman coders that break ties differently both produce valid streams — so it is judged the way
//! [`crate::compression::jdlz`]'s is: on reading back, and on ratio. Every one of the install's
//! 52,389 HUFF blobs re-encodes and decodes to the original bytes, at **1.028×** EA's total size,
//! where JDLZ on the same data is 1.242×.
//!
//! What carries that ratio is not the Huffman. Order-0 coding with the run escape switched off
//! comes to **6.37×** EA; with it, 1.03×. The clue escape is where this format's compression lives,
//! and [`RUN_MIN`] is the whole of the policy.
//!
//! **What is locked and what is only transcribed.** Every HUFF blob in a real install — 52,390 of
//! 52,390 — carries header `(version 1, header size 0x10, flags 0)` and reads its stream type word
//! as **`0x30fb`**. That value is what decides the shape: `& 0x8000 == 0` takes the small-size
//! (24-bit length) branch, `& 0x100 == 0` means no skip words, and it matches no arm of
//! [`undo_delta_filter`], so the plain non-delta path is the one the install exercises and the one
//! validated byte-for-byte. (It is written `0x30fb` and not `0xfb30`, which is how this comment had
//! it: the two differ by a byte swap, and the swapped spelling has the `0x8000` bit set, so it
//! describes the 32-bit branch this format never takes. The conclusions below were right and the
//! number above them was not.) The 32-bit-size branch (`type & 0x8000`), the skip-words
//! branch (`type & 0x100`) and both delta pre-filters have never seen a real byte here; they are
//! transcribed from the sources above and are the first place to look if a blob from another EA
//! title reads as noise. This crate's habit is to say which is which rather than to call all of it
//! validated.

use crate::error::{NfsError, NfsResult};
use crate::reader::ByteReader;

/// The four magic bytes at the start of a HUFF stream.
pub const MAGIC: &[u8; 4] = b"HUFF";

/// Refuse to allocate an output larger than this from the (attacker-controlled) size field.
const MAX_OUTPUT: usize = 256 * 1024 * 1024;
/// Maximum Huffman code length the format uses (tables are sized for this).
const MAX_LEN: usize = 32;

/// The 16-byte HUFF header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HuffHeader {
    /// Declared size of the decompressed output.
    pub uncompressed_size: u32,
    /// Declared size of the compressed stream, **not** counting this 16-byte header.
    ///
    /// The opposite of [`crate::compression::jdlz`]'s field of the same name, which is why this
    /// says so: measured over one install, `compressed_size + 16 == blob length` for 52,390 HUFF
    /// blobs and `compressed_size == blob length` for none of them.
    pub compressed_size: u32,
}

/// Parse the 16-byte HUFF header.
pub fn parse_header(buf: &[u8]) -> NfsResult<HuffHeader> {
    let mut r = ByteReader::new(buf);
    let magic = r.take(4)?;
    if magic != MAGIC.as_slice() {
        return Err(NfsError::BadMagic { context: "huff", found: first_four(buf) });
    }
    let _version = r.u8()?;
    let _header_size = r.u8()?; // 0x10
    let _flags = r.u16_le()?;
    let uncompressed_size = r.u32_le()?;
    let compressed_size = r.u32_le()?;
    Ok(HuffHeader { uncompressed_size, compressed_size })
}

/// A HUFF-specific decompression error.
fn err(detail: &'static str) -> NfsError {
    NfsError::Decompression { codec: "huff", detail }
}

/// MSB-first bit reader over big-endian 16-bit words (EA `SQgetbits` model).
struct Bits<'a> {
    d: &'a [u8],
    pos: usize,
    /// Working accumulator; the next code sits at the top (bit 31).
    bits: u32,
    /// Signed "bits available" counter, relative to the 16-bit refill unit.
    left: i32,
    /// Rolling raw-byte staging register.
    bu: u32,
}

impl<'a> Bits<'a> {
    fn new(d: &'a [u8]) -> Self {
        let mut b = Bits { d, pos: 0, bits: 0, left: -16, bu: 0 };
        b.getbits(0); // prime `bits` with the first word
        b
    }

    /// Append the next big-endian 16-bit word to the low half of the staging register.
    fn get16(&mut self) {
        let hi = self.d.get(self.pos).copied().unwrap_or(0) as u32;
        let lo = self.d.get(self.pos + 1).copied().unwrap_or(0) as u32;
        self.bu = (self.bu << 8) | hi;
        self.bu = (self.bu << 8) | lo;
        self.pos += 2;
    }

    /// Take the top `n` bits (MSB-first), refilling from the stream when underflowed.
    fn getbits(&mut self, n: u32) -> u32 {
        let mut v = 0;
        if n != 0 {
            v = self.bits >> (32 - n);
            self.bits = self.bits.wrapping_shl(n);
            self.left -= n as i32;
        }
        if self.left < 0 {
            self.get16();
            self.bits = self.bu.wrapping_shl((-self.left) as u32);
            self.left += 16;
        }
        v
    }

    /// Consume `n` bits already peeked from the accumulator (no value returned).
    fn consume(&mut self, n: u32) {
        self.bits = self.bits.wrapping_shl(n);
        self.left -= n as i32;
        if self.left < 0 {
            self.get16();
            self.bits = self.bu.wrapping_shl((-self.left) as u32);
            self.left += 16;
        }
    }
}

/// EA `SQgetnum`: a variable-length signed integer, values down to −4.
///
/// The length is a unary run of zeros, so a stream *of* zeros — the shape of truncated or
/// garbage input, since the bit reader pads with zeros past the end — would scan forever. Stop
/// at the longest run the format can encode and report a corrupt stream instead: this parser
/// may not panic or hang on untrusted bytes, only fail.
fn getnum(b: &mut Bits) -> NfsResult<i32> {
    if (b.bits as i32) < 0 {
        // Top bit set → tiny value in `-4..=3`.
        return Ok(b.getbits(3) as i32 - 4);
    }
    let mut n: u32 = 2;
    if b.bits >> 16 != 0 {
        // The terminating 1 lies within the top 16 bits: count leading zeros directly.
        loop {
            b.bits = b.bits.wrapping_shl(1);
            n += 1;
            if (b.bits as i32) < 0 {
                break;
            }
        }
        b.bits = b.bits.wrapping_shl(1); // consume the terminating 1
        b.left -= (n - 1) as i32;
        b.getbits(0); // refill only
    } else {
        // A long run of leading zeros crossing the refill boundary. The top bit has already been
        // tested as zero above, so consume it before counting: the fast path skips it by shifting
        // *before* its first test, and counting it here instead reads the same run as one bit
        // wider. The two branches are one code — only where the terminating 1 falls decides which
        // of them runs — so a stream must not decode differently for having a longer zero run.
        b.getbits(1);
        loop {
            n += 1;
            if b.getbits(1) != 0 {
                break;
            }
            if n as usize > MAX_LEN {
                return Err(err("unterminated length run"));
            }
        }
    }
    let bias = ((1i64 << n) - 4) as i32;
    // Wrapping, not checked: `n` is bounded above, but a corrupt stream can still yield the
    // full u32 range here, and the callers range-check what they get.
    let raw = if n > 16 {
        let hi = b.getbits(n - 16);
        let lo = b.getbits(16);
        (lo | (hi << 16)) as i32
    } else {
        b.getbits(n) as i32
    };
    Ok(raw.wrapping_add(bias))
}

/// Decompress a HUFF stream into its original bytes.
///
/// The five phases below are the format: a prologue, a canonical code-length table, a symbol table,
/// the decode loop, and an optional whole-buffer delta filter. Each is its own function so the
/// shape of the format is readable here rather than buried in 160 lines of bit twiddling.
pub fn decompress(buf: &[u8]) -> NfsResult<Vec<u8>> {
    let header = parse_header(buf)?;
    let out_len = header.uncompressed_size as usize;
    if out_len > MAX_OUTPUT {
        return Err(NfsError::Allocation { requested: out_len });
    }
    let stream = buf.get(16..).ok_or_else(|| err("truncated header"))?;
    let mut b = Bits::new(stream);

    let prologue = prologue(&mut b)?;
    let lengths = code_lengths(&mut b)?;
    let symbols = symbol_table(&mut b, lengths.numchars)?;
    let mut out = decode_symbols(&mut b, &lengths, &symbols, prologue.clue, prologue.ulen)?;
    undo_delta_filter(prologue.type_, &mut out);
    Ok(out)
}

/// The stream's type word, its declared output length, and the escape ("clue") symbol.
struct Prologue {
    type_: u32,
    ulen: usize,
    clue: u8,
}

/// Read the prologue. The type word's top bit picks between 32-bit and 24-bit length fields, and
/// its `0x100` bit means two words of something this decoder does not need are in the way.
fn prologue(b: &mut Bits) -> NfsResult<Prologue> {
    let mut type_ = b.getbits(16);
    let ulen: u32 = if type_ & 0x8000 != 0 {
        // "Big" variant: 32-bit sizes.
        if type_ & 0x100 != 0 {
            b.getbits(16);
            b.getbits(16);
        }
        type_ &= !0x100;
        let hi = b.getbits(16);
        let lo = b.getbits(16);
        lo | (hi << 16)
    } else {
        // "Small" variant: 24-bit sizes.
        if type_ & 0x100 != 0 {
            b.getbits(8);
            b.getbits(16);
        }
        type_ &= !0x100;
        let hi = b.getbits(8);
        let lo = b.getbits(16);
        lo | (hi << 16)
    };
    let clue = b.getbits(8) as u8;
    let ulen = ulen as usize;
    if ulen > MAX_OUTPUT {
        return Err(NfsError::Allocation { requested: ulen });
    }
    Ok(Prologue { type_, ulen, clue })
}

/// The canonical Huffman table: how many codes there are of each length, and what to subtract from
/// a code of that length to get its symbol index.
struct Lengths {
    delta: [u32; MAX_LEN + 1],
    /// Upper limit per length, against which the next 16 bits are compared to find the length.
    cmp_tbl: [u32; MAX_LEN + 2],
    mostbits: usize,
    numchars: usize,
}

fn code_lengths(b: &mut Bits) -> NfsResult<Lengths> {
    let mut delta = [0u32; MAX_LEN + 1];
    let mut cmp_tbl = [0u32; MAX_LEN + 2];
    let mut numchars = 0i32;
    let mut numbits = 1usize;
    let mut basecmp = 0u32;
    loop {
        if numbits > MAX_LEN {
            return Err(err("code length overflow"));
        }
        basecmp <<= 1;
        delta[numbits] = basecmp.wrapping_sub(numchars as u32);
        let bn = getnum(b)?;
        if !(0..=256).contains(&bn) {
            return Err(err("bad code-length count"));
        }
        numchars += bn;
        basecmp = basecmp.wrapping_add(bn as u32);
        let cmp = if bn != 0 { (basecmp << (16 - numbits.min(16))) & 0xffff } else { 0 };
        cmp_tbl[numbits] = cmp;
        numbits += 1;
        if bn == 0 || cmp != 0 {
            continue;
        }
        break;
    }
    let mostbits = numbits - 1;
    cmp_tbl[mostbits] = 0xffff_ffff; // sentinel
    if numchars <= 0 || numchars > 256 {
        return Err(err("bad symbol count"));
    }
    Ok(Lengths { delta, cmp_tbl, mostbits, numchars: numchars as usize })
}

/// Which byte each code stands for, read as "leaps" over the symbols already claimed.
fn symbol_table(b: &mut Bits, numchars: usize) -> NfsResult<[u8; 256]> {
    let mut codetbl = [0u8; 256];
    let mut used = [false; 256];
    let mut nextchar: u8 = 0xFF;
    for (claimed, slot) in codetbl.iter_mut().take(numchars).enumerate() {
        // A leap counts *free* slots, so it can never exceed how many are left: asking for the
        // n-th free byte when only m < n remain means walking the alphabet round again and
        // claiming one twice, which no encoder can mean. Unbounded, this is worse than merely
        // wrong — `getnum` legitimately returns up to `i32::MAX`, and the walk below only counts
        // down when it lands on a free slot, so 256 maximal leaps in 1,952 bytes spin for hours.
        // Bounded, the walk is at most one lap: a full cycle of 256 meets every free slot once.
        let free = 256 - claimed;
        let mut leap = getnum(b)?.saturating_add(1);
        if leap < 1 || leap as usize > free {
            return Err(err("symbol leap past the end of the alphabet"));
        }
        loop {
            nextchar = nextchar.wrapping_add(1);
            if !used[nextchar as usize] {
                leap -= 1;
            }
            if leap == 0 {
                break;
            }
        }
        used[nextchar as usize] = true;
        *slot = nextchar;
    }
    Ok(codetbl)
}

/// The decode loop: read a code, emit its byte, and treat the clue symbol as run-length, a raw
/// literal, or end-of-stream.
fn decode_symbols(
    b: &mut Bits,
    lengths: &Lengths,
    codetbl: &[u8; 256],
    clue: u8,
    ulen: usize,
) -> NfsResult<Vec<u8>> {
    let mut out = Vec::new();
    out.try_reserve(ulen).map_err(|_| NfsError::Allocation { requested: ulen })?;
    // A malformed stream must not spin: every iteration either emits a byte or ends, so a bound of
    // four times the declared output is generous and still finite.
    let mut guard = 0usize;
    let guard_max = ulen.saturating_mul(4).saturating_add(4096);
    while out.len() < ulen {
        guard += 1;
        if guard > guard_max {
            return Err(err("decode did not terminate"));
        }
        // Determine this code's length by comparing the top 16 bits against the limits.
        let cmp16 = b.bits >> 16;
        let mut len = 1usize;
        while len < lengths.mostbits && cmp16 >= lengths.cmp_tbl[len] {
            len += 1;
        }
        let code_val = b.bits >> (32 - len as u32);
        b.consume(len as u32);
        let idx = code_val.wrapping_sub(lengths.delta[len]) as usize;
        let code = *codetbl.get(idx).ok_or_else(|| err("symbol index out of range"))?;

        if code != clue {
            out.push(code);
            continue;
        }
        // Clue escape: run-length, raw literal, or end-of-stream.
        let runlen = getnum(b)?;
        if runlen != 0 {
            if runlen < 0 {
                return Err(err("negative run length"));
            }
            let prev = *out.last().ok_or_else(|| err("run before any output"))?;
            for _ in 0..runlen {
                if out.len() >= ulen {
                    break;
                }
                out.push(prev);
            }
        } else if b.getbits(1) != 0 {
            break; // end of stream
        } else {
            out.push(b.getbits(8) as u8);
        }
    }
    out.truncate(ulen);
    Ok(out)
}

/// Undo the whole-buffer delta pre-filter the type word selects: one pass of running sums, or two.
fn undo_delta_filter(type_: u32, out: &mut [u8]) {
    match type_ {
        0x32fb | 0xb2fb => {
            let mut acc = 0u32;
            for x in out {
                acc = acc.wrapping_add(u32::from(*x));
                *x = acc as u8;
            }
        }
        0x34fb | 0xb4fb => {
            let (mut acc, mut acc2) = (0u32, 0u32);
            for x in out {
                acc = acc.wrapping_add(u32::from(*x));
                acc2 = acc2.wrapping_add(acc);
                *x = acc2 as u8;
            }
        }
        _ => {}
    }
}

fn first_four(buf: &[u8]) -> [u8; 4] {
    let mut out = [0u8; 4];
    for (o, b) in out.iter_mut().zip(buf.iter()) {
        *o = *b;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zero_filled_stream_fails_instead_of_spinning() {
        // A valid magic + version and nothing else. The bit reader pads with zeros past the end,
        // so `getnum`'s unary length run never terminates: it used to count until `n` overflowed
        // (a panic in debug, a nonsense shift in release). Untrusted input may only fail.
        let mut buf = vec![0u8; 16];
        buf[..4].copy_from_slice(MAGIC);
        buf[4] = 1;
        assert!(matches!(decompress(&buf), Err(NfsError::Decompression { codec: "huff", .. })));
    }

    #[test]
    fn a_truncated_header_is_an_error_not_a_panic() {
        assert!(decompress(b"HUFF").is_err());
        assert!(parse_header(&[]).is_err());
    }

    /// Append the low `n` bits of `v`, most significant first.
    fn push_bits(bits: &mut Vec<u8>, v: u64, n: u32) {
        for i in (0..n).rev() {
            bits.push(((v >> i) & 1) as u8);
        }
    }

    /// Under 2 KB that used to occupy the decoder for minutes.
    ///
    /// `getnum` legitimately returns values up to `i32::MAX`, and `symbol_table`'s walk counted
    /// down once per *free* slot with no bound — so 256 maximal leaps is billions of iterations
    /// from a file that declares a 16-byte output. The proptest in `tests/no_panic.rs` cannot
    /// reach it: a maximal leap needs a 28-zero unary run followed by a 31-bit field, which random
    /// bytes do not produce. And a hang is not a panic, so nothing else here would have caught it.
    #[test]
    fn a_maximal_symbol_leap_fails_instead_of_running_for_minutes() {
        let mut bits = Vec::new();
        push_bits(&mut bits, 0x30fb, 16); // the type word every real blob carries
        push_bits(&mut bits, 0, 8);
        push_bits(&mut bits, 16, 16); // declared output length
        push_bits(&mut bits, 0, 8); // clue
        push_bits(&mut bits, 0, 6); // code lengths: one length holding all 256 symbols,
        push_bits(&mut bits, 1, 1); // so the table closes immediately and the symbol
        push_bits(&mut bits, 4, 8); // table below is reached with numchars = 256
        for _ in 0..256 {
            push_bits(&mut bits, 0, 28); // a 28-zero unary run …
            push_bits(&mut bits, 1, 1); // … terminated, so the field is 31 bits wide
            push_bits(&mut bits, 0, 31); // → getnum ≈ 2^31, i.e. a maximal leap
        }
        push_bits(&mut bits, 0, 64);

        let body: Vec<u8> = bits
            .chunks(8)
            .map(|c| c.iter().fold(0u8, |b, bit| (b << 1) | bit) << (8 - c.len()))
            .collect();
        let mut buf = Vec::from(MAGIC.as_slice());
        buf.extend_from_slice(&[1, 0x10, 0, 0]); // version, header size, flags
        buf.extend_from_slice(&16u32.to_le_bytes()); // uncompressed size
        buf.extend_from_slice(&(body.len() as u32).to_le_bytes());
        buf.extend_from_slice(&body);

        let started = std::time::Instant::now();
        let out = decompress(&buf);
        // The bound is generous by three orders of magnitude: the fixed path answers in microseconds
        // and the unbounded one took over fifteen seconds before it was killed.
        assert!(started.elapsed().as_secs() < 5, "decompress took {:?}", started.elapsed());
        assert!(matches!(out, Err(NfsError::Decompression { codec: "huff", .. })), "{out:?}");
    }
}

// ---------------------------------------------------------------------------------------------
// Encoding
// ---------------------------------------------------------------------------------------------

/// The one stream shape this encoder writes, and the only one the game was ever measured to ship.
///
/// All 52,390 HUFF blobs in an install read their type word as `0x30fb`: `& 0x8000 == 0` picks the
/// 24-bit length fields, `& 0x100 == 0` means no skip words, and it matches no arm of
/// [`undo_delta_filter`], so no pre-filter is undone. Writing anything else would be writing a
/// variant nothing here has seen read back.
const TYPE_WORD: u32 = 0x30fb;

/// The longest code this encoder will emit.
///
/// Not the format's limit — [`MAX_LEN`] is 32 — but EA's. Across the install the deepest code in a
/// real blob is 15 bits and the shallowest tree tops out at 7, so a limit of 15 stays inside what
/// the game's own files demonstrate. It also keeps `cmp_tbl`'s `16 - numbits` shift positive, which
/// is where a longer code would start meaning something this decoder does not implement.
const MAX_ENCODED_LEN: usize = 15;

/// MSB-first bit writer, the mirror of [`Bits`].
///
/// The reader refills sixteen bits at a time from big-endian pairs, but what it consumes is simply
/// the bit sequence of the bytes in order, so this writes bytes and pads the tail to an even length.
struct BitWriter {
    out: Vec<u8>,
    acc: u32,
    n: u32,
}

impl BitWriter {
    fn new() -> Self {
        BitWriter { out: Vec::new(), acc: 0, n: 0 }
    }

    /// Append the low `bits` of `v`, most significant first.
    fn put(&mut self, v: u32, bits: u32) {
        for i in (0..bits).rev() {
            self.acc = (self.acc << 1) | ((v >> i) & 1);
            self.n += 1;
            if self.n == 8 {
                self.out.push(self.acc as u8);
                self.acc = 0;
                self.n = 0;
            }
        }
    }

    /// Flush to a whole number of 16-bit words, which is the unit the reader refills in.
    fn finish(mut self) -> Vec<u8> {
        if self.n != 0 {
            self.acc <<= 8 - self.n;
            self.out.push(self.acc as u8);
        }
        if !self.out.len().is_multiple_of(2) {
            self.out.push(0);
        }
        self.out
    }
}

/// The inverse of [`getnum`].
///
/// One rule covers both of the decoder's branches. A value `v` is written as `n - 2` zeroes, a `1`,
/// and `n` bits of `v - (2^n - 4)`, for the smallest `n >= 2` that fits — which for `n == 2` is the
/// three-bit form the decoder takes when it sees the top bit set, and for larger `n` is its
/// leading-zero count. The ranges tile exactly: `n=2` covers 0..=3, `n=3` covers 4..=11, `n=4`
/// covers 12..=27, and so on, each starting one past where the last ended.
fn putnum(w: &mut BitWriter, v: u32) {
    let mut n = 2u32;
    // The largest value `n` can express is `2^(n+1) - 5`.
    while n < 31 && v > (1u32 << (n + 1)).wrapping_sub(5) {
        n += 1;
    }
    let bias = (1u32 << n) - 4;
    w.put(0, n - 2); // the unary run of zeroes
    w.put(1, 1); // its terminator
    w.put(v - bias, n);
}

/// Code lengths for a canonical Huffman code over `freq`, none longer than [`MAX_ENCODED_LEN`].
///
/// Plain Huffman first; if that overflows the limit, the lengths are flattened and repaired so the
/// Kraft sum comes back to exactly one. The code has to be *complete* — the decoder's table loop
/// ends only when the code space is exactly filled — so "close enough" is not a thing here.
fn encode_lengths(freq: &[u64; 256]) -> [u8; 256] {
    let live: Vec<usize> = (0..256).filter(|&b| freq[b] > 0).collect();
    let mut len = [0u8; 256];
    if live.len() <= 1 {
        // One symbol is not a code. A single one-bit code claims half the space, and the decoder's
        // table loop ends only when the space is exactly *full* — so it would go on reading counts
        // past the end of the table and fail as an unterminated run. Two one-bit codes fill it, so
        // a second symbol is invented; it has no frequency and is never emitted, and it costs one
        // leap in the symbol table.
        let only = live.first().copied().unwrap_or(0);
        len[only] = 1;
        len[if only == 0 { 1 } else { 0 }] = 1;
        return len;
    }

    // Huffman by repeated merge. Nodes are (weight, left, right); leaves have no children.
    let mut w: Vec<u64> = Vec::new();
    let mut kids: Vec<(i32, i32)> = Vec::new();
    let mut leaf = [usize::MAX; 256];
    for &b in &live {
        leaf[b] = w.len();
        w.push(freq[b]);
        kids.push((-1, -1));
    }
    let mut pool: Vec<usize> = (0..w.len()).collect();
    while pool.len() > 1 {
        // Smallest two. Ties broken by index so the result does not depend on sort stability.
        pool.sort_unstable_by_key(|&i| (std::cmp::Reverse(w[i]), i));
        let a = pool.pop().expect("two nodes");
        let b = pool.pop().expect("two nodes");
        w.push(w[a] + w[b]);
        kids.push((a as i32, b as i32));
        pool.push(w.len() - 1);
    }

    let mut depth = vec![0u32; w.len()];
    let mut stack = vec![(pool[0], 0u32)];
    while let Some((n, d)) = stack.pop() {
        let (l, r) = kids[n];
        if l < 0 {
            depth[n] = d.max(1);
            continue;
        }
        stack.push((l as usize, d + 1));
        stack.push((r as usize, d + 1));
    }
    for &b in &live {
        len[b] = depth[leaf[b]].min(u32::from(u8::MAX)) as u8;
    }
    limit_lengths(&mut len, &live);
    len
}

/// Pull every code back inside [`MAX_ENCODED_LEN`] and restore a complete code.
///
/// Clamping alone breaks the Kraft sum — several codes shortened to the limit overfill the space —
/// so afterwards the sum is repaired: while it is over, lengthen the currently-shortest codes; while
/// it is under, shorten the longest. It ends at exactly `2^MAX`, which is the completeness the
/// decoder's table loop requires.
fn limit_lengths(len: &mut [u8; 256], live: &[usize]) {
    for &b in live {
        if len[b] as usize > MAX_ENCODED_LEN {
            len[b] = MAX_ENCODED_LEN as u8;
        }
    }
    let full = 1u64 << MAX_ENCODED_LEN;
    let kraft = |len: &[u8; 256]| -> u64 {
        live.iter().map(|&b| 1u64 << (MAX_ENCODED_LEN - len[b] as usize)).sum()
    };
    let mut sum = kraft(len);
    // Too much code space claimed: lengthen shallow codes, cheapest first.
    while sum > full {
        let pick = live
            .iter()
            .copied()
            .filter(|&b| (len[b] as usize) < MAX_ENCODED_LEN)
            .min_by_key(|&b| len[b])
            .expect("a code shorter than the limit exists while the sum is over");
        sum -= 1u64 << (MAX_ENCODED_LEN - len[pick] as usize);
        len[pick] += 1;
        sum += 1u64 << (MAX_ENCODED_LEN - len[pick] as usize);
    }
    // Room to spare: shorten the deepest codes until it is exactly filled.
    while let Some(&pick) = live
        .iter()
        .filter(|&&b| len[b] > 1)
        .max_by_key(|&&b| len[b])
        .filter(|&&b| sum + (1u64 << (MAX_ENCODED_LEN - len[b] as usize)) <= full)
    {
        sum += 1u64 << (MAX_ENCODED_LEN - len[pick] as usize);
        len[pick] -= 1;
    }
}

/// One thing the encoder decided to emit.
enum Op {
    /// A byte, by its Huffman code.
    Sym(u8),
    /// Repeat the byte just emitted `n` more times, through the clue escape.
    Run(u32),
    /// A byte with no code of its own — the clue's own value, which cannot be emitted directly.
    Literal(u8),
}

/// Split `data` into symbols and runs, and count how often each byte is coded.
///
/// A run costs the clue's code plus a [`putnum`], against the byte's own code repeated. Where the
/// break-even sits depends on the code lengths, which are not known until the frequencies are — so
/// this uses a fixed threshold rather than iterating to a fixed point. Four is where a run stops
/// being able to lose: the clue and the length together are never worse than four copies of a byte
/// whose code is at most fifteen bits, and runs of three or less are left as plain symbols.
///
/// This matters more than the Huffman does. Measured over the install's 52,390 HUFF blobs, order-0
/// Huffman with no runs at all comes to **6.37×** what EA's encoder achieves; with runs it comes to
/// 1.04×. The clue escape is where this format's compression lives.
const RUN_MIN: usize = 4;

fn plan(data: &[u8], clue: u8) -> (Vec<Op>, [u64; 256]) {
    let mut ops = Vec::new();
    let mut freq = [0u64; 256];
    let mut i = 0;
    while i < data.len() {
        let b = data[i];
        let mut j = i + 1;
        while j < data.len() && data[j] == b {
            j += 1;
        }
        let run = j - i;
        // The clue's own byte has no direct code: every occurrence is an escaped literal.
        if b == clue {
            for _ in 0..run {
                ops.push(Op::Literal(b));
                freq[clue as usize] += 1;
            }
            i = j;
            continue;
        }
        ops.push(Op::Sym(b));
        freq[b as usize] += 1;
        if run >= RUN_MIN {
            ops.push(Op::Run((run - 1) as u32));
            freq[clue as usize] += 1;
        } else {
            for _ in 1..run {
                ops.push(Op::Sym(b));
                freq[b as usize] += 1;
            }
        }
        i = j;
    }
    // The end-of-stream marker is one more use of the clue.
    freq[clue as usize] += 1;
    (ops, freq)
}

/// Which byte to make the escape.
///
/// Any byte can be it, but every occurrence of it in the data then costs an escaped literal — so
/// the cheapest choice is a byte that does not occur at all, and failing that the rarest one. EA
/// evidently does the same: across the install the clue takes 254 distinct values, so it is chosen
/// per blob rather than fixed, and the commonest choices (`0xD0`, `0xFE`, `0xFD`) are bytes that are
/// rare in texture data.
fn choose_clue(data: &[u8]) -> u8 {
    let mut count = [0u64; 256];
    for &b in data {
        count[b as usize] += 1;
    }
    // Lowest count wins; ties go to the higher byte value, which is where the unused ones cluster.
    (0..256)
        .rev()
        .min_by_key(|&b| count[b])
        .map_or(0xFF, |b| b as u8)
}

/// Compress `input` into a HUFF stream this crate's [`decompress`] reads back.
///
/// It writes exactly one shape — the [`TYPE_WORD`] one every real blob in an install uses — because
/// that is the only shape there is evidence for. **It does not try to reproduce EA's byte stream.**
/// Two Huffman encoders that break frequency ties differently, or draw the run/literal line in
/// different places, both produce valid streams; the same reasoning [`crate::compression::jdlz`]
/// records applies here.
///
/// So it is judged on reading back — a proptest over arbitrary bytes, and every real HUFF blob in an
/// install re-encoded and decoded again — and on ratio, which is what decides whether a rewritten
/// texture still fits the slot it came out of.
///
/// # Errors
/// When the input is longer than the 24-bit length field this stream shape carries (16 MiB − 1).
pub fn compress(input: &[u8]) -> NfsResult<Vec<u8>> {
    const MAX_24: usize = (1 << 24) - 1;
    if input.len() > MAX_24 {
        return Err(NfsError::Allocation { requested: input.len() });
    }

    let clue = choose_clue(input);
    let (ops, freq) = plan(input, clue);
    let lengths = encode_lengths(&freq);

    // Canonical codes: symbols ordered by (length, value), codes ascending within a length.
    let mut order: Vec<u8> = (0..=255u8).filter(|&b| lengths[b as usize] > 0).collect();
    order.sort_by_key(|&b| (lengths[b as usize], b));
    let maxlen = order.iter().map(|&b| lengths[b as usize] as usize).max().unwrap_or(0);
    let mut count = [0usize; MAX_ENCODED_LEN + 2];
    for &b in &order {
        count[lengths[b as usize] as usize] += 1;
    }
    let mut code = [0u32; 256];
    let mut next = 0u32;
    for l in 1..=maxlen {
        for &b in &order {
            if lengths[b as usize] as usize == l {
                code[b as usize] = next;
                next += 1;
            }
        }
        next <<= 1;
    }

    let mut w = BitWriter::new();
    w.put(TYPE_WORD, 16);
    // 24-bit length, high byte first — the small-size branch of the prologue.
    w.put((input.len() >> 16) as u32, 8);
    w.put((input.len() & 0xffff) as u32, 16);
    w.put(u32::from(clue), 8);

    // The code-length table: one count per length, stopping at the length that fills the code
    // space. The decoder's loop ends on exactly that condition, so an incomplete code would leave
    // it reading counts forever.
    for c in count.iter().take(maxlen + 1).skip(1) {
        putnum(&mut w, *c as u32);
    }

    // The symbol table, as leaps over the alphabet's still-free slots.
    let mut used = [false; 256];
    let mut nextchar: u8 = 0xFF;
    for &b in &order {
        let mut leap = 0u32;
        let mut probe = nextchar;
        loop {
            probe = probe.wrapping_add(1);
            if !used[probe as usize] {
                leap += 1;
            }
            if probe == b {
                break;
            }
        }
        putnum(&mut w, leap - 1);
        used[b as usize] = true;
        nextchar = b;
    }

    // The symbols themselves.
    let emit = |w: &mut BitWriter, b: u8| {
        w.put(code[b as usize], u32::from(lengths[b as usize]));
    };
    for op in &ops {
        match op {
            Op::Sym(b) => emit(&mut w, *b),
            Op::Run(n) => {
                emit(&mut w, clue);
                putnum(&mut w, *n);
            }
            Op::Literal(b) => {
                emit(&mut w, clue);
                putnum(&mut w, 0);
                w.put(0, 1);
                w.put(u32::from(*b), 8);
            }
        }
    }
    // End of stream: the clue, a zero run length, and a set bit.
    emit(&mut w, clue);
    putnum(&mut w, 0);
    w.put(1, 1);

    let stream = w.finish();
    let mut out = Vec::with_capacity(16 + stream.len());
    out.extend_from_slice(MAGIC);
    out.push(1); // version
    out.push(0x10); // header size
    out.extend_from_slice(&0u16.to_le_bytes()); // flags
    out.extend_from_slice(&(input.len() as u32).to_le_bytes());
    out.extend_from_slice(&(stream.len() as u32).to_le_bytes());
    out.extend_from_slice(&stream);
    Ok(out)
}

#[cfg(test)]
mod encode_tests {
    use super::*;

    fn round_trip(data: &[u8]) {
        let packed = compress(data).expect("compress");
        assert_eq!(&packed[..4], MAGIC, "magic");
        let back = decompress(&packed).expect("decompress what we just wrote");
        assert_eq!(back, data, "round trip differs for {} bytes", data.len());
    }

    /// The shapes that break a Huffman encoder: nothing, one symbol, two, all of them, and a long
    /// run of one byte — which is the case the clue escape exists for.
    #[test]
    fn the_awkward_inputs_round_trip() {
        round_trip(&[]);
        round_trip(&[0]);
        round_trip(&[0xAB]);
        round_trip(&[7; 1]);
        round_trip(&[7; 2]);
        round_trip(&[7; 3]);
        round_trip(&[7; 4]);
        round_trip(&[7; 5000]);
        round_trip(&[0, 1]);
        round_trip(&(0..=255u8).collect::<Vec<_>>());
        round_trip(&(0..=255u8).cycle().take(4096).collect::<Vec<_>>());
    }

    /// Every byte value present, including whichever one becomes the clue — so the escaped-literal
    /// path is exercised rather than only described.
    #[test]
    fn a_full_alphabet_forces_the_literal_escape() {
        let mut data: Vec<u8> = (0..=255u8).collect();
        // Make one byte overwhelmingly common so the clue lands on a byte that *is* in the data.
        data.extend(std::iter::repeat_n(0x41, 4000));
        round_trip(&data);
        let clue = choose_clue(&data);
        assert!(data.contains(&clue), "this test is pointless unless the clue occurs in the data");
    }

    /// `putnum` and `getnum` are inverses across every range boundary the tiling has.
    #[test]
    fn putnum_is_getnums_inverse() {
        let mut values: Vec<u32> = (0..600).collect();
        values.extend([1023, 1024, 65_535, 65_536, 1 << 20, (1 << 24) - 1]);
        for v in values {
            let mut w = BitWriter::new();
            putnum(&mut w, v);
            // A reader needs a whole word to prime, so pad generously.
            let mut bytes = w.finish();
            bytes.extend_from_slice(&[0; 8]);
            let mut b = Bits::new(&bytes);
            assert_eq!(getnum(&mut b).expect("getnum"), v as i32, "putnum({v}) did not read back");
        }
    }

    /// A code the decoder can use at all has to be *complete*: its table loop ends only when the
    /// code space is exactly filled, so a Kraft sum under one would leave it reading counts past
    /// the end of the table.
    #[test]
    fn the_code_is_always_complete_and_within_the_limit() {
        let cases: Vec<Vec<u8>> = vec![
            vec![1],
            vec![1, 2],
            (0..=255u8).collect(),
            // Wildly skewed, which is what drives codes past the length limit.
            {
                let mut v = vec![0u8; 60_000];
                for (i, x) in v.iter_mut().enumerate() {
                    *x = if i % 5000 == 0 { (i / 5000) as u8 } else { 0 };
                }
                v
            },
        ];
        for data in cases {
            let clue = choose_clue(&data);
            let (_, freq) = plan(&data, clue);
            let len = encode_lengths(&freq);
            let live: Vec<usize> = (0..256).filter(|&b| freq[b] > 0).collect();
            let sum: u64 =
                live.iter().map(|&b| 1u64 << (MAX_ENCODED_LEN - len[b] as usize)).sum();
            assert_eq!(sum, 1u64 << MAX_ENCODED_LEN, "code space not exactly filled");
            for &b in &live {
                assert!(len[b] >= 1 && len[b] as usize <= MAX_ENCODED_LEN, "length {}", len[b]);
            }
        }
    }
}
