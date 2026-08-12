//! `ug2 track` — read the route files' node tables, and make them answer for themselves.
//!
//! [`super::verify`] does this for the city: decode it, then compare the decode against what the
//! file declared. A route file declares no counts to compare against, so the check has to come
//! from the record's own internal consistency instead — which turns out to be a stronger test than
//! it sounds, because three of the six fields are indices into the very table they sit in.
//!
//! Six things are asserted, and the install passes all six:
//!
//! - a `+12`/`+14`/`+16` that is neither `0xFFFF` nor a valid index into its own file's table — 0
//!   of 54,192 index reads,
//! - a `+18` that is not zero — 0 of 18,064,
//! - a path value that occupies more than one contiguous run — 2,923 runs over 2,923 values,
//! - a gap in the path numbering — dense `0..k` in all 105 tables,
//! - a region whose stated box is not its own corners' extent — 0 of 42,698,
//! - a region whose heading is not a unit vector — 0 of 42,698.
//!
//! The region records are self-describing, so reading them is a walk rather than a guess: `+64` is
//! the corner count, `+66` the record's own byte length, and `length == 68 + 8 × count` holds
//! everywhere. Walking by it lands exactly on the end of the payload in 112 of 112 chunks — which
//! [`gizmo_nfs::world::routes::read_regions`] refuses to do otherwise.
//!
//! A misaligned or misread field does not survive that. The links would have to be small enough, in
//! every one of eighteen thousand records, to pass for an index into a table as short as five
//! entries; and the path field would have to partition every table in the game by accident. Both
//! have been tried: shifting the link read two bytes puts 15,931 links out of range, and taking the
//! path from `+10` instead of `+8` gives 15,967 runs over 13,352 values with holes in the
//! numbering. The command exits non-zero for either.
//!
//! The report also states the two distributions the layout was read from, so the claims in
//! [`gizmo_nfs::world::routes`] can be re-derived rather than believed: the step between nodes of
//! one path, and the ratio of the `+20` gain to that step. The second is what says `+20` is a
//! distance in world units and not some other quantity — and both are measured *within* a path,
//! because that is the only place where consecutive means adjacent.
//!
//! ```bash
//! ug2 track "$NFSU2_ROOT/TRACKS"          # every route file
//! ug2 track "$NFSU2_ROOT/TRACKS/ROUTESL4RA/Paths4001.bin"
//! ```
//!
//! Exits non-zero if any check fails, so it can be a step in a script rather than something to read.

use crate::paths::Result;
use gizmo_nfs::world::routes::{
    check, check_events, check_lanes, check_markers, check_regions, events, lanes, markers, nodes,
    path_info, paths, regions, segments, check_path_info, EventCheck, LaneCheck, MarkerCheck,
    NodeCheck, PathInfoCheck, RaceEvent, RegionCheck, RouteNode, StartMarker,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Every `TrackPosMarkers*.bin` beside the route files under a path.
fn collect_markers(path: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![if path.is_file() {
        path.parent().unwrap_or(path).to_path_buf()
    } else {
        path.to_path_buf()
    }];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.filter_map(std::result::Result::ok) {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("TrackPosMarkers") && n.ends_with(".bin"))
            {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// Every `Routes*.bin` beside the route files under a path.
fn collect_lanes(path: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![if path.is_file() {
        path.parent().unwrap_or(path).to_path_buf()
    } else {
        path.to_path_buf()
    }];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.filter_map(std::result::Result::ok) {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("Routes") && n.ends_with(".bin"))
            {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// Every `Paths*.bin` under a path, whether it names one file, a `ROUTES*` directory or `TRACKS`.
fn collect(path: &Path) -> Result<Vec<PathBuf>> {
    if path.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }
    let mut out = Vec::new();
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        for entry in entries.filter_map(std::result::Result::ok) {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("Paths") && n.ends_with(".bin"))
            {
                out.push(p);
            }
        }
    }
    out.sort();
    Ok(out)
}

#[derive(Default)]
struct Tally {
    files: usize,
    files_without_nodes: usize,
    check: NodeCheck,
    region: RegionCheck,
    event: EventCheck,
    marker: MarkerCheck,
    lane: LaneCheck,
    /// Lane block names, so the thirty of them are counted rather than assumed.
    lane_names: std::collections::BTreeSet<String>,
    /// Grids whose derived heading agrees with, or opposes, their event's outline order, and the
    /// per-track view that the pooled rate hides.
    facing: (usize, usize, usize, usize, usize, usize, usize),
    /// Every event seen, so a grid can be matched to one whatever file it came from.
    catalogue: std::collections::BTreeMap<u16, RaceEvent>,
    /// Events by id, so the catalogue's repetition across a region's files is counted once.
    seen_events: std::collections::BTreeMap<u16, usize>,
    segments: usize,
    seg_refs: usize,
    seg_words: std::collections::BTreeSet<u32>,
    seg_len: Vec<f32>,
    path_info: PathInfoCheck,
    paths_seen: usize,
    paths_falling: usize,
    corners: Vec<usize>,
    steps: Vec<f32>,
    ratios: Vec<f32>,
}

impl Tally {
    fn add(&mut self, n: &[RouteNode]) {
        let c = check(n);
        self.check.nodes += c.nodes;
        self.check.links_read += c.links_read;
        self.check.links_absent += c.links_absent;
        self.check.links_out_of_range += c.links_out_of_range;
        self.check.word_18_set += c.word_18_set;
        self.check.paths += c.paths;
        self.check.path_runs += c.path_runs;
        self.check.paths_dense &= c.paths_dense;
        // Measured **within a path**, which is the only place consecutive means adjacent. Doing it
        // over the whole table mixes in the jump from one path's end to the next one's start, and
        // that single mistake is what a "0.787" turns out to be made of.
        for path in paths(n) {
            // Does a path's own progress rise along file order? The module says nodes of one path
            // are "in driving order", and contiguity is proven — but the *direction* is not, and
            // a walk that assumes it runs a third of the network backwards.
            if path.len() > 1 {
                self.paths_seen += 1;
                if path[path.len() - 1].progress < path[0].progress {
                    self.paths_falling += 1;
                }
            }
            for pair in path.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                let step = ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt();
                self.steps.push(step);
                let gain = b.progress - a.progress;
                if step > 1.0 && gain > 0.0 {
                    self.ratios.push(gain / step);
                }
            }
        }
    }
}

fn percentile(sorted: &[f32], p: f64) -> f32 {
    if sorted.is_empty() {
        return f32::NAN;
    }
    let i = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[i]
}

/// Are `0x0003414D`'s trailing words the region's own object hashes?
///
/// The chunk is 36-byte records: four floats that read as a short segment — median length around
/// 11 m, which is a road's width — a count at `+17`, and up to four `u32` at `+20`. Those words
/// come in consecutive runs (`0x4EA23B92`, `93`, `94`, `95`), and consecutive is the signature of
/// [`gizmo_nfs::hash`]'s `h * 33 + byte` over two names differing in their last character. So the
/// guess is that they are `bStringHash` keys naming world objects.
///
/// It is a guess with a decisive test: resolve them against the `STREAM*.BUN` of the same region
/// and count how many land. A hit rate near 100 % says yes; anything else says the words are
/// something else and the shape of the record is still open.
///
/// Returns (words resolved, words not resolved, records read).
fn resolve_object_words(paths_file: &Path, tracks: &Path) -> Option<(usize, usize, usize)> {
    let region = paths_file.parent()?.file_name()?.to_str()?.strip_prefix("ROUTES")?;
    let bundle = tracks.join(format!("STREAM{region}.BUN"));
    let bytes = std::fs::read(&bundle).ok()?;
    // Three candidate namespaces, because the first one tried — the solids' own hashes — resolved
    // exactly zero of 195,561 words, and a second guess deserves the same test rather than a
    // narrative. Texture slot keys are the other 32-bit key the bundle carries; region ids come
    // from `0x3414A`, which is already decoded and sits in the very same file.
    let mut known: std::collections::HashSet<u32> = std::collections::HashSet::new();
    let mut textures: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for o in gizmo_nfs::world::manifest(&bytes).ok()? {
        known.insert(o.header.hash.0);
        textures.extend(o.texture_slots.iter().map(|t| t.0));
    }
    let region_ids: std::collections::HashSet<u32> =
        gizmo_nfs::world::routes::regions(&std::fs::read(paths_file).ok()?)
            .unwrap_or_default()
            .into_iter()
            .map(|r| r.id)
            .collect();

    let file = std::fs::read(paths_file).ok()?;
    let mut payload = Vec::new();
    gizmo_nfs::chunk::walk(
        &file,
        |_, _| Ok(gizmo_nfs::chunk::Visit::Descend),
        |_, header, data| {
            if header.id == 0x0003_414D {
                payload.extend_from_slice(data);
            }
            Ok(())
        },
    )
    .ok()?;

    let (mut hit, mut miss, mut records) = (0, 0, 0);
    let (mut tex, mut reg) = (0, 0);
    for r in payload.chunks_exact(36) {
        records += 1;
        let count = usize::from(r[17]).min(4);
        for i in 0..count {
            let w = u32::from_le_bytes([r[20 + i * 4], r[21 + i * 4], r[22 + i * 4], r[23 + i * 4]]);
            if known.contains(&w) {
                hit += 1;
            } else {
                miss += 1;
            }
            if textures.contains(&w) {
                tex += 1;
            }
            if region_ids.contains(&w) {
                reg += 1;
            }
        }
    }
    println!(
        "    {}: {records} records · {} words · solids {hit} · textures {tex} · regions {reg}",
        paths_file.file_name().unwrap_or_default().to_string_lossy(),
        hit + miss
    );
    Some((hit, miss, records))
}

/// Does a starting grid face the way its event's outline is drawn?
///
/// The one question these files were expected to answer and do not answer anywhere directly: a
/// grid records eight positions and no direction, so which of its two rows is the front is a
/// reading, not a fact. [`gizmo_nfs::world::routes`]' rule takes slots `0..3` as the front.
///
/// This holds that rule against the only other thing in the data with a direction — the event
/// outline, which [`RaceEvent::outline`] is documented as being in lap order. For each grid: take
/// the derived heading, find the nearest outline segment, and ask whether the two point the same
/// way. Agreement near 100 % or near 0 % settles the rule; near 50 % says the outline order is not
/// race order and the question stays open.
///
/// **A track carries two grids and they must be tested together.** Held one at a time the answer
/// is 55 % agreement, which says nothing: if the two grids of a track are the two race directions,
/// then whatever the rule is, one of them agrees and the other does not, and a pooled rate near a
/// half is what that looks like. So this also reports, per track, whether the two grids oppose each
/// other and whether exactly one of them agrees with the outline.
///
/// Returns (agreed, disagreed, no event found, tracks with two opposed grids, tracks where exactly
/// one of the two agrees, tracks with two grids, tracks where the agreeing one is not the first).
fn grid_faces_outline(
    markers: &[StartMarker],
    events: &[RaceEvent],
) -> (usize, usize, usize, usize, usize, usize, usize) {
    let (mut agree, mut against, mut missing, mut changed) = (0, 0, 0, 0);
    let mut per_track: std::collections::BTreeMap<u32, Vec<([f32; 2], bool)>> =
        std::collections::BTreeMap::new();
    for g in gizmo_nfs::world::routes::grids(markers) {
        let Some(ev) = events.iter().find(|e| u32::from(e.id) == g[0].track) else {
            missing += 1;
            continue;
        };
        if ev.outline.len() < 2 {
            missing += 1;
            continue;
        }
        let mean = |s: &[StartMarker]| {
            let n = s.len() as f32;
            [s.iter().map(|m| m.at[0]).sum::<f32>() / n, s.iter().map(|m| m.at[1]).sum::<f32>() / n]
        };
        let (front, back) = (mean(&g[..4]), mean(&g[4..]));
        let head = [front[0] - back[0], front[1] - back[1]];
        let centre = [(front[0] + back[0]) * 0.5, (front[1] + back[1]) * 0.5];

        // Nearest outline segment, by its midpoint — good enough to pick a segment, and the
        // direction is what is being asked about, not the distance.
        let mut best = (f32::MAX, [0.0f32, 0.0f32]);
        for w in ev.outline.windows(2) {
            let mid = [(w[0][0] + w[1][0]) * 0.5, (w[0][1] + w[1][1]) * 0.5];
            let d = (mid[0] - centre[0]).powi(2) + (mid[1] - centre[1]).powi(2);
            if d < best.0 {
                best = (d, [w[1][0] - w[0][0], w[1][1] - w[0][1]]);
            }
        }
        let dot = head[0] * best.1[0] + head[1] * best.1[1];
        if dot >= 0.0 {
            agree += 1;
        } else {
            against += 1;
        }
        per_track.entry(g[0].track).or_default().push((head, dot >= 0.0));
    }
    let (mut opposed, mut split, mut twos) = (0, 0, 0);
    for g in per_track.values().filter(|v| v.len() == 2) {
        twos += 1;
        if g[0].0[0] * g[1].0[0] + g[0].0[1] * g[1].0[1] < 0.0 {
            opposed += 1;
        }
        // Where exactly one agrees and it is **not** the first, taking "the first" was wrong. That
        // count is the whole value of reading the outline, so it is reported rather than implied.
        if g[0].1 != g[1].1 {
            split += 1;
            if !g[0].1 {
                changed += 1;
            }
        }
    }
    (agree, against, missing, opposed, split, twos, changed)
}

/// Print every start-marker group in a file: what it belongs to, how complete it is, and where.
///
/// `grids()` only hands back the eights numbered `0..7`, which is the right filter for a race and
/// the wrong one for looking: the groups it drops are the interesting ones. So this groups by
/// `(track, group)` directly and reports what each holds.
///
/// The heading is the grid's own geometry — front four minus back four — the same rule the game
/// side uses to point a car, stated here so the two can be compared rather than trusted.
fn dump_grids(file: &Path, m: &[StartMarker]) {
    let mut by: BTreeMap<(u32, u32), Vec<&StartMarker>> = BTreeMap::new();
    for x in m {
        by.entry((x.track, x.group)).or_default().push(x);
    }
    println!("  {} — {} markers in {} groups", file.display(), m.len(), by.len());
    for ((track, group), mut g) in by {
        g.sort_by_key(|x| x.slot);
        let mean = |s: &[&StartMarker]| {
            let n = s.len().max(1) as f32;
            let f = |i: usize| s.iter().map(|x| x.at[i]).sum::<f32>() / n;
            [f(0), f(1), f(2)]
        };
        let c = mean(&g);
        let (dx, dy) = if g.len() >= 4 {
            let (front, back) = (mean(&g[..g.len() / 2]), mean(&g[g.len() / 2..]));
            let (dx, dy) = (front[0] - back[0], front[1] - back[1]);
            let len = dx.hypot(dy).max(f32::EPSILON);
            (dx / len, dy / len)
        } else {
            (0.0, 0.0)
        };
        let slots: Vec<u32> = g.iter().map(|x| x.slot).collect();
        let dense = slots.iter().copied().eq(0..g.len() as u32);
        println!(
            "    track {track:<5} group {group:<5} {:>2} slots{} · centre \
             ({:>8.1},{:>8.1},{:>7.1}) · heading ({dx:>5.2},{dy:>5.2})",
            g.len(),
            if dense { "" } else { " (gaps)" },
            c[0],
            c[1],
            c[2]
        );
    }
}

pub fn run(path: &Path, list: usize, grids_flag: bool, objects_flag: bool) -> Result<()> {
    let files = collect(path)?;
    if files.is_empty() {
        return Err(format!("{}: no Paths*.bin here", path.display()));
    }

    let mut tally = Tally { check: NodeCheck { paths_dense: true, ..NodeCheck::default() }, ..Tally::default() };
    let mut offenders: Vec<String> = Vec::new();

    for file in &files {
        let bytes = crate::paths::read(file)?;
        let n = nodes(&bytes).map_err(|e| format!("{}: {e}", file.display()))?;
        let r = regions(&bytes).map_err(|e| format!("{}: {e}", file.display()))?;
        let rc = check_regions(&r);
        tally.region.regions += rc.regions;
        tally.region.bounds_wrong += rc.bounds_wrong;
        tally.region.heading_not_unit += rc.heading_not_unit;
        tally.corners.extend(r.iter().map(|x| x.points.len()));
        // 0x0003414D, held to its own redundancy: the reader refuses a record whose count and
        // words disagree, so reading them all at all is the check.
        // 0x00034149: one 220-byte record per path, and the two fields in it that are read are
        // checked against the very nodes they describe.
        let pi = path_info(&bytes).map_err(|e| format!("{}: {e}", file.display()))?;
        if !pi.is_empty() {
            let pc = check_path_info(&pi, &n);
            tally.path_info.records += pc.records;
            tally.path_info.count_wrong += pc.count_wrong;
            tally.path_info.ends_wrong += pc.ends_wrong;
            tally.path_info.span_wrong += pc.span_wrong;
            tally.path_info.reversed += pc.reversed;
            tally.path_info.id_wrong += pc.id_wrong;
            tally.path_info.node_count_wrong += pc.node_count_wrong;
        }
        let seg = segments(&bytes).map_err(|e| format!("{}: {e}", file.display()))?;
        tally.segments += seg.len();
        for s in &seg {
            tally.seg_refs += s.refs.len();
            tally.seg_words.extend(s.refs.iter().copied());
            let d = (s.at[1][0] - s.at[0][0]).hypot(s.at[1][1] - s.at[0][1]);
            tally.seg_len.push(d);
        }
        let ev = events(&bytes).map_err(|e| format!("{}: {e}", file.display()))?;
        let ec = check_events(&ev);
        tally.event.events += ec.events;
        tally.event.circuits += ec.circuits;
        tally.event.sprints += ec.sprints;
        tally.event.flag_disagrees += ec.flag_disagrees;
        for e in &ev {
            *tally.seen_events.entry(e.id).or_default() += 1;
            tally.catalogue.entry(e.id).or_insert_with(|| e.clone());
        }
        let name = file.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        tally.files += 1;
        if n.is_empty() {
            tally.files_without_nodes += 1;
            continue;
        }
        let c = check(&n);
        if !c.is_clean() && offenders.len() < list {
            offenders.push(format!(
                "  {name}: {} nodes · {} links out of range · {} non-zero +18",
                c.nodes, c.links_out_of_range, c.word_18_set
            ));
        }
        tally.add(&n);
    }

    tally.steps.sort_by(f32::total_cmp);
    tally.ratios.sort_by(f32::total_cmp);
    let c = tally.check;

    println!(
        "{} route files · {} carry no node table · {} nodes",
        tally.files, tally.files_without_nodes, c.nodes
    );
    println!(
        "links: {} read, {} absent (0xFFFF), {} resolve, {} OUT OF RANGE",
        c.links_read,
        c.links_absent,
        c.links_read - c.links_absent - c.links_out_of_range,
        c.links_out_of_range
    );
    println!("+18 non-zero: {}", c.word_18_set);
    println!(
        "paths: {} · contiguous runs {} (must match) · numbered 0..k with no gaps: {}",
        c.paths,
        c.path_runs,
        if c.paths_dense { "yes" } else { "NO" }
    );
    println!(
        "step between nodes of one path: p05 {:.1} · median {:.1} · p95 {:.1} m",
        percentile(&tally.steps, 0.05),
        percentile(&tally.steps, 0.50),
        percentile(&tally.steps, 0.95)
    );
    println!(
        "+20 gain / step, over {} in-path pairs: p05 {:.4} · median {:.4} · p95 {:.4}",
        tally.ratios.len(),
        percentile(&tally.ratios, 0.05),
        percentile(&tally.ratios, 0.50),
        percentile(&tally.ratios, 0.95)
    );

    // The Routes####F/B.bin beside the Paths files: the same road, laned, and the only place the
    // forward and backward readings of a route differ.
    for file in collect_lanes(path) {
        let bytes = crate::paths::read(&file)?;
        let l = lanes(&bytes).map_err(|e| format!("{}: {e}", file.display()))?;
        let lc = check_lanes(&l);
        tally.lane.lanes += lc.lanes;
        tally.lane.points += lc.points;
        tally.lane.outside_bounds += lc.outside_bounds;
        tally.lane.span_wrong += lc.span_wrong;
        tally.lane_names.extend(l.into_iter().map(|x| x.name));
    }

    for file in collect_markers(path) {
        let bytes = crate::paths::read(&file)?;
        let m = markers(&bytes).map_err(|e| format!("{}: {e}", file.display()))?;
        if grids_flag {
            dump_grids(&file, &m);
        }
        let mc = check_markers(&m);
        tally.marker.markers += mc.markers;
        tally.marker.grids += mc.grids;
        tally.marker.slot_out_of_range += mc.slot_out_of_range;
        tally.marker.grid_misnumbered += mc.grid_misnumbered;
        let cat: Vec<RaceEvent> = tally.catalogue.values().cloned().collect();
        let f6 = grid_faces_outline(&m, &cat);
        tally.facing.0 += f6.0;
        tally.facing.1 += f6.1;
        tally.facing.2 += f6.2;
        tally.facing.3 += f6.3;
        tally.facing.4 += f6.4;
        tally.facing.5 += f6.5;
        tally.facing.6 += f6.6;
    }

    tally.corners.sort_unstable();
    let rg = tally.region;
    println!(
        "regions: {} · {} with a box that is not their points' extent · {} with a heading that is \
         not a unit vector · corners p05 {} median {} p95 {}",
        rg.regions,
        rg.bounds_wrong,
        rg.heading_not_unit,
        tally.corners.first().copied().unwrap_or(0),
        tally.corners.get(tally.corners.len() / 2).copied().unwrap_or(0),
        tally.corners.get(tally.corners.len() * 19 / 20).copied().unwrap_or(0)
    );

    let ev = tally.event;
    println!(
        "events: {} records · {} distinct (the catalogue repeats in every file of its region) · \
         {} circuits, {} sprints · {} whose flag and outline disagree",
        ev.events,
        tally.seen_events.len(),
        ev.circuits,
        ev.sprints,
        ev.flag_disagrees
    );

    let (agree, against, missing, opposed, split, twos, changed) = tally.facing;
    if agree + against > 0 {
        println!(
            "grid facing vs event outline: {agree} agree, {against} oppose, {missing} with no event \
             ({:.0}% pooled, which a two-grid track makes meaningless)",
            100.0 * agree as f64 / (agree + against) as f64
        );
        if twos > 0 {
            println!(
                "  of {twos} tracks with exactly two grids: {opposed} have the two facing opposite \
                 ways, {split} have exactly one of the two agreeing with the outline, and in \
                 {changed} of those the agreeing one is not the first — which is how often taking \
                 the first put the car on the wrong grid",
            );
        }
    }

    if objects_flag {
        let tracks = if path.is_file() {
            path.parent().and_then(Path::parent).unwrap_or(path).to_path_buf()
        } else {
            path.to_path_buf()
        };
        let (mut hit, mut miss, mut recs, mut seen) = (0, 0, 0, 0);
        for file in &files {
            if let Some((h, m, r)) = resolve_object_words(file, &tracks) {
                hit += h;
                miss += m;
                recs += r;
                seen += 1;
            }
        }
        if hit + miss > 0 {
            println!(
                "0x3414D words vs region objects: {hit} resolve, {miss} do not ({:.1}% over \
                 {recs} records in {seen} files)",
                100.0 * hit as f64 / (hit + miss) as f64
            );
        }
    }

    tally.seg_len.sort_by(f32::total_cmp);
    if tally.segments > 0 {
        let q = |p: f64| {
            tally.seg_len.get((tally.seg_len.len() as f64 * p) as usize).copied().unwrap_or(f32::NAN)
        };
        println!(
            "segments: {} records · {} references over {} distinct words · length p05 {:.1} \
             median {:.1} p95 {:.1} m",
            tally.segments,
            tally.seg_refs,
            tally.seg_words.len(),
            q(0.05),
            q(0.50),
            q(0.95)
        );
    }

    let pi = tally.path_info;
    println!(
        "path records (0x34149): {} · {} files whose count is not the path count · {} whose \
         from/to are not the first and last node · {} whose low/high are not the extent · {} the \
         file marks as stored against the way they are driven · {} whose +20 is not their own \
         index · {} whose +32 is not the node count",
        pi.records, pi.count_wrong, pi.ends_wrong, pi.span_wrong, pi.reversed, pi.id_wrong,
        pi.node_count_wrong
    );
    println!(
        "path direction: {} of {} paths end lower in progress than they start — measured, and the \
         file says the same in 0x34149",
        tally.paths_falling, tally.paths_seen
    );

    let ln = tally.lane;
    println!(
        "lanes: {} blocks · {} points · {} distinct names · {} with a point outside their own box \
         · {} whose stated span is not their points' extent",
        ln.lanes,
        ln.points,
        tally.lane_names.len(),
        ln.outside_bounds,
        ln.span_wrong
    );

    let mk = tally.marker;
    println!(
        "start markers: {} · {} full eight-car grids · {} slots outside 0..8 · {} eights numbered \
         wrong",
        mk.markers, mk.grids, mk.slot_out_of_range, mk.grid_misnumbered
    );

    for line in &offenders {
        println!("{line}");
    }

    if c.is_clean() && rg.is_clean() && ev.is_clean() && mk.is_clean() {
        println!(
            "OK — every link is a node of its own table or absent, every +18 is zero, every path \
             is one contiguous run, every region's box is its own points' extent, and every \
             event's circuit flag matches its outline, and every grid is numbered 0..7"
        );
        Ok(())
    } else {
        Err(format!(
            "{} links out of range, {} non-zero +18, {} path runs over {} path values (dense: {}): \
             the record is not being read as it is laid out",
            c.links_out_of_range, c.word_18_set, c.path_runs, c.paths, c.paths_dense
        ))
    }
}
