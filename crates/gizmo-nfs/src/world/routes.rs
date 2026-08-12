//! The route files: `TRACKS/ROUTES*/Paths*.bin`, and the lane files beside them,
//! `TRACKS/ROUTES*/Routes####F.bin` / `Routes####B.bin`.
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
//! **A polyline, though, and not an arrow.** File order gives the *sequence* and not the
//! *direction*: across the install **1,361 of the 2,923 paths end lower in `progress` than they
//! start**, so nearly half are stored against the way they are driven. Which end is the start is a
//! question for `+20` and not for the index — walk a path by file order and you have a coin flip
//! per path, which reads as a car driving the right road the wrong way.
//!
//! And the file says so itself, in the leaf that was unread until this was measured the hard way:
//! see [`read_path_info`]. `0x00034149` is **one 220-byte record per path** — exactly
//! `220 × the path count` in all 105 files — and its `+116`/`+120` are the progress at the path's
//! first and last node in file order, exact in 2,923 of 2,923. When those two are the wrong way
//! round the path is reversed, and the count that falls out agrees with the measurement to the
//! record.
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
//! ## `0x00034149`: one record per path
//!
//! 220 bytes each, and the count is the node table's own path count in all 105 files that carry
//! the leaf. Two fields are read and both are exact in 2,923 of 2,923 records: `+116`/`+120`, the
//! progress at the path's first and last node **in file order**, and `+128`/`+132`, the smallest
//! and largest on it. Together they are the file stating a path's direction — see [`PathInfo`].
//!
//! Four more fields are read. `+20` is the path's own id and `+32` its node count, both exact in
//! 2,923 of 2,923 — the first is what makes a record self-identifying, and inferring it from
//! position instead disagrees in 2,811. `+36` names another path, and it is carried as a lead with
//! its own limits attached: see [`PathInfo::next`].
//!
//! The record opens with up to four bytes of ASCII (`R8d`, `F19d`, `M…`, `C…`, `J…`), carried as a
//! tag without a claim about what it names. The remaining ~180 bytes are not read. One reading was
//! tried and refused: the `u16` run near `+174` looks like a list of the events that use the path —
//! in `Paths4001.bin` it reads `4001, 4003, 4104, 4083`, four real track numbers — but only **5 of
//! the 105** files contain their own number anywhere in the record, so whatever that run is, it is
//! not that.
//!
//! ## `0x0003414D`: read, but not named
//!
//! 36-byte records, 118,729 across 111 files, and the leaf is **region-wide** — byte-identical in
//! every file of a region, `PathsFreeRoam.bin` included. The record is pinned by its own
//! redundancy: `+17` is the count of non-zero words at `+20`, exactly so in 118,729 of 118,729,
//! with everything past the count zero and `+16`/`+19` zero throughout. See [`read_segments`].
//!
//! What each record holds is two points a median 24.4 m apart — a segment, not a box, because none
//! of the 118,729 is zero-length — a flag, and one to four 32-bit references.
//!
//! **The references are not named, and three guesses have been killed rather than left open.** They
//! are not the region's solid hashes, not its texture slot keys and not `0x0003414A`'s region ids:
//! 0 of 3,032 resolve in each case. The whole install uses **159 distinct values** in four tight
//! clusters, with runs of consecutive values inside a cluster — the signature of `h * 33 + byte`
//! over names differing in their last character — so they read as name hashes over four families,
//! and naming them wants a corpus of candidate names that is not to hand.
//!
//! ## The lane files: `Routes####F.bin` and `Routes####B.bin`
//!
//! 226 files beside the `Paths*.bin`, in forward/backward pairs, holding three leaves:
//! `0x00034121` in all 226, `0x00034122` in 222 — all but the two test-track regions — and
//! `0x00034123` in the 122 `ROUTESL4RA` files only, where it is always exactly 10,488 bytes and
//! **byte-identical in all 122**. Only `0x00034121` is read; see [`read_lanes`] for the walk and
//! what refuses a wrong one.
//!
//! **What the `F`/`B` pair actually is.** Of a pair's three chunks, `0x00034123` is byte-identical
//! in all 60 pairs that have it, while `0x00034121` and `0x00034122` differ in all 105 — but at
//! the same length in 104 of them. Inside `0x00034121` the two files carry the **same blocks, with
//! the same names, in the same order, with the same point counts**, and the order is emphatically
//! not reversed: matching block `i` of `F` against block `n-1-i` of `B` holds in 9 of 7,399 tries
//! against 5,912 for matching them straight. Only two things change — the header's `+56`/`+60`
//! span, and each point's [`LanePoint::along`]. Their sum is constant within a block in 5,912 of
//! 7,399, so the pair is one geometry with the route measure taken from opposite ends.
//!
//! **What that does not settle.** It was expected to say which of a track's two starting grids a
//! race uses and which end of a grid is the front. It does not: `along` is a coordinate along a
//! *block*, and a block is a stretch of road shared between races rather than one race's line — of
//! 442 full grids, the nearest lane point puts 116 near a block's start, 54 near its end and 212 in
//! the middle. The layout is stated here; the front/back question stays open.

use crate::chunk::{walk, Visit};
use crate::error::{NfsError, NfsResult};
use crate::reader::ByteReader;
use std::collections::BTreeMap;

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
/// **What these are is not settled and the names here say only what was measured.** [`Self::kind`]
/// splits them into two families, and the split is sharp:
///
/// **Placed markers, whose polygon is derived.** Kinds 13, 18 and 19 are **8.00 × 8.00 m** squares
/// and kind 14 is **15.00 × 15.00 m**; across the install every one of them has four corners, right
/// angles at all four, and a centre on its own `origin` — 11,801 / 3,416 / 3,599 / 1,403 records
/// with no exception among the quads — and its `heading` lies along an edge to within 0.0° at the
/// 10th *and* 90th percentile. There is no spread at all, which means the corners are generated
/// from `origin`, `heading` and a size rather than authored, and a consumer can treat these as an
/// oriented square and ignore the points. 19,000 of the 42,698 records are one of these.
///
/// **Areas, whose polygon is the point.** Kinds 3, 4, 11, 12, 16 and 17 have their `origin`
/// *outside* the polygon in 100 % of records and run from 731 m² up to a single 7,961,882 m² sheet.
/// Kinds 5, 6 and 7 sit between: origin inside, hundreds to thousands of m², and the highest rate
/// of lying on the carriageway (85–91 % of their centres stand on drivable road).
///
/// The heading is **not** the road's direction — compared against the local direction of the
/// nearest route node within 60 m, the medians run 48–91° with no kind aligning. For the marker
/// kinds it is the square's own orientation; for the area kinds it is still unexplained.
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
/// The length field has an independent witness. `yugecin/nfsu2-re`, a separate reverse-engineering
/// project, reads this chunk in `nfsu2-re-binfiles/main.c` and walks it the same way —
/// `part_size = *(short*)(pos + 0x42); pos += part_size;` — with the box at `0x20..0x2C` and the
/// first corner at `0x44`, which are these offsets counted from the same place. Two readers
/// arriving at one record boundary from different directions is worth more than either alone.
///
/// It also has the field before it wrong, which is the part worth keeping: it types `0x40` as
/// `short radius`. It is the corner count, and `length == 68 + 8 × count` in all 42,698 records is
/// how that is known rather than argued. That reader also prints only records whose first byte is
/// `0x12` — one [`RouteRegion::kind`] of the fourteen, as it happens the 8 m marker — so the
/// chunk's shape was never in view.
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

/// The event-catalogue leaf, `0x0003414C`.
pub const EVENT_CHUNK: u32 = 0x0003_414C;

/// One event record's length.
pub const EVENT_LEN: usize = 272;

/// Point slots a record reserves. Only the first [`RaceEvent::outline`] of them are live.
pub const EVENT_SLOTS: usize = 33;

/// One race the region offers: what shape the course is and where it runs.
///
/// **This is the course.** The node table is a road *network* — its cross-path links go forward and
/// back about equally and its paths do not tile the distance axis — so nothing in it says where a
/// race goes. The outline here does, and its vertices are in lap order.
///
/// The catalogue is repeated in **every** `Paths*.bin` of its region, which is why one file's
/// 16,320-byte chunk is 60 records and `ROUTESL4RA` holds 60 files: 3,600 copies of 60 events.
/// Across the install that is 4,067 records and **105 distinct events**, ids 4001..4716, spread
/// L4RA 60 · L4RB 9 · L4RC 12 · L4RD 3 · L4RF 8 · L4RG 13. `ROUTESL4RH` and `ROUTESL4RR` carry no
/// events at all.
///
/// 99 of the 105 differ byte-for-byte between the files that carry them and **none differs in its
/// live part** — the tail past `outline` holds stale values from whatever longer record the slot
/// last held, so a reader that compared whole records would call the catalogue inconsistent and a
/// reader that trusts the tail would draw a course through it.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RaceEvent {
    /// `+0`: the event's id, which is also the name of the `Paths####.bin` that carries its network.
    pub id: u16,
    /// `+3`: whether the course is a closed lap rather than a point-to-point sprint.
    pub circuit: bool,
    /// `+4`, uninterpreted.
    pub byte_4: u8,
    /// `+5`: the course's length in hundreds of metres, as the file states it. A hint and no more:
    /// against the outline's own perimeter it runs median 0.90 with a p10 of 0.27 and a p90 of 1.79,
    /// which is what you get comparing a road's length with the outline drawn around it.
    pub length_hint: u32,
    /// `+8`: the course outline, in lap order, in the world's own frame. Closed — last equals
    /// first — exactly when [`Self::circuit`] is set.
    pub outline: Vec<[f32; 2]>,
}

impl RaceEvent {
    /// Whether the outline's last point is its first.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        match (self.outline.first(), self.outline.last()) {
            (Some(a), Some(b)) => (a[0] - b[0]).abs() < 0.01 && (a[1] - b[1]).abs() < 0.01,
            _ => false,
        }
    }
}

/// Read one `0x0003414C` payload into events.
///
/// # Errors
///
/// [`NfsError::CorruptArchive`] if the payload is not a whole number of records, or if a record
/// claims more outline points than it has slots for.
pub fn read_events(payload: &[u8]) -> NfsResult<Vec<RaceEvent>> {
    if !payload.len().is_multiple_of(EVENT_LEN) {
        return Err(NfsError::CorruptArchive {
            detail: "0x0003414C payload is not a whole number of 272-byte event records",
        });
    }
    let mut out = Vec::with_capacity(payload.len() / EVENT_LEN);
    for record in payload.chunks_exact(EVENT_LEN) {
        let mut r = ByteReader::at(record, 0)?;
        let id = r.u16_le()?;
        let count = r.u8()? as usize;
        let circuit = r.u8()? != 0;
        let byte_4 = r.u8()?;
        let length_hint = u32::from(r.u8()?) * 100;
        if count == 0 || count > EVENT_SLOTS {
            return Err(NfsError::CorruptArchive {
                detail: "0x0003414C event claims an outline longer than its 33 slots",
            });
        }
        let mut p = ByteReader::at(record, 8)?;
        let mut outline = Vec::with_capacity(count);
        for _ in 0..count {
            outline.push([p.f32_le()?, p.f32_le()?]);
        }
        out.push(RaceEvent { id, circuit, byte_4, length_hint, outline });
    }
    Ok(out)
}

/// Every event a route file's catalogue declares, in file order.
///
/// # Errors
///
/// Propagates a walk failure, or [`read_events`]'s.
pub fn events(file: &[u8]) -> NfsResult<Vec<RaceEvent>> {
    let mut out = Vec::new();
    let mut failed = None;
    walk(
        file,
        |_, _| Ok(Visit::Descend),
        |_, header, data| {
            if header.id == EVENT_CHUNK && !data.is_empty() {
                match read_events(data) {
                    Ok(mut e) => out.append(&mut e),
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

/// What a catalogue checks out as. The last field is zero across the install.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct EventCheck {
    pub events: usize,
    /// Closed lap circuits, whose outline comes back to its first point.
    pub circuits: usize,
    /// Point-to-point sprints, whose outline does not.
    pub sprints: usize,
    /// **Must be zero.** An event whose circuit flag and whose outline disagree about whether the
    /// course is a loop. Over the install's 105 events it is 63 circuits, 42 sprints and no
    /// exception, which is what makes the flag readable rather than decorative.
    pub flag_disagrees: usize,
}

impl EventCheck {
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.flag_disagrees == 0
    }
}

/// Ask a catalogue whether its circuit flags match its outlines.
#[must_use]
pub fn check_events(events: &[RaceEvent]) -> EventCheck {
    let mut c = EventCheck { events: events.len(), ..EventCheck::default() };
    for e in events {
        match (e.circuit, e.is_closed()) {
            (true, true) => c.circuits += 1,
            (false, false) => c.sprints += 1,
            _ => c.flag_disagrees += 1,
        }
    }
    c
}

/// The start-grid leaf, `0x00034146` — the whole of a `TrackPosMarkers*.bin`.
pub const MARKER_CHUNK: u32 = 0x0003_4146;

/// One marker record's length, after the chunk's leading `0x11` filler.
pub const MARKER_LEN: usize = 48;

/// Cars on a starting grid.
pub const GRID_SLOTS: usize = 8;

/// One place a car is put before a race starts.
///
/// **These are spawn slots, not course anchors** — a correction someone else had to make before us
/// and it is worth carrying: the natural reading of a file full of positions along a route is
/// "checkpoints", and it is wrong. What is here is starting grids: markers sharing a [`group`]
/// come in eights, numbered `0..7` in [`slot`], and the eight sit on a flat patch — median span
/// **10.5 × 11.9 m with 0.00 m between the highest and the lowest** across the install's 219 full
/// grids. Four wide and two deep, on the tarmac.
///
/// A track usually has two of them, at opposite ends of the course, one per race direction — which
/// is the same F/B split the `Routes####F.bin` / `Routes####B.bin` files carry.
///
/// [`group`]: Self::group
/// [`slot`]: Self::slot
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StartMarker {
    /// `+8`: which grid this belongs to. The eight markers of one grid share it.
    pub group: u32,
    /// `+12`: the place on the grid, `0..7`. Pole is `0`.
    ///
    /// Documented elsewhere as a zero field, which it is in 488 of the install's 2,114 markers and
    /// nothing else in the other 1,626.
    pub slot: u32,
    /// `+16`: where the car is put, in the world's own frame — and unlike the rest of these files
    /// this one has a height.
    pub at: [f32; 3],
    /// `+32`, uninterpreted. Its low byte agrees across a grid and its top nibble does not.
    pub word_32: u32,
    /// `+36`: the event this grid belongs to, or `4000` for free roam.
    pub track: u32,
}

/// Read one `0x00034146` payload into markers.
///
/// # Errors
///
/// [`NfsError::CorruptArchive`] if what follows the filler is not a whole number of records, or if
/// a record does not carry the format's own `11, 11` signature in its first two words — which every
/// one of the install's 2,114 does, and which is what makes a misread alignment fail loudly here
/// rather than produce plausible coordinates.
pub fn read_markers(payload: &[u8]) -> NfsResult<Vec<StartMarker>> {
    let body = crate::geometry::skip_leading_filler(payload);
    if !body.len().is_multiple_of(MARKER_LEN) {
        return Err(NfsError::CorruptArchive {
            detail: "0x00034146 payload is not a whole number of 48-byte marker records",
        });
    }
    let mut out = Vec::with_capacity(body.len() / MARKER_LEN);
    for record in body.chunks_exact(MARKER_LEN) {
        let mut r = ByteReader::at(record, 0)?;
        if (r.u32_le()?, r.u32_le()?) != (11, 11) {
            return Err(NfsError::CorruptArchive {
                detail: "0x00034146 record does not begin with the format's 11, 11 signature",
            });
        }
        let group = r.u32_le()?;
        let slot = r.u32_le()?;
        let at = [r.f32_le()?, r.f32_le()?, r.f32_le()?];
        let _pad = r.f32_le()?;
        let word_32 = r.u32_le()?;
        let track = r.u32_le()?;
        out.push(StartMarker { group, slot, at, word_32, track });
    }
    Ok(out)
}

/// Every start marker a `TrackPosMarkers*.bin` declares, in file order.
///
/// # Errors
///
/// Propagates a walk failure, or [`read_markers`]'s.
pub fn markers(file: &[u8]) -> NfsResult<Vec<StartMarker>> {
    let mut out = Vec::new();
    let mut failed = None;
    walk(
        file,
        |_, _| Ok(Visit::Descend),
        |_, header, data| {
            if header.id == MARKER_CHUNK && !data.is_empty() {
                match read_markers(data) {
                    Ok(mut m) => out.append(&mut m),
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

/// The full starting grids among a set of markers: eight markers of one `(group, track)`, in slot
/// order, pole first.
///
/// Groups that are not eight strong are left out rather than padded — the install has 298 lone
/// markers and 24 pairs beside its grids, and a caller asking for a grid wants somewhere to put
/// eight cars.
///
/// **Group per file, not across them.** A region ships `TrackPosMarkersAll.bin` and
/// `TrackPosMarkersFreeRoam.bin`, and one grid is in both: pool the two files' markers first and
/// its `(track, group)` collects sixteen and is dropped as not-a-grid, so the install reads as 219
/// grids instead of 221. Nothing here stops a caller doing that — the fix is to call this once per
/// file, which is what `ug2 track` does.
#[must_use]
pub fn grids(markers: &[StartMarker]) -> Vec<Vec<StartMarker>> {
    let mut by: BTreeMap<(u32, u32), Vec<StartMarker>> = BTreeMap::new();
    for m in markers {
        by.entry((m.track, m.group)).or_default().push(*m);
    }
    by.into_values()
        .filter_map(|mut g| {
            g.sort_by_key(|m| m.slot);
            let numbered = g.iter().map(|m| m.slot).eq(0..GRID_SLOTS as u32);
            (g.len() == GRID_SLOTS && numbered).then_some(g)
        })
        .collect()
}

/// What a set of markers checks out as. Both counts are zero across the install.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct MarkerCheck {
    pub markers: usize,
    /// Full eight-car grids.
    pub grids: usize,
    /// **Must be zero.** A slot outside `0..8`.
    pub slot_out_of_range: usize,
    /// **Must be zero.** A group of eight whose slots are not exactly `0..7`.
    pub grid_misnumbered: usize,
}

impl MarkerCheck {
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.slot_out_of_range == 0 && self.grid_misnumbered == 0
    }
}

/// Ask a set of markers whether it reads as grids.
#[must_use]
pub fn check_markers(markers: &[StartMarker]) -> MarkerCheck {
    let mut by: BTreeMap<(u32, u32), Vec<u32>> = BTreeMap::new();
    for m in markers {
        by.entry((m.track, m.group)).or_default().push(m.slot);
    }
    let mut c = MarkerCheck {
        markers: markers.len(),
        slot_out_of_range: markers.iter().filter(|m| m.slot as usize >= GRID_SLOTS).count(),
        ..MarkerCheck::default()
    };
    for mut slots in by.into_values() {
        if slots.len() != GRID_SLOTS {
            continue;
        }
        slots.sort_unstable();
        if slots.iter().copied().eq(0..GRID_SLOTS as u32) {
            c.grids += 1;
        } else {
            c.grid_misnumbered += 1;
        }
    }
    c
}

// ── 0x00034149: one record per path ───────────────────────────────────────────────────────────

/// The per-path leaf of a `Paths*.bin`.
pub const PATH_CHUNK: u32 = 0x0003_4149;

/// One path record's length.
///
/// Not a divisor that happens to fit: the payload is **exactly** `PATH_LEN × the node table's path
/// count` in all 105 files that carry one, and the path count is itself proven — every value of
/// `+8` occupies one contiguous run, in 105 of 105 tables.
pub const PATH_LEN: usize = 220;

/// What the route file says about one of its paths.
///
/// Most of the record is not read. What is read is read at 100 %.
#[derive(Debug, Clone, PartialEq)]
pub struct PathInfo {
    /// `+0`, up to four bytes of ASCII: `R8d`, `F19d`, `M…`, `C…`, `J…`. A tag rather than a
    /// sentence, and it is carried without a claim about what it names.
    pub name: String,
    /// `+20`: the path's own id — the same value its nodes carry at their `+8`. Exact in 2,923 of
    /// 2,923 records, which is what makes a record self-identifying: a caller does not have to
    /// infer that the *n*th record describes path *n*, and inferring it from position in the node
    /// table instead disagrees in 2,811 of them.
    pub id: u16,
    /// `+32`: how many nodes the path has. Exact in 2,923 of 2,923.
    pub nodes: u16,
    /// `+36`: another path this one runs into.
    ///
    /// **A lead, not a decoded field, and the numbers say exactly how far it goes.** It names a
    /// path the node-level links also reach in **96.6 %** of records, so it is a path-level
    /// adjacency rather than a coincidence. Followed as a successor — chain from path to path
    /// until it repeats — it lays out a line covering at least 95 % of the route's own stated
    /// length in **43 of 105** routes and at least half in 79, with a median of 80 %; every route
    /// made of a single path comes out at 100 %.
    ///
    /// So it is *a* successor and not *the* sequence. Something else decides where the chain
    /// starts and what happens where it breaks, and that something is not read.
    pub next: u16,
    /// `+116`/`+120`: [`RouteNode::progress`] at the path's first and last node **in file order**.
    ///
    /// Exact in **2,923 of 2,923** records. Together with [`Self::low`]/[`Self::high`] this is the
    /// file stating a path's direction, which otherwise has to be measured: `from > to` says the
    /// path is stored against the way it is driven, and 1,361 of the install's paths are.
    pub from: f32,
    pub to: f32,
    /// `+128`/`+132`: the smallest and largest progress on the path. Exact in 2,923 of 2,923.
    pub low: f32,
    pub high: f32,
}

impl PathInfo {
    /// Whether the path's nodes are stored in the order they are driven.
    ///
    /// The one thing this record settles that nothing else does. A caller that walks a path by
    /// index without asking gets a coin flip, and the way it fails — a car driving the right road
    /// the wrong way — reads as a physics or AI bug long before it reads as a decode.
    #[must_use]
    pub fn driven_in_file_order(&self) -> bool {
        self.to >= self.from
    }
}

/// Read one `0x00034149` payload into per-path records.
///
/// # Errors
///
/// [`NfsError::CorruptArchive`] if the payload is not a whole number of records.
pub fn read_path_info(payload: &[u8]) -> NfsResult<Vec<PathInfo>> {
    if !payload.len().is_multiple_of(PATH_LEN) {
        return Err(NfsError::CorruptArchive {
            detail: "0x00034149 payload is not a whole number of 220-byte path records",
        });
    }
    let mut out = Vec::with_capacity(payload.len() / PATH_LEN);
    for record in payload.chunks_exact(PATH_LEN) {
        let end = record[..4].iter().position(|b| *b == 0).unwrap_or(4);
        let mut r = ByteReader::at(record, 20)?;
        let id = r.u16_le()?;
        let mut r = ByteReader::at(record, 32)?;
        let nodes = r.u16_le()?;
        let mut r = ByteReader::at(record, 36)?;
        let next = r.u16_le()?;
        let mut r = ByteReader::at(record, 116)?;
        let (from, to) = (r.f32_le()?, r.f32_le()?);
        let mut r = ByteReader::at(record, 128)?;
        let (low, high) = (r.f32_le()?, r.f32_le()?);
        out.push(PathInfo {
            name: String::from_utf8_lossy(&record[..end]).into_owned(),
            id,
            nodes,
            next,
            from,
            to,
            low,
            high,
        });
    }
    Ok(out)
}

/// Every path record a route file declares. Empty rather than an error when it carries none.
///
/// # Errors
///
/// Propagates a walk failure, or [`NfsError::CorruptArchive`] from [`read_path_info`].
pub fn path_info(file: &[u8]) -> NfsResult<Vec<PathInfo>> {
    let mut out = Vec::new();
    let mut failed = None;
    walk(
        file,
        |_, _| Ok(Visit::Descend),
        |_, header, data| {
            if header.id == PATH_CHUNK && !data.is_empty() {
                match read_path_info(data) {
                    Ok(mut p) => out.append(&mut p),
                    Err(e) => failed = Some(e),
                }
            }
            Ok(())
        },
    )?;
    failed.map_or(Ok(out), Err)
}

/// Hold the path records against the node table they describe.
///
/// Every count here is zero across the install, which is what makes the two fields a reading rather
/// than a guess: a record whose stated span disagrees with the nodes it covers would be an offset
/// that lands on something else.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PathInfoCheck {
    pub records: usize,
    /// Files where the record count is not the node table's path count.
    pub count_wrong: usize,
    /// Records whose `from`/`to` are not the first and last node's progress.
    pub ends_wrong: usize,
    /// Records whose `low`/`high` are not the smallest and largest.
    pub span_wrong: usize,
    /// Records the file marks as stored against the way they are driven.
    pub reversed: usize,
    /// Records whose `+20` is not their own index.
    pub id_wrong: usize,
    /// Records whose `+32` is not the path's node count.
    pub node_count_wrong: usize,
}

/// Check a file's path records against its own nodes.
#[must_use]
pub fn check_path_info(info: &[PathInfo], nodes: &[RouteNode]) -> PathInfoCheck {
    // Keyed by the path's **id**, not by where its run sits in the file. The two are not the same
    // order — `paths` returns runs as the file lays them out, and a record's index is the id it
    // describes. Pairing them by position instead disagrees with the nodes in 2,811 of 2,923
    // records, which is what that mistake looks like from the outside: a field that seems wrong.
    let runs = paths(nodes);
    let by_id: BTreeMap<u16, &[RouteNode]> =
        runs.iter().filter(|r| !r.is_empty()).map(|r| (r[0].path, *r)).collect();
    let mut c = PathInfoCheck {
        records: info.len(),
        count_wrong: usize::from(info.len() != runs.len()),
        ..PathInfoCheck::default()
    };
    for (i, p) in info.iter().enumerate() {
        let Ok(id) = u16::try_from(i) else { continue };
        let Some(run) = by_id.get(&id).copied() else { continue };
        let (first, last) = (run[0].progress, run[run.len() - 1].progress);
        let (lo, hi) = run.iter().fold((f32::MAX, f32::MIN), |(a, b), n| {
            (a.min(n.progress), b.max(n.progress))
        });
        if (p.from - first).abs() > 0.5 || (p.to - last).abs() > 0.5 {
            c.ends_wrong += 1;
        }
        if (p.low - lo).abs() > 0.5 || (p.high - hi).abs() > 0.5 {
            c.span_wrong += 1;
        }
        if !p.driven_in_file_order() {
            c.reversed += 1;
        }
        if usize::from(p.id) != i {
            c.id_wrong += 1;
        }
        if usize::from(p.nodes) != run.len() {
            c.node_count_wrong += 1;
        }
    }
    c
}

// ── 0x0003414D: the marked segments ───────────────────────────────────────────────────────────

/// The segment leaf of a `Paths*.bin`. Region-wide: byte-identical in every file of a region,
/// including its `PathsFreeRoam.bin`.
pub const SEGMENT_CHUNK: u32 = 0x0003_414D;

/// One segment record's length. Every payload in the install is a whole multiple of it.
pub const SEGMENT_LEN: usize = 36;

/// A short run of ground with up to four references hung off it.
///
/// **What it is not** is as far as this has been taken, and the negatives are the useful part: the
/// four trailing words are not the region's solid hashes (0 of 3,032 resolve), not its texture slot
/// keys (0), and not `0x0003414A`'s region ids (0). Naming them is left open rather than guessed.
#[derive(Debug, Clone, PartialEq)]
pub struct RouteSegment {
    /// `+0`…`+15`: two points in the world's own frame, no height.
    ///
    /// A segment and not a box: across the install the two points are a median **24.4 m** apart
    /// (p05 7.9, p95 75.1, longest 792.6) and **none** of the 118,729 is zero-length, which a box
    /// stored as `(min, max)` would be wherever something was flat.
    pub at: [[f32; 2]; 2],
    /// `+18`: set in 115,984 of 118,729 records and clear in 2,745. Uninterpreted.
    pub flag: u8,
    /// `+20`…`+35`: one to four 32-bit references, the count declared at `+17`.
    ///
    /// **The whole install uses 159 distinct values**, in four tight clusters — 93 in
    /// `0x01F224BE..0x01F242AA`, 60 in `0x4EA23A69..0x4EA23F98`, 5 in `0xD59BF29A..0xD59BF29E`, and
    /// one alone at `0x01FB47FD`. Values inside a cluster run consecutively in places, which is
    /// what [`crate::hash`]'s `h * 33 + byte` gives two names differing in their last character; so
    /// they read as name hashes over four families. Which names is not answered. Only the
    /// `0x01F2` family appears outside `ROUTESL4RA`; the other three are the city's alone.
    pub refs: Vec<u32>,
}

/// Read one `0x0003414D` payload into segments.
///
/// The record is pinned by its own redundancy rather than by a stride that happens to divide:
/// `+17` is the number of non-zero words at `+20`, and it is exactly that in **118,729 of
/// 118,729** records, with every word past the count zero in all of them, and `+16` and `+19` zero
/// in all of them. Read at 24 bytes instead that count holds for 47.3 % of records and at 48 bytes
/// for 33.5 %; 32 and 40 do not divide a single payload.
///
/// # Errors
///
/// [`NfsError::CorruptArchive`] if the payload is not a whole number of records, if `+17` is above
/// four or disagrees with the words that follow it, or if `+16`/`+19` are not zero.
pub fn read_segments(payload: &[u8]) -> NfsResult<Vec<RouteSegment>> {
    if !payload.len().is_multiple_of(SEGMENT_LEN) {
        return Err(NfsError::CorruptArchive {
            detail: "0x0003414D payload is not a whole number of 36-byte segment records",
        });
    }
    let mut out = Vec::with_capacity(payload.len() / SEGMENT_LEN);
    for record in payload.chunks_exact(SEGMENT_LEN) {
        let mut r = ByteReader::at(record, 0)?;
        let at = [[r.f32_le()?, r.f32_le()?], [r.f32_le()?, r.f32_le()?]];
        if record[16] != 0 || record[19] != 0 {
            return Err(NfsError::CorruptArchive {
                detail: "0x0003414D record has a non-zero byte where the format keeps zero",
            });
        }
        let count = usize::from(record[17]);
        if count > 4 {
            return Err(NfsError::CorruptArchive {
                detail: "0x0003414D record declares more than four references",
            });
        }
        let mut refs = Vec::with_capacity(count);
        for i in 0..4 {
            let mut w = ByteReader::at(record, 20 + i * 4)?;
            let word = w.u32_le()?;
            if i < count {
                if word == 0 {
                    return Err(NfsError::CorruptArchive {
                        detail: "0x0003414D record declares a reference it did not write",
                    });
                }
                refs.push(word);
            } else if word != 0 {
                return Err(NfsError::CorruptArchive {
                    detail: "0x0003414D record wrote a reference past its own count",
                });
            }
        }
        out.push(RouteSegment { at, flag: record[18], refs });
    }
    Ok(out)
}

/// Every segment a route file declares. Empty rather than an error when the file carries none.
///
/// # Errors
///
/// Propagates a walk failure, or [`NfsError::CorruptArchive`] from [`read_segments`].
pub fn segments(file: &[u8]) -> NfsResult<Vec<RouteSegment>> {
    let mut out = Vec::new();
    let mut failed = None;
    walk(
        file,
        |_, _| Ok(Visit::Descend),
        |_, header, data| {
            if header.id == SEGMENT_CHUNK && !data.is_empty() {
                match read_segments(data) {
                    Ok(mut s) => out.append(&mut s),
                    Err(e) => failed = Some(e),
                }
            }
            Ok(())
        },
    )?;
    failed.map_or(Ok(out), Err)
}

// ── Routes####F/B.bin: the lane blocks ────────────────────────────────────────────────────────

/// The chunk holding the lane blocks, in `Routes####F.bin` and `Routes####B.bin`.
///
/// The two other chunks in those files are not read yet. `0x0003_4122` is in 222 of the 226 files
/// — every one except the two test-track regions — and autocorrelates at a stride of 32;
/// `0x0003_4123` is in the 122 `ROUTESL4RA` files only, is always exactly 10,488 bytes, is
/// **byte-identical in all 122**, and autocorrelates at 16.
pub const LANE_CHUNK: u32 = 0x0003_4121;

/// Bytes of a lane block's header, before its records.
const LANE_HEADER: usize = 84;
/// Bytes of one record inside a lane block.
const LANE_RECORD: usize = 56;

/// One point on a lane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LanePoint {
    /// `+0`/`+4`: where it is, in the world's own frame. No height — these files are flat.
    pub at: [f32; 2],
    /// `+8`: how far along the route this point is.
    ///
    /// The unit is world units, the same as the step between points: across 58,714 consecutive
    /// pairs the ratio of this field's change to the plain XY distance has p05 0.018, median 0.75
    /// and p95 1.37 — under one because a lane wanders across the road while it advances along it.
    ///
    /// **The direction is not fixed.** A block may count up or down: it is one-way along the block
    /// in 3,927 of 6,686 sampled blocks, and where it counts down the first point holds the
    /// largest value. Read it as a coordinate, not as a distance travelled, and take direction from
    /// the sign of the change rather than from the record order.
    pub along: f32,
}

/// One lane of a route: a named run of points with the extent it covers.
#[derive(Debug, Clone, PartialEq)]
pub struct Lane {
    /// `+16`, 16 bytes: `TrackRoutesA10` … `TrackRoutesA64`.
    ///
    /// Thirty of them exist, `A{group}{variant}` with group `1..6` and variant `0..4`, and every
    /// one of the thirty is used. The letter is always `A` — it is not the region, which was the
    /// first guess: `ROUTESL4RG`'s files carry `TrackRoutesA50` just as `ROUTESL4RB`'s do. A group
    /// belongs to a set of regions and not to one (`A30` appears in six of the eight, `A6x` only in
    /// `ROUTESL4RB`), so it is a kind of lane rather than a place.
    pub name: String,
    /// `+10`: the block's number within its file. Runs up, with gaps.
    pub id: u16,
    /// `+56`/`+60`: the smallest and largest [`LanePoint::along`] in the block.
    ///
    /// Holds in 16,358 of 17,222 blocks with a median error of zero. **These are the only two
    /// fields of the header that differ between the `F` file and the `B` one.**
    pub span: [f32; 2],
    /// The first record's `+12`…`+27`: `[min_x, min_y, max_x, max_y]`.
    ///
    /// Every point of every block is inside it — 17,222 of 17,222 — with a median margin of
    /// exactly 10 m. The first record is not a point and is not returned as one; this is what it
    /// carries.
    pub bounds: [f32; 4],
    /// The lane itself, in file order.
    pub points: Vec<LanePoint>,
}

/// Read the lane blocks of a `0x0003_4121` payload.
///
/// **The walk is self-describing rather than a guess.** A block is `84` bytes of header and
/// `count + 1` records of `56`, where `count` is the `u16` at `+52` — repeated at `+54`, and the
/// two agree in all 17,222 blocks of the install. Walking by it consumes the payload **exactly**
/// in all 222 files that have one, and the four that do not have one carry an empty chunk.
///
/// That is a stronger statement than it sounds, because every neighbouring reading fails it in
/// every file: a header of 80 or 88 bytes, a record of 52 or 60, or taking the count from `+50` or
/// `+56` each land off the end in **222 of 222**. This function refuses all of them.
///
/// # Errors
///
/// [`NfsError::CorruptArchive`] if the two counts disagree, if a block's name is not one of the
/// format's, if a block runs past the payload, or if the walk does not end exactly on its end.
pub fn read_lanes(payload: &[u8]) -> NfsResult<Vec<Lane>> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at < payload.len() {
        let head = payload.get(at..at + LANE_HEADER).ok_or(NfsError::CorruptArchive {
            detail: "0x00034121 block header runs past the payload",
        })?;
        let mut r = ByteReader::at(head, 10)?;
        let id = r.u16_le()?;
        // Cut at the first NUL, not at the trailing ones: the field is a C string in a fixed
        // 16-byte slot and what follows the terminator is whatever was there before. Trimming from
        // the end instead lets that tail into the name, and turns thirty names into thirty-four.
        let field = &head[16..32];
        let end = field.iter().position(|b| *b == 0).unwrap_or(field.len());
        let name = String::from_utf8_lossy(&field[..end]).trim_end().to_owned();
        if !name.starts_with("TrackRoutes") {
            return Err(NfsError::CorruptArchive {
                detail: "0x00034121 block is not named TrackRoutes*",
            });
        }
        let mut r = ByteReader::at(head, 52)?;
        let (count, again) = (r.u16_le()?, r.u16_le()?);
        if count != again {
            return Err(NfsError::CorruptArchive {
                detail: "0x00034121 block states its point count twice and the two disagree",
            });
        }
        let mut r = ByteReader::at(head, 56)?;
        let span = [r.f32_le()?, r.f32_le()?];

        let records = usize::from(count) + 1;
        let body_at = at + LANE_HEADER;
        let body = payload
            .get(body_at..body_at + records * LANE_RECORD)
            .ok_or(NfsError::CorruptArchive {
                detail: "0x00034121 block claims more records than the payload holds",
            })?;
        // The first record is the block's own box, not a point on it.
        let mut r = ByteReader::at(&body[..LANE_RECORD], 12)?;
        let bounds = [r.f32_le()?, r.f32_le()?, r.f32_le()?, r.f32_le()?];
        let mut points = Vec::with_capacity(usize::from(count));
        for record in body[LANE_RECORD..].chunks_exact(LANE_RECORD) {
            let mut r = ByteReader::at(record, 0)?;
            points.push(LanePoint { at: [r.f32_le()?, r.f32_le()?], along: r.f32_le()? });
        }
        out.push(Lane { name, id, span, bounds, points });
        at = body_at + records * LANE_RECORD;
    }
    if at != payload.len() {
        return Err(NfsError::CorruptArchive {
            detail: "0x00034121 block walk did not land on the end of the payload",
        });
    }
    Ok(out)
}

/// Every lane block in a `Routes####F.bin` or `Routes####B.bin`.
///
/// # Errors
///
/// Propagates a walk failure, or [`NfsError::CorruptArchive`] from [`read_lanes`].
pub fn lanes(file: &[u8]) -> NfsResult<Vec<Lane>> {
    let mut out = Vec::new();
    let mut failed = None;
    walk(
        file,
        |_, _| Ok(Visit::Descend),
        |_, header, data| {
            if header.id == LANE_CHUNK && !data.is_empty() {
                match read_lanes(data) {
                    Ok(mut l) => out.append(&mut l),
                    Err(e) => failed = Some(e),
                }
            }
            Ok(())
        },
    )?;
    failed.map_or(Ok(out), Err)
}

/// What a set of lanes checks out as. Both counts are zero across the install.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LaneCheck {
    /// How many blocks were read.
    pub lanes: usize,
    /// How many points they hold, the first record of each block excluded.
    pub points: usize,
    /// Blocks with a point outside their own stated box. **Zero across the install.**
    pub outside_bounds: usize,
    /// Blocks whose `span` is not the extent of their points' `along`. 864 of 17,222 — a check
    /// that fails, kept as a number rather than dropped, because whatever those blocks are is not
    /// understood yet.
    pub span_wrong: usize,
}

/// Hold a set of lanes against themselves.
#[must_use]
pub fn check_lanes(lanes: &[Lane]) -> LaneCheck {
    let mut c = LaneCheck { lanes: lanes.len(), ..LaneCheck::default() };
    for l in lanes {
        c.points += l.points.len();
        if l.points.iter().any(|p| {
            p.at[0] < l.bounds[0]
                || p.at[0] > l.bounds[2]
                || p.at[1] < l.bounds[1]
                || p.at[1] > l.bounds[3]
        }) {
            c.outside_bounds += 1;
        }
        let (lo, hi) = l.points.iter().fold((f32::MAX, f32::MIN), |(a, b), p| {
            (a.min(p.along), b.max(p.along))
        });
        if l.points.is_empty() || (lo - l.span[0]).abs() > 0.01 || (hi - l.span[1]).abs() > 0.01 {
            c.span_wrong += 1;
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

    fn event(id: u16, circuit: bool, pts: &[[f32; 2]]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&id.to_le_bytes());
        v.push(pts.len() as u8);
        v.push(u8::from(circuit));
        v.push(0);
        v.push(50); // 5,000 m
        v.extend_from_slice(&0u16.to_le_bytes());
        for p in pts {
            v.extend_from_slice(&p[0].to_le_bytes());
            v.extend_from_slice(&p[1].to_le_bytes());
        }
        // The slots past the outline hold whatever the record last held. Filling them with
        // something that would be a plausible point is the whole point of the test below.
        while v.len() < EVENT_LEN {
            v.extend_from_slice(&999.0f32.to_le_bytes());
        }
        v.truncate(EVENT_LEN);
        v
    }

    const LAP: [[f32; 2]; 4] = [[0.0, 0.0], [100.0, 0.0], [100.0, 80.0], [0.0, 0.0]];
    const SPRINT: [[f32; 2]; 3] = [[0.0, 0.0], [100.0, 0.0], [200.0, 40.0]];

    /// The tail past the outline is stale, and it looks like data. 99 of the install's 105 events
    /// differ from file to file in nothing else, so a reader that took all 33 slots would draw a
    /// course through whatever the slot last held.
    #[test]
    fn only_the_live_part_of_an_outline_is_read() {
        let e = read_events(&event(4001, true, &LAP)).unwrap();
        assert_eq!(e[0].outline, LAP.to_vec(), "four points, not thirty-three");
        assert_eq!(e[0].id, 4001);
        assert_eq!(e[0].length_hint, 5000);
    }

    /// The circuit flag and the outline agree in all 105 events — 63 loops, 42 sprints, no
    /// exception — which is what makes the flag worth reading instead of measuring the outline.
    #[test]
    fn the_circuit_flag_and_the_outline_agree_or_it_is_reported() {
        let mut payload = event(4001, true, &LAP);
        payload.extend(event(4002, false, &SPRINT));
        let e = read_events(&payload).unwrap();
        let c = check_events(&e);
        assert_eq!((c.events, c.circuits, c.sprints, c.flag_disagrees), (2, 1, 1, 0));
        assert!(c.is_clean());

        // A loop whose flag says sprint is the file contradicting itself, and it is reported.
        let lying = read_events(&event(4003, false, &LAP)).unwrap();
        assert_eq!(check_events(&lying).flag_disagrees, 1);
        assert!(!check_events(&lying).is_clean());
    }

    #[test]
    fn an_outline_longer_than_its_slots_is_refused() {
        let mut bad = event(4001, true, &LAP);
        bad[2] = 34;
        assert!(read_events(&bad).is_err());
        bad[2] = 0;
        assert!(read_events(&bad).is_err(), "an event with no outline is not an event");
        assert!(read_events(&[0u8; EVENT_LEN + 1]).is_err());
    }

    fn marker(group: u32, slot: u32, at: [f32; 3], track: u32) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&11u32.to_le_bytes());
        v.extend_from_slice(&11u32.to_le_bytes());
        v.extend_from_slice(&group.to_le_bytes());
        v.extend_from_slice(&slot.to_le_bytes());
        for c in at {
            v.extend_from_slice(&c.to_le_bytes());
        }
        v.extend_from_slice(&0f32.to_le_bytes());
        v.extend_from_slice(&0u32.to_le_bytes());
        v.extend_from_slice(&track.to_le_bytes());
        v.extend_from_slice(&[0u8; 8]);
        assert_eq!(v.len(), MARKER_LEN);
        v
    }

    fn full_grid(group: u32, track: u32) -> Vec<u8> {
        (0..GRID_SLOTS as u32)
            .flat_map(|s| marker(group, s, [s as f32 * 3.0, 0.0, 12.0], track))
            .collect()
    }

    /// The `11, 11` signature is the alignment check. Without it a marker file read at the wrong
    /// offset gives coordinates that look like coordinates.
    #[test]
    fn a_marker_reads_at_its_stated_offsets_and_the_signature_is_enforced() {
        let mut payload = vec![0x11u8; 8];
        payload.extend(marker(0xDEAD, 3, [584.1, -128.4, 15.8], 4101));
        let m = read_markers(&payload).unwrap();
        assert_eq!(m.len(), 1);
        assert_eq!((m[0].group, m[0].slot, m[0].track), (0xDEAD, 3, 4101));
        assert_eq!(m[0].at, [584.1, -128.4, 15.8]);

        let mut broken = payload.clone();
        broken[8] = 12;
        assert!(read_markers(&broken).is_err(), "11, 11 or it is not a marker record");
    }

    /// A grid is eight markers of one group numbered 0..7, pole first — 219 of them across the
    /// install and not one misnumbered.
    #[test]
    fn a_full_grid_comes_back_in_slot_order_and_partial_ones_do_not_come_back() {
        let mut payload = vec![0x11u8; 8];
        payload.extend(full_grid(0xAAAA, 4101));
        payload.extend(marker(0xBBBB, 0, [0.0, 0.0, 0.0], 4101)); // a lone marker
        let m = read_markers(&payload).unwrap();
        let g = grids(&m);
        assert_eq!(g.len(), 1, "the lone marker is not a grid");
        assert_eq!(g[0].len(), GRID_SLOTS);
        assert!(g[0].iter().map(|m| m.slot).eq(0..GRID_SLOTS as u32), "pole first");

        let c = check_markers(&m);
        assert_eq!((c.markers, c.grids), (9, 1));
        assert!(c.is_clean());
    }

    /// Eight markers whose slots are not 0..7 is the file disagreeing with itself, and it is
    /// reported rather than sorted into something plausible.
    #[test]
    fn eight_markers_numbered_wrong_are_reported() {
        let mut payload = vec![0x11u8; 8];
        payload.extend(full_grid(0xAAAA, 4101));
        // Break one slot number: 0..6 and a 7 turned into a second 6.
        let last = payload.len() - MARKER_LEN;
        payload[last + 12..last + 16].copy_from_slice(&6u32.to_le_bytes());
        let m = read_markers(&payload).unwrap();
        let c = check_markers(&m);
        assert_eq!((c.grids, c.grid_misnumbered), (0, 1));
        assert!(!c.is_clean());
        assert!(grids(&m).is_empty(), "a misnumbered eight is not a grid");
    }

    #[test]
    fn a_file_without_a_node_table_reads_as_empty() {
        let file = chunk(0x8003_4147, &chunk(0x0003_414D, &[0u8; 36]));
        assert!(nodes(&file).unwrap().is_empty());
    }
}
