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
//! | `+8` | `u16` | which path of this file's network the node belongs to |
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
//! - **`+20` is in the same unit as the coordinates.** Over the 6,942 pairs that are consecutive
//!   *within one path* (see below), the ratio of its delta to the XY step has median **1.0004**,
//!   p05 0.884, p95 1.027. It is cumulative arc length in world units — the remaining spread is the
//!   height this record does not carry, plus corners cut by a 2-D chord. The p95 was 1.054 while
//!   the pairs were being filtered by "step under 100 m" instead of by path membership: knowing
//!   where a path ends tightened the distribution on its own, which is its own small argument that
//!   the split below is the right one.
//!
//!   This corrects a figure carried in the notes for a while, that the deltas were ≈0.787 of the
//!   XY step. That ratio comes out of pairs taken through the *index* fields rather than through
//!   consecutive records: those are not neighbours in the driving sense, and sampled that way the
//!   median lands at 38.7 rather than near 1. What made the wrong number believable is that 0.785
//!   is π/4, which reads like a derivation rather than a sampling accident.
//!
//! ## The table is a road network, not one path
//!
//! `+8` is the **path index**, and the file's own layout proves it rather than suggests it. In
//! **105 of 105** node tables every value of `+8` occupies exactly one *contiguous run* in file
//! order — 2,923 runs across the install against 2,923 distinct values, with no exception — and in
//! **105 of 105** the values are densely `0..k-1` with no gaps. A field that merely correlated with
//! position would not come out as a clean partition in every table in the game.
//!
//! What that buys is the driving order, which nothing else in the record gives. Taken in file
//! order the node table jumps more than 100 m between neighbours 12.5 % of the time, and sorted by
//! `progress` it is worse at 31.4 % — the table is not one route. Split on `+8` first and the same
//! measurement reads **median 29.2 m, p95 65.9 m, and 0.9 % jumps**: each run is a polyline you
//! could drive.
//!
//! The links then say what they are for. **94.8 %** of the 19,409 present links point at a node in
//! a *different* path — so adjacency along a path is implicit in the order, and `+12`/`+14`/`+16`
//! are the junctions where paths meet. That also explains why they read as a poor undirected
//! neighbour relation: only 29 % are mutual, because a junction is directed.
//!
//! ## What is deliberately not named
//!
//! `+10`'s two bytes each use `0xFF` heavily (3,616 records have `0xFF` in the high byte) which is
//! how the other optional fields here spell "none". It is carried through as a raw word so a caller
//! can measure it further; naming a field is a claim, and this one is not earned yet.
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
    /// `+8`: which path of this file's network the node belongs to. Nodes of one path are
    /// contiguous and in driving order — see the module docs for what pins that.
    pub path: u16,
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
        let path = r.u16_le()?;
        let word_10 = r.u16_le()?;
        let link = |v: u16| (v != NO_LINK).then_some(v);
        let links = [link(r.u16_le()?), link(r.u16_le()?), link(r.u16_le()?)];
        let word_18 = r.u16_le()?;
        let progress = r.f32_le()?;
        out.push(RouteNode { x, y, path, word_10, word_18, links, progress });
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

/// The table's paths: the contiguous runs of one [`RouteNode::path`], in file order.
///
/// This is the whole reason the field is named. A path's nodes are consecutive *and* in driving
/// order, so a run is a polyline — median step 29 m — while the table as a whole is not.
///
/// Runs, not a group-by: if a value ever appeared in two places the two are different runs and this
/// says so, rather than welding them into one path that teleports. [`check`] reports when that
/// happens; it does not happen anywhere in the install.
#[must_use]
pub fn paths(nodes: &[RouteNode]) -> Vec<&[RouteNode]> {
    let mut out = Vec::new();
    let mut start = 0;
    for i in 1..=nodes.len() {
        if i == nodes.len() || nodes[i].path != nodes[start].path {
            out.push(&nodes[start..i]);
            start = i;
        }
    }
    out
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
    /// Distinct [`RouteNode::path`] values.
    pub paths: usize,
    /// Contiguous runs of one path value. **Must equal [`Self::paths`]** — a value appearing in two
    /// runs would mean the field does not partition the table, and every ordering built on it would
    /// be joining two unrelated stretches of road.
    pub path_runs: usize,
    /// Whether the path values are exactly `0..paths`. **Must hold**: the install numbers them
    /// densely from zero in all 105 tables, and a gap would mean a path was dropped on the way in.
    pub paths_dense: bool,
}

impl NodeCheck {
    /// Whether every check the table can make on itself passed.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.links_out_of_range == 0
            && self.word_18_set == 0
            && self.path_runs == self.paths
            && self.paths_dense
    }
}

/// Ask a node table whether it reads as one.
#[must_use]
pub fn check(nodes: &[RouteNode]) -> NodeCheck {
    let n = nodes.len();
    let seen: std::collections::BTreeSet<u16> = nodes.iter().map(|p| p.path).collect();
    let mut c = NodeCheck {
        nodes: n,
        paths: seen.len(),
        path_runs: paths(nodes).len(),
        paths_dense: seen.iter().copied().eq(0..seen.len() as u16),
        ..NodeCheck::default()
    };
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

/// The polygon-region leaf, `0x0003414A` — the largest one a route file carries.
pub const REGION_CHUNK: u32 = 0x0003_414A;

/// Fixed part of a region record: everything before its points.
pub const REGION_HEAD: usize = 68;

/// One polygonal region of a route: a place, a facing, and the ground it covers.
///
/// **What these are is not settled and the names here say only what was measured.** They are large
/// — median area 8,073 m² — they lie on the road (198 of 200 sampled centres stand on drivable
/// surface, and 97 % of their edges have road on both sides), and each carries a direction. That
/// combination is what an "are you on the course, and facing the right way" test needs, and this
/// install ships no barrier chunk, so it is the standing candidate for one. It is not proof.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RouteRegion {
    /// `+0`: a small code, 14 distinct values across the install's 42,698 records. Uninterpreted.
    pub kind: u32,
    /// `+4`: a position in the world's own frame. Inside the polygon in most records but not all,
    /// so it is carried as its own thing rather than called a centre.
    pub origin: [f32; 2],
    /// `+12`: a **unit** direction — `|heading| = 1` in all 42,698, which is one of the four things
    /// that pins this layout.
    pub heading: [f32; 2],
    /// `+32`: the polygon's own bounding box, `[min x, min y, max x, max y]`.
    pub bounds: [f32; 4],
    /// `+48`: a small id, 445 distinct values, zero in 12,830 records. **Not** an object name hash —
    /// none of the 445 matches any of the city's 29,515. Uninterpreted.
    pub id: u32,
    /// `+68`: the polygon's corners, in file order. Mostly four of them (33,772 of 42,698).
    pub points: Vec<[f32; 2]>,
}

impl RouteRegion {
    /// Whether the record's own box agrees with its own points.
    ///
    /// True in all 42,698 records of the install, which is why it is worth asking: a reader that
    /// has drifted by a field cannot keep that true.
    #[must_use]
    pub fn bounds_match(&self) -> bool {
        let Some(first) = self.points.first() else { return true };
        let (mut lo, mut hi) = (*first, *first);
        for p in &self.points {
            lo = [lo[0].min(p[0]), lo[1].min(p[1])];
            hi = [hi[0].max(p[0]), hi[1].max(p[1])];
        }
        let near = |a: f32, b: f32| (a - b).abs() < 0.01;
        near(lo[0], self.bounds[0])
            && near(lo[1], self.bounds[1])
            && near(hi[0], self.bounds[2])
            && near(hi[1], self.bounds[3])
    }

    /// Whether `heading` is the unit vector the format says it is.
    #[must_use]
    pub fn heading_is_unit(&self) -> bool {
        let l = self.heading[0].hypot(self.heading[1]);
        (l - 1.0).abs() < 0.005
    }
}

/// Read one `0x0003414A` payload into regions.
///
/// The record is **self-describing**, which is what makes this a walk rather than a guess: `+64` is
/// the corner count and `+66` is the record's own length in bytes, and `length == 68 + 8 × count`
/// holds in all 42,698 records of the install. Walking by that length lands exactly on the end of
/// the payload in **112 of 112** chunks.
///
/// # Errors
///
/// [`NfsError::CorruptArchive`] if a record's stated length disagrees with its corner count, if it
/// runs past the payload, or if the walk does not consume the payload exactly. All three are the
/// file contradicting itself, and all three are things the install never does — so a reader that
/// tolerated them would be hiding a drift rather than surviving one.
pub fn read_regions(payload: &[u8]) -> NfsResult<Vec<RouteRegion>> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at + REGION_HEAD <= payload.len() {
        let mut r = ByteReader::at(payload, at + 64)?;
        let count = r.u16_le()? as usize;
        let size = r.u16_le()? as usize;
        if size != REGION_HEAD + 8 * count {
            return Err(NfsError::CorruptArchive {
                detail: "0x0003414A record length disagrees with its own corner count",
            });
        }
        if at + size > payload.len() {
            return Err(NfsError::CorruptArchive {
                detail: "0x0003414A record runs past the end of its payload",
            });
        }

        let mut h = ByteReader::at(payload, at)?;
        let kind = h.u32_le()?;
        let origin = [h.f32_le()?, h.f32_le()?];
        let heading = [h.f32_le()?, h.f32_le()?];
        let mut b = ByteReader::at(payload, at + 32)?;
        let bounds = [b.f32_le()?, b.f32_le()?, b.f32_le()?, b.f32_le()?];
        let id = b.u32_le()?;

        let mut p = ByteReader::at(payload, at + REGION_HEAD)?;
        let mut points = Vec::with_capacity(count);
        for _ in 0..count {
            points.push([p.f32_le()?, p.f32_le()?]);
        }

        out.push(RouteRegion { kind, origin, heading, bounds, id, points });
        at += size;
    }
    if at != payload.len() {
        return Err(NfsError::CorruptArchive {
            detail: "0x0003414A walk did not consume the payload exactly",
        });
    }
    Ok(out)
}

/// Every region a route file declares, in file order.
///
/// # Errors
///
/// Propagates a walk failure, or [`read_regions`]'s.
pub fn regions(file: &[u8]) -> NfsResult<Vec<RouteRegion>> {
    let mut out = Vec::new();
    let mut failed = None;
    walk(
        file,
        |_, _| Ok(Visit::Descend),
        |_, header, data| {
            if header.id == REGION_CHUNK && !data.is_empty() {
                match read_regions(data) {
                    Ok(mut r) => out.append(&mut r),
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

/// What a set of regions checks out as. Both fields are zero across the install.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RegionCheck {
    pub regions: usize,
    /// **Must be zero.** A box that does not equal its own points' extent.
    pub bounds_wrong: usize,
    /// **Must be zero.** A heading that is not a unit vector.
    pub heading_not_unit: usize,
}

impl RegionCheck {
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.bounds_wrong == 0 && self.heading_not_unit == 0
    }
}

/// Ask a set of regions whether they read as ones.
#[must_use]
pub fn check_regions(regions: &[RouteRegion]) -> RegionCheck {
    RegionCheck {
        regions: regions.len(),
        bounds_wrong: regions.iter().filter(|r| !r.bounds_match()).count(),
        heading_not_unit: regions.iter().filter(|r| !r.heading_is_unit()).count(),
    }
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
        on_path(0, x, y, links, progress)
    }

    fn on_path(path: u16, x: f32, y: f32, links: [u16; 3], progress: f32) -> Vec<u8> {
        let mut v = Vec::with_capacity(NODE_LEN);
        v.extend_from_slice(&x.to_le_bytes());
        v.extend_from_slice(&y.to_le_bytes());
        v.extend_from_slice(&path.to_le_bytes());
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

    /// The split is what makes a run drivable, so it is asserted on the shape the install has:
    /// consecutive nodes of one path, numbered densely from zero.
    #[test]
    fn a_table_splits_into_contiguous_paths() {
        let mut payload = on_path(0, 0.0, 0.0, [NO_LINK; 3], 0.0);
        payload.extend(on_path(0, 30.0, 0.0, [NO_LINK; 3], 30.0));
        payload.extend(on_path(1, 0.0, 90.0, [NO_LINK; 3], 0.0));
        let nodes = read_nodes(&payload).unwrap();
        let split = paths(&nodes);
        assert_eq!(split.len(), 2);
        assert_eq!(split[0].len(), 2, "path 0 keeps both of its nodes, in order");
        assert_eq!(split[1][0].y, 90.0);
        let c = check(&nodes);
        assert_eq!((c.paths, c.path_runs, c.paths_dense), (2, 2, true));
        assert!(c.is_clean());
    }

    /// A path value in two places is two runs, and welding them would join two unrelated stretches
    /// of road into one polyline that teleports between them. It is reported, not silently merged.
    #[test]
    fn a_path_value_that_comes_back_is_two_runs_and_is_reported() {
        let mut payload = on_path(0, 0.0, 0.0, [NO_LINK; 3], 0.0);
        payload.extend(on_path(1, 0.0, 500.0, [NO_LINK; 3], 0.0));
        payload.extend(on_path(0, 0.0, 1000.0, [NO_LINK; 3], 0.0));
        let nodes = read_nodes(&payload).unwrap();
        assert_eq!(paths(&nodes).len(), 3, "three runs over two values");
        let c = check(&nodes);
        assert_eq!((c.paths, c.path_runs), (2, 3));
        assert!(!c.is_clean(), "a field that does not partition the table is not usable as one");
    }

    /// A gap in the numbering means a path went missing on the way in.
    #[test]
    fn path_numbering_with_a_hole_is_reported() {
        let mut payload = on_path(0, 0.0, 0.0, [NO_LINK; 3], 0.0);
        payload.extend(on_path(2, 0.0, 500.0, [NO_LINK; 3], 0.0));
        let c = check(&read_nodes(&payload).unwrap());
        assert!(!c.paths_dense);
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
    fn region(kind: u32, heading: [f32; 2], pts: &[[f32; 2]]) -> Vec<u8> {
        let (mut lo, mut hi) = (pts[0], pts[0]);
        for p in pts {
            lo = [lo[0].min(p[0]), lo[1].min(p[1])];
            hi = [hi[0].max(p[0]), hi[1].max(p[1])];
        }
        let mut v = Vec::new();
        v.extend_from_slice(&kind.to_le_bytes());
        v.extend_from_slice(&0f32.to_le_bytes());
        v.extend_from_slice(&0f32.to_le_bytes());
        v.extend_from_slice(&heading[0].to_le_bytes());
        v.extend_from_slice(&heading[1].to_le_bytes());
        v.extend_from_slice(&[0u8; 12]);
        for c in [lo[0], lo[1], hi[0], hi[1]] {
            v.extend_from_slice(&c.to_le_bytes());
        }
        v.extend_from_slice(&7u32.to_le_bytes()); // id
        v.extend_from_slice(&[0u8; 12]);
        v.extend_from_slice(&(pts.len() as u16).to_le_bytes());
        v.extend_from_slice(&((REGION_HEAD + 8 * pts.len()) as u16).to_le_bytes());
        for p in pts {
            v.extend_from_slice(&p[0].to_le_bytes());
            v.extend_from_slice(&p[1].to_le_bytes());
        }
        assert_eq!(v.len(), REGION_HEAD + 8 * pts.len());
        v
    }

    const QUAD: [[f32; 2]; 4] = [[0.0, 0.0], [10.0, 0.0], [10.0, 6.0], [0.0, 6.0]];

    /// The record is self-describing: `+64` counts corners and `+66` is the record's own length.
    /// Reading either at the wrong offset is what a walk over 42,698 records cannot survive.
    #[test]
    fn a_region_reads_at_its_stated_offsets() {
        let r = read_regions(&region(13, [0.6, 0.8], &QUAD)).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].kind, 13);
        assert_eq!(r[0].id, 7);
        assert_eq!(r[0].points, QUAD.to_vec());
        assert_eq!(r[0].bounds, [0.0, 0.0, 10.0, 6.0]);
        assert!(r[0].bounds_match() && r[0].heading_is_unit());
    }

    /// Records are variable length and walked by that length, so two of different sizes in one
    /// payload is the case worth asserting.
    #[test]
    fn records_of_different_lengths_walk_one_after_the_other() {
        let mut payload = region(13, [1.0, 0.0], &QUAD);
        payload.extend(region(6, [0.0, -1.0], &[[0.0, 0.0], [4.0, 0.0], [2.0, 9.0]]));
        let r = read_regions(&payload).unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!((r[0].points.len(), r[1].points.len()), (4, 3));
        assert_eq!(r[1].kind, 6);
        assert!(check_regions(&r).is_clean());
    }

    /// The length field and the count field have to agree, and a payload has to be consumed
    /// exactly. Both are true of every chunk in the install, so tolerating either would hide a
    /// drift rather than survive one.
    #[test]
    fn a_record_that_contradicts_itself_is_refused() {
        let mut bad = region(13, [1.0, 0.0], &QUAD);
        bad[66..68].copy_from_slice(&((REGION_HEAD + 8 * 5) as u16).to_le_bytes());
        assert!(read_regions(&bad).is_err(), "length must equal 68 + 8 * corners");

        let mut short = region(13, [1.0, 0.0], &QUAD);
        short.truncate(short.len() - 8);
        assert!(read_regions(&short).is_err(), "a record may not run past its payload");

        let mut trailing = region(13, [1.0, 0.0], &QUAD);
        trailing.extend_from_slice(&[0u8; 4]);
        assert!(read_regions(&trailing).is_err(), "the walk must land on the end");
    }

    /// The two checks have to fail on data that fails them, or they are not checks.
    #[test]
    fn a_box_that_does_not_match_its_points_is_reported() {
        let mut r = read_regions(&region(13, [1.0, 0.0], &QUAD)).unwrap();
        r[0].bounds[2] = 99.0;
        r[0].heading = [1.0, 1.0];
        let c = check_regions(&r);
        assert_eq!((c.bounds_wrong, c.heading_not_unit), (1, 1));
        assert!(!c.is_clean());
    }

    #[test]
    fn a_file_without_a_node_table_reads_as_empty() {
        let file = chunk(0x8003_4147, &chunk(0x0003_414D, &[0u8; 36]));
        assert!(nodes(&file).unwrap().is_empty());
    }
}
