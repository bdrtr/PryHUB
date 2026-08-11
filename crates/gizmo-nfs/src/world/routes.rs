//! The route files: `TRACKS/ROUTES*/Paths*.bin`.
//!
//! 113 files across eight `ROUTES*` directories, one per race route. Each is a single
//! `0x80034147` container holding up to five leaves; every file has `0x0003414A` and `0x0003414D`,
//! and 105 of the 113 also carry `0x00034148`, `0x00034149` and `0x0003414C`.
//!
//! **Only `0x00034148` is read here.** It is the node table — the graph a race route is driven
//! along — and it is the one leaf whose record boundary the files prove rather than suggest:
//! every one of the 105 payloads is a whole number of 24-byte records, 18,064 nodes in all.
//!
//! ## The record, and what pins each field
//!
//! | offset | type | what it is |
//! |---|---|---|
//! | `+0`, `+4` | `f32` | position, in the world's own frame and units (Z-up, so these are X and Y and there is no height) |
//! | `+8` | `u16` | a small integer — see below, **not** interpreted |
//! | `+10` | `u16` | two bytes with `0xFF` used as a sentinel — **not** interpreted |
//! | `+12`, `+14`, `+16` | `u16` | node indices into this file's own table, `0xFFFF` for none |
//! | `+18` | `u16` | zero in all 18,064 records |
//! | `+20` | `f32` | distance along the route, cumulative, in the same unit as `+0`/`+4` |
//!
//! The two claims worth having are measured, not assumed:
//!
//! - **The three index fields really are indices.** Across all 105 tables that is 54,192 reads, and
//!   **not one** holds a value that is neither `0xFFFF` nor smaller than its own file's node count.
//!   A field that were anything else — a distance, a bitfield, a hash — would land outside on some
//!   record out of eighteen thousand.
//! - **`+20` is in the same unit as the coordinates.** Over the 7,277 consecutive in-run pairs (a
//!   step under 100 m with the value rising), the ratio of its delta to the XY step has median
//!   **1.0005**, p05 0.884, p95 1.054. It is cumulative arc length in world units — the small
//!   spread is the height this record does not carry, plus corners cut by a 2-D chord.
//!
//!   This corrects a figure carried in the notes for a while, that the deltas were ≈0.787 of the
//!   XY step. That ratio comes out of pairs taken through the *index* fields rather than through
//!   consecutive records: those are not neighbours in the driving sense, and sampled that way the
//!   median lands at 38.7 rather than near 1. What made the wrong number believable is that 0.785
//!   is π/4, which reads like a derivation rather than a sampling accident.
//!
//! ## What is deliberately not named
//!
//! `+8` is 0..112 across the install with 113 distinct values, and in 18,064 of 18,064 records it
//! *would* be a valid index into its own table — including in the five-node files, where that is a
//! real constraint. That is suggestive and it is not proof: any small integer passes the same test.
//! `+10`'s two bytes each use `0xFF` heavily (3,616 records have `0xFF` in the high byte) which is
//! how the other optional fields here spell "none". Both are carried through as raw words so a
//! caller can measure them further; naming a field is a claim, and neither is earned yet.
//!
//! The other four leaves are not read. `0x0003414D` is a whole number of 36-byte units in every
//! file and `0x0003414C` of 16-byte ones; `0x0003414A` divides by nothing above 4, so it is either
//! variable-length or headed.

use crate::chunk::{walk, Visit};
use crate::error::{NfsError, NfsResult};
use crate::reader::ByteReader;

/// The node-table leaf.
pub const NODE_CHUNK: u32 = 0x0003_4148;

/// One node record's length. Every `0x00034148` payload in the install is a whole multiple of it.
pub const NODE_LEN: usize = 24;

/// The value the index fields use for "no node here".
pub const NO_LINK: u16 = 0xFFFF;

/// One node of a route's graph.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RouteNode {
    /// Position as the file states it: the world's own frame, no fixup, no height.
    pub x: f32,
    pub y: f32,
    /// `+8`, uninterpreted — see the module docs for what is and is not known about it.
    pub word_8: u16,
    /// `+10`, uninterpreted.
    pub word_10: u16,
    /// `+18`, uninterpreted — zero in all 18,064 records of the install, and carried rather than
    /// asserted so [`check`] can say so about a given file instead of this reader refusing it.
    pub word_18: u16,
    /// `+12`, `+14`, `+16`: indices into the same file's node table, `None` where the file wrote
    /// [`NO_LINK`].
    pub links: [Option<u16>; 3],
    /// `+20`: distance along the route so far, in world units.
    pub progress: f32,
}

impl RouteNode {
    /// The links that are present, in file order.
    pub fn linked(&self) -> impl Iterator<Item = u16> + '_ {
        self.links.iter().filter_map(|l| *l)
    }
}

/// Read one `0x00034148` payload into nodes.
///
/// # Errors
///
/// [`NfsError::CorruptArchive`] if the payload is not a whole number of records — which is the
/// file saying this is not the table it claims to be, since all 105 in the install are.
pub fn read_nodes(payload: &[u8]) -> NfsResult<Vec<RouteNode>> {
    if !payload.len().is_multiple_of(NODE_LEN) {
        return Err(NfsError::CorruptArchive {
            detail: "0x00034148 payload is not a whole number of 24-byte node records",
        });
    }
    let mut out = Vec::with_capacity(payload.len() / NODE_LEN);
    for record in payload.chunks_exact(NODE_LEN) {
        let mut r = ByteReader::at(record, 0)?;
        let x = r.f32_le()?;
        let y = r.f32_le()?;
        let word_8 = r.u16_le()?;
        let word_10 = r.u16_le()?;
        let link = |v: u16| (v != NO_LINK).then_some(v);
        let links = [link(r.u16_le()?), link(r.u16_le()?), link(r.u16_le()?)];
        let word_18 = r.u16_le()?;
        let progress = r.f32_le()?;
        out.push(RouteNode { x, y, word_8, word_10, word_18, links, progress });
    }
    Ok(out)
}

/// Every node a route file declares, in file order.
///
/// A file with no `0x00034148` yields an empty vector rather than an error: eight of the install's
/// 113 route files genuinely carry none, and a reader that refuses them cannot walk the set.
///
/// # Errors
///
/// Propagates a walk failure, or [`NfsError::CorruptArchive`] from [`read_nodes`].
pub fn nodes(file: &[u8]) -> NfsResult<Vec<RouteNode>> {
    let mut out = Vec::new();
    let mut failed = None;
    walk(
        file,
        |_, _| Ok(Visit::Descend),
        |_, header, data| {
            if header.id == NODE_CHUNK {
                match read_nodes(data) {
                    Ok(mut n) => out.append(&mut n),
                    Err(e) => failed = Some(e),
                }
            }
            Ok(())
        },
    )?;
    match failed {
        Some(e) => Err(e),
        None => Ok(out),
    }
}

/// What a node table checks out as, so a caller can print it rather than re-derive it.
///
/// The point of the type is that every field is a question the file can answer about itself. A
/// reading that is wrong fails one of them on some record out of eighteen thousand; a reading that
/// is right fails none, and that is the difference between this and a plausible guess.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct NodeCheck {
    pub nodes: usize,
    /// Index fields read: three per node.
    pub links_read: usize,
    /// Of those, how many said [`NO_LINK`].
    pub links_absent: usize,
    /// **Must be zero.** A link that is neither absent nor a valid index into this table.
    pub links_out_of_range: usize,
    /// **Must be zero.** A `+18` that is not zero.
    pub word_18_set: usize,
}

impl NodeCheck {
    /// Whether every check the table can make on itself passed.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.links_out_of_range == 0 && self.word_18_set == 0
    }
}

/// Ask a node table whether it reads as one.
#[must_use]
pub fn check(nodes: &[RouteNode]) -> NodeCheck {
    let n = nodes.len();
    let mut c = NodeCheck { nodes: n, ..NodeCheck::default() };
    for node in nodes {
        if node.word_18 != 0 {
            c.word_18_set += 1;
        }
        for link in node.links {
            c.links_read += 1;
            match link {
                None => c.links_absent += 1,
                Some(i) if (i as usize) < n => {}
                Some(_) => c.links_out_of_range += 1,
            }
        }
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(id: u32, payload: &[u8]) -> Vec<u8> {
        let mut v = id.to_le_bytes().to_vec();
        v.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        v.extend_from_slice(payload);
        v
    }

    fn record(x: f32, y: f32, links: [u16; 3], progress: f32) -> Vec<u8> {
        let mut v = Vec::with_capacity(NODE_LEN);
        v.extend_from_slice(&x.to_le_bytes());
        v.extend_from_slice(&y.to_le_bytes());
        v.extend_from_slice(&0u16.to_le_bytes());
        v.extend_from_slice(&0u16.to_le_bytes());
        for l in links {
            v.extend_from_slice(&l.to_le_bytes());
        }
        v.extend_from_slice(&0u16.to_le_bytes());
        v.extend_from_slice(&progress.to_le_bytes());
        assert_eq!(v.len(), NODE_LEN);
        v
    }

    /// The field offsets, against a record laid out by hand. Get one of these wrong and the graph
    /// still reads — it just links the wrong nodes — so the offsets are asserted rather than
    /// exercised.
    #[test]
    fn a_record_reads_at_its_stated_offsets() {
        let n = read_nodes(&record(-1186.3341, -309.1943, [167, 9, NO_LINK], 2657.095)).unwrap();
        assert_eq!(n.len(), 1);
        assert_eq!((n[0].x, n[0].y), (-1186.3341, -309.1943));
        assert_eq!(n[0].links, [Some(167), Some(9), None]);
        assert_eq!(n[0].progress, 2657.095);
        assert_eq!(n[0].linked().collect::<Vec<_>>(), vec![167, 9]);
    }

    /// `0xFFFF` is "no node", not node 65535. Reading it as an index is the failure that turns a
    /// route's last node into a link to somewhere past the end of the table.
    #[test]
    fn the_sentinel_is_absence_and_not_an_index() {
        let n = read_nodes(&record(0.0, 0.0, [NO_LINK; 3], 0.0)).unwrap();
        assert_eq!(n[0].links, [None, None, None]);
        let c = check(&n);
        assert_eq!((c.links_read, c.links_absent, c.links_out_of_range), (3, 3, 0));
        assert!(c.is_clean());
    }

    /// The self-check has to fail on a table that does not check out, or it is not a check.
    #[test]
    fn an_index_past_the_end_of_the_table_is_reported() {
        let mut payload = record(0.0, 0.0, [1, NO_LINK, NO_LINK], 0.0);
        payload.extend(record(1.0, 0.0, [9, NO_LINK, NO_LINK], 1.0));
        let nodes = read_nodes(&payload).unwrap();
        let c = check(&nodes);
        assert_eq!(c.nodes, 2);
        assert_eq!(c.links_out_of_range, 1, "node 9 does not exist in a two-node table");
        assert!(!c.is_clean());
    }

    #[test]
    fn a_payload_that_is_not_whole_records_is_refused() {
        assert!(read_nodes(&[0u8; NODE_LEN + 3]).is_err());
        assert!(read_nodes(&[]).is_ok(), "an empty table is empty, not broken");
    }

    /// A route file is a container, and the nodes are one leaf inside it.
    #[test]
    fn the_node_leaf_is_found_inside_the_container() {
        let mut payload = record(1.0, 2.0, [NO_LINK; 3], 0.0);
        payload.extend(record(3.0, 4.0, [0, NO_LINK, NO_LINK], 2.0));
        let mut body = chunk(0x0003_4149, &[0u8; 16]); // a sibling leaf this reader ignores
        body.extend(chunk(NODE_CHUNK, &payload));
        let file = chunk(0x8003_4147, &body);
        let n = nodes(&file).unwrap();
        assert_eq!(n.len(), 2);
        assert_eq!((n[1].x, n[1].y), (3.0, 4.0));
        assert!(check(&n).is_clean());
    }

    /// Eight of the install's route files carry no node table at all.
    #[test]
    fn a_file_without_a_node_table_reads_as_empty() {
        let file = chunk(0x8003_4147, &chunk(0x0003_414D, &[0u8; 36]));
        assert!(nodes(&file).unwrap().is_empty());
    }
}
