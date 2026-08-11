//! `ug2 track` — read the route files' node tables, and make them answer for themselves.
//!
//! [`super::verify`] does this for the city: decode it, then compare the decode against what the
//! file declared. A route file declares no counts to compare against, so the check has to come
//! from the record's own internal consistency instead — which turns out to be a stronger test than
//! it sounds, because three of the six fields are indices into the very table they sit in.
//!
//! Two things are asserted over every node, and both are zero across the install's 18,064:
//!
//! - a `+12`/`+14`/`+16` that is neither `0xFFFF` nor a valid index into its own file's table,
//! - a `+18` that is not zero.
//!
//! That is 54,192 index reads with no exception. A misaligned or misread field does not survive
//! that: it would have to be small enough, in every one of eighteen thousand records, to pass for
//! an index into a table as short as five entries.
//!
//! The report also states the two distributions the layout was read from, so the claim in
//! [`gizmo_nfs::world::routes`] can be re-derived rather than believed: the step between
//! consecutive nodes, and the ratio of the `+20` delta to that step. The second is the one worth
//! watching — it is what says `+20` is a distance in world units and not some other quantity.
//!
//! ```bash
//! ug2 track "$NFSU2_ROOT/TRACKS"          # every route file
//! ug2 track "$NFSU2_ROOT/TRACKS/ROUTESL4RA/Paths4001.bin"
//! ```
//!
//! Exits non-zero if any check fails, so it can be a step in a script rather than something to read.

use crate::paths::Result;
use gizmo_nfs::world::routes::{check, nodes, NodeCheck, RouteNode};
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

/// A run of consecutive nodes is only a run while the step stays small. The jumps are where one
/// path in the table ends and the next begins, and measuring across one turns a 30 m step into a
/// 2 km one and the ratio below into noise.
const RUN_STEP_LIMIT: f32 = 100.0;

#[derive(Default)]
struct Tally {
    files: usize,
    files_without_nodes: usize,
    check: NodeCheck,
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
        for pair in n.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let step = ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt();
            self.steps.push(step);
            let gain = b.progress - a.progress;
            if step > 1.0 && step < RUN_STEP_LIMIT && gain > 0.0 {
                self.ratios.push(gain / step);
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

    let mut tally = Tally::default();
    let mut offenders: Vec<String> = Vec::new();

    for file in &files {
        let bytes = crate::paths::read(file)?;
        let n = nodes(&bytes).map_err(|e| format!("{}: {e}", file.display()))?;
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
        "step between consecutive nodes: p05 {:.1} · median {:.1} · p95 {:.1} m",
        percentile(&tally.steps, 0.05),
        percentile(&tally.steps, 0.50),
        percentile(&tally.steps, 0.95)
    );
    println!(
        "+20 gain / step, over {} in-run pairs: p05 {:.4} · median {:.4} · p95 {:.4}",
        tally.ratios.len(),
        percentile(&tally.ratios, 0.05),
        percentile(&tally.ratios, 0.50),
        percentile(&tally.ratios, 0.95)
    );

    for line in &offenders {
        println!("{line}");
    }

    if c.is_clean() {
        println!("OK — every link is a node of its own table or absent, and every +18 is zero");
        Ok(())
    } else {
        Err(format!(
            "{} links out of range and {} non-zero +18: the record is not being read as it is laid out",
            c.links_out_of_range, c.word_18_set
        ))
    }
}
