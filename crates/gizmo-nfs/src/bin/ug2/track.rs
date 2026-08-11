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
    check, check_regions, nodes, paths, regions, NodeCheck, RegionCheck, RouteNode,
};
use std::path::{Path, PathBuf};

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

pub fn run(path: &Path, list: usize) -> Result<()> {
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

    for line in &offenders {
        println!("{line}");
    }

    if c.is_clean() && rg.is_clean() {
        println!(
            "OK — every link is a node of its own table or absent, every +18 is zero, every path \
             is one contiguous run, and every region's box is its own points' extent"
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
