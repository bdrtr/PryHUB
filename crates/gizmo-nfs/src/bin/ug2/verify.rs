//! `ug2 verify` — does the reader produce the geometry the file declares?
//!
//! [`super::world`] reads the headers and stops, which answers "what is in here". This one also
//! decodes every buffer and compares the two, which answers the question that actually matters:
//! **did anything go missing on the way in?**
//!
//! Nothing asked that before, and the cost of not asking is measured. 234 of the city's 28,985
//! solids use the standard 36-byte vertex record while `world::object` demanded exactly 24; every
//! one came back as an object with empty arrays, and an empty object is skipped without comment by
//! everything downstream. The city was missing its road signs, median poles and chevrons, and no
//! number anywhere moved. This is the number that would have moved.
//!
//! The check rests on a promise `world::meshes` already makes: an unreadable solid is returned with
//! empty arrays *rather than dropped*, so its output lines up one-to-one with the manifest's. That
//! makes the comparison a zip rather than a search, and it also means the alignment itself is worth
//! asserting — if it ever stops holding, every other number here is meaningless.
//!
//! ```bash
//! ug2 verify "$NFSU2_ROOT/TRACKS"
//! ```
//!
//! Exits non-zero when anything is short, so it can be a step in a script rather than something to
//! read.

use crate::paths::Result;
use gizmo_nfs::chunk::{walk_positions, Visit, WalkOptions};
use gizmo_nfs::geometry::format::{
    INDEX_BUFFER, MATERIAL_RANGES, MAT_RANGE_STRIDE, SOLID_HEADER,
};
use gizmo_nfs::geometry::skip_leading_filler;
use gizmo_nfs::types::AssetHash;
use gizmo_nfs::world::{manifest, meshes};
use std::path::Path;

/// What the *file* says about a solid's run table and index buffer.
///
/// Read here with its own walk rather than taken from `world::meshes`, and that is the point: a
/// check fed by the reader it is checking can only ever agree with it. The vertex comparison above
/// gets its independent side from the mesh header, which `manifest` reads and `meshes` reads
/// separately; these two quantities have no such second source, so this is it.
struct Raw {
    /// 60-byte entries in the `0x00134B02` payload, after its `0x11` filler.
    runs: usize,
    /// Bytes of `0x00134B03` payload, after its filler.
    ibuf: usize,
}

fn raw_of(bundle: &[u8]) -> Result<Vec<Raw>> {
    let mut out: Vec<Raw> = Vec::new();
    walk_positions(
        bundle,
        WalkOptions::default(),
        |_, _| Ok(Visit::Descend),
        |_, header, _, data| {
            match header.id {
                SOLID_HEADER => out.push(Raw { runs: 0, ibuf: 0 }),
                MATERIAL_RANGES => {
                    if let Some(p) = out.last_mut() {
                        p.runs = skip_leading_filler(data).len() / MAT_RANGE_STRIDE;
                    }
                }
                INDEX_BUFFER => {
                    if let Some(p) = out.last_mut() {
                        p.ibuf = skip_leading_filler(data).len();
                    }
                }
                _ => {}
            }
            Ok(())
        },
    )
    .map_err(|e| format!("{e}"))?;
    Ok(out)
}

/// The bytes a `t`-triangle index buffer occupies: three `u16` per triangle, padded to a multiple
/// of four. Exact, not a floor — a buffer larger than this is a buffer the reader is guessing at.
fn ibuf_bytes(triangles: usize) -> usize {
    triangles * 6 + if triangles % 2 == 1 { 2 } else { 0 }
}

/// One solid that did not decode to what it declared.
struct Short {
    bundle: String,
    name: String,
    declared: (usize, usize),
    decoded: (usize, usize),
}

#[derive(Default)]
struct Tally {
    solids: usize,
    declared_v: u64,
    declared_t: u64,
    decoded_v: u64,
    decoded_t: u64,
    /// Declared geometry and produced none — the silent-drop shape.
    empty: usize,
    /// Produced a different count than declared, without being empty.
    partial: usize,
    /// Declared nothing and produced nothing. Not a fault: 79 city solids carry an empty mesh.
    no_mesh: usize,

    // ── The rest of the record, past the vertices ──
    /// 60-byte run entries the file declares.
    runs_declared: usize,
    /// Runs `material_ranges` emitted. It drops a run whose count is zero or whose slice falls
    /// outside the index buffer, silently — and a dropped run is triangles that never get a texture.
    runs_kept: usize,
    /// Solids whose index buffer is not exactly the size their triangle count implies.
    ibuf_wrong: usize,
    /// Runs whose material index missed the solid's slot list. `material_ranges` answers
    /// `AssetHash(0)` there, and no city slot is legitimately zero — `manifest` measured that over
    /// all 70,439 of them — so a zero here is the fallback firing, not a texture.
    matidx_lost: usize,
    /// Solids whose runs do not add up to the index buffer they split.
    runs_short: usize,
}

impl Tally {
    fn add(&mut self, other: &Tally) {
        self.solids += other.solids;
        self.declared_v += other.declared_v;
        self.declared_t += other.declared_t;
        self.decoded_v += other.decoded_v;
        self.decoded_t += other.decoded_t;
        self.empty += other.empty;
        self.partial += other.partial;
        self.no_mesh += other.no_mesh;
        self.runs_declared += other.runs_declared;
        self.runs_kept += other.runs_kept;
        self.ibuf_wrong += other.ibuf_wrong;
        self.matidx_lost += other.matidx_lost;
        self.runs_short += other.runs_short;
    }

    fn print(&self, name: &str) {
        outln!("── {name} ──");
        outln!("  solids             {:>12}", self.solids);
        outln!("  declared           {:>12} vertices · {} triangles", self.declared_v, self.declared_t);
        outln!("  decoded            {:>12} vertices · {} triangles", self.decoded_v, self.decoded_t);
        outln!("  no mesh declared   {:>12}", self.no_mesh);
        outln!("  DROPPED            {:>12}   (declared geometry, decoded none)", self.empty);
        outln!("  SHORT              {:>12}   (decoded a different count)", self.partial);
        outln!("  runs               {:>12}   declared, {} kept", self.runs_declared, self.runs_kept);
        outln!("  RUNS DROPPED       {:>12}   (count 0, or a slice outside the index buffer)", self.runs_declared - self.runs_kept);
        outln!("  RUNS UNTEXTURED    {:>12}   (material index missed the slot list)", self.matidx_lost);
        outln!("  RUNS NOT COVERING  {:>12}   (solids whose runs do not add up to their indices)", self.runs_short);
        outln!("  INDEX BUFFER WRONG {:>12}   (not exactly the declared triangle count)", self.ibuf_wrong);
        outln!();
    }
}

pub fn run(path: &Path, list: usize) -> Result<()> {
    let bundles = super::world::collect(path)?;
    if bundles.is_empty() {
        return Err(format!("{}: no STREAM*.BUN here", path.display()));
    }

    let mut total = Tally::default();
    let mut shorts: Vec<Short> = Vec::new();

    for bundle in &bundles {
        let bytes = crate::paths::read(bundle)?;
        let name = bundle.file_stem().unwrap_or_default().to_string_lossy().into_owned();

        let declared = manifest(&bytes).map_err(|e| format!("{}: {e}", bundle.display()))?;
        let decoded = meshes(&bytes).map_err(|e| format!("{}: {e}", bundle.display()))?;

        // The one-to-one promise, asserted rather than assumed: every count below is a zip, and a
        // zip over lists that have drifted compares unrelated solids and reports nothing wrong.
        if declared.len() != decoded.len() {
            return Err(format!(
                "{name}: manifest has {} objects and the reader produced {} — \
                 world::meshes no longer lines up one-to-one with world::manifest, \
                 so nothing else this command prints can be trusted",
                declared.len(),
                decoded.len()
            ));
        }

        let raw = raw_of(&bytes)?;
        if raw.len() != declared.len() {
            return Err(format!(
                "{name}: the chunk walk found {} solid headers and the manifest {} —                  the two disagree about what a solid is",
                raw.len(),
                declared.len()
            ));
        }

        let mut t = Tally { solids: declared.len(), ..Tally::default() };
        for ((info, mesh), raw) in declared.iter().zip(&decoded).zip(&raw) {
            // The run table and the index buffer, checked against the file rather than the reader.
            t.runs_declared += raw.runs;
            t.runs_kept += mesh.groups.len();
            if info.triangles > 0 && raw.ibuf != ibuf_bytes(info.triangles as usize) {
                t.ibuf_wrong += 1;
            }
            t.matidx_lost += mesh
                .groups
                .iter()
                .filter(|g| g.hash == AssetHash(0) && !mesh.texture_slots.is_empty())
                .count();
            if !mesh.groups.is_empty() {
                let covered: usize = mesh.groups.iter().map(|g| g.index_count).sum();
                if covered != mesh.indices.len() {
                    t.runs_short += 1;
                }
            }

            let want = (info.vertices as usize, info.triangles as usize);
            let got = (mesh.positions.len(), mesh.triangles());
            t.declared_v += u64::from(info.vertices);
            t.declared_t += u64::from(info.triangles);
            t.decoded_v += got.0 as u64;
            t.decoded_t += got.1 as u64;

            if want.0 == 0 {
                t.no_mesh += 1;
            } else if got.0 == 0 {
                t.empty += 1;
            } else if want != got {
                t.partial += 1;
            } else {
                continue;
            }
            if want.0 != 0 && want != got {
                shorts.push(Short {
                    bundle: name.clone(),
                    name: info.header.name.clone(),
                    declared: want,
                    decoded: got,
                });
            }
        }

        t.print(&name);
        total.add(&t);
    }

    if bundles.len() > 1 {
        total.print(&format!("all {} bundles", bundles.len()));
    }

    // Every one of these is zero on a whole install and none of them is vacuous: the index-buffer
    // size is evaluated for all 28,906 mesh-bearing solids and the three run checks for all 70,548
    // runs, so a silent zero here means the checks ran and found nothing, not that they had nothing
    // to run on.
    let dropped_runs = total.runs_declared - total.runs_kept;
    let faults = [
        (shorts.len(), "solid(s) lost geometry the file declares"),
        (dropped_runs, "run(s) were dropped for a count or a slice the reader would not trust"),
        (total.matidx_lost, "run(s) lost their texture to an out-of-range material index"),
        (total.runs_short, "solid(s) have runs that do not cover their index buffer"),
        (total.ibuf_wrong, "solid(s) have an index buffer that is not the declared triangle count"),
    ];
    let summary: Vec<String> = faults
        .iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, what)| format!("{n} {what}"))
        .collect();

    if summary.is_empty() {
        outln!("every solid decoded to what it declared, and every run reached its texture.");
        return Ok(());
    }
    if shorts.is_empty() {
        return Err(summary.join("; "));
    }

    // Named, not counted. A total says something is wrong; a name says where to look, and the
    // first thing anyone will want is whether the losses share a family.
    outln!("{} solid(s) did not decode to what they declared:", shorts.len());
    for s in shorts.iter().take(list) {
        outln!(
            "  {:<14} {:<28} declared {} v / {} t → decoded {} v / {} t",
            s.bundle,
            s.name,
            s.declared.0,
            s.declared.1,
            s.decoded.0,
            s.decoded.1
        );
    }
    if shorts.len() > list {
        outln!("  … and {} more (--list to raise the cap)", shorts.len() - list);
    }
    Err(summary.join("; "))
}
