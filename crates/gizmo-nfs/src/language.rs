//! The game's text: `LANGUAGES/<language>.bin`, chunk `0x00039000`.
//!
//! The payload opens with four words — `0x10`, the entry count, the offset of the entry table and
//! the offset of the strings, both from the start of the payload — then the table: one
//! `(label hash, string offset)` pair per entry, the offset counted from the strings. Strings are
//! NUL-terminated single-byte text. `English.bin` holds 3 925 of them; the table ends exactly
//! where the strings begin.
//!
//! The hashes are what other files name text by: the style events (`globalb::style`) carry
//! `DRIFT`, `POWER SLIDE`, … as hashes, not as words.

use std::collections::HashMap;

/// The chunk id.
pub const CHUNK: u32 = 0x0003_9000;

/// Every label in a language file, by hash. Empty if the file is not one.
#[must_use]
pub fn labels(bytes: &[u8]) -> HashMap<u32, String> {
    let word = |o: usize| -> Option<u32> {
        bytes.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let mut out = HashMap::new();
    if word(0) != Some(CHUNK) {
        return out;
    }
    let base = 8usize;
    let (Some(count), Some(table), Some(strings)) = (word(base + 4), word(base + 8), word(base + 12))
    else {
        return out;
    };
    for i in 0..count as usize {
        let at = base + table as usize + i * 8;
        let (Some(hash), Some(off)) = (word(at), word(at + 4)) else { break };
        let from = base + strings as usize + off as usize;
        let Some(tail) = bytes.get(from..) else { continue };
        let end = tail.iter().position(|&b| b == 0).unwrap_or(tail.len());
        // Single-byte text; read as Latin-1, which maps every byte to the code point of the same
        // number and so never fails.
        out.insert(hash, tail[..end].iter().map(|&b| char::from(b)).collect());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_two_entry_file_reads_back() {
        let strings = b"DRIFT\0HANG TIME\0";
        let table_off = 0x10u32;
        let strings_off = table_off + 16;
        let mut b = Vec::new();
        b.extend_from_slice(&CHUNK.to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        for w in [0x10u32, 2, table_off, strings_off] {
            b.extend_from_slice(&w.to_le_bytes());
        }
        for (h, o) in [(0xA46A_AEA8u32, 0u32), (0x4416_7CBC, 6)] {
            b.extend_from_slice(&h.to_le_bytes());
            b.extend_from_slice(&o.to_le_bytes());
        }
        b.extend_from_slice(strings);
        let l = labels(&b);
        assert_eq!(l.get(&0xA46A_AEA8).map(String::as_str), Some("DRIFT"));
        assert_eq!(l.get(&0x4416_7CBC).map(String::as_str), Some("HANG TIME"));
    }
}
