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
