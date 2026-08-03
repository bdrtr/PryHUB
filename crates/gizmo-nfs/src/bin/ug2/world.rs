//! `ug2 world` — what a city bundle declares, without decoding a vertex.
//!
//! This is the measurement command for `TRACKS/STREAM*.BUN`. It answers the questions that decide
//! how the city can be loaded at all — how many objects, how much vertex data, how many of them are
//! duplicates — and the ones that say whether the *reader* is right: the filler histogram, the
//! identity/placed split, the determinant signs.
//!
//! Point it at one bundle or at the `TRACKS` directory for all of them.

use crate::paths::Result;
use gizmo_nfs::geometry::PACKED_VERTEX_STRIDE;
use gizmo_nfs::world::{manifest, WorldObjectInfo};
use std::collections::BTreeMap;
use std::path::Path;

/// Bytes per vertex once expanded to the engine's current non-indexed vertex. Reported beside the
/// on-disk figure because the ratio between them is what decides whether indexed geometry is
/// needed before the city can be held in memory at all.
const ENGINE_VERTEX_STRIDE: usize = 92;

pub fn run(path: &Path, csv: bool) -> Result<()> {
    let bundles = collect(path)?;
    if bundles.is_empty() {
        return Err(format!("{}: no STREAM*.BUN here", path.display()));
    }

    if csv {
        outln!("region,objects,vertices,triangles,disk_mb,engine_mb,identity,placed,neg_det,dup_keys,names_truncated");
    }

    let mut total = Totals::default();
    for bundle in &bundles {
        let bytes = crate::paths::read(bundle)?;
        let objects =
            manifest(&bytes).map_err(|e| format!("{}: {e}", bundle.display()))?;
        let name = bundle.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let s = Stats::of(&objects);
        total.add(&s);
        if csv {
            outln!(
                "{name},{},{},{},{:.1},{:.1},{},{},{},{},{}",
                s.objects, s.vertices, s.triangles,
                mb(s.disk_bytes()), mb(s.engine_bytes()),
                s.identity, s.placed, s.negative_det, s.duplicate_keys, s.truncated
            );
        } else {
            s.print(&name);
        }
    }

    if !csv && bundles.len() > 1 {
        outln!();
        outln!("── all {} bundles ──", bundles.len());
        outln!("  objects            {:>12}", total.objects);
        outln!("  vertices           {:>12}", total.vertices);
        outln!("  triangles          {:>12}", total.triangles);
        outln!(
            "  vertex data        {:>9.1} MB on disk @ {PACKED_VERTEX_STRIDE} B",
            mb(total.vertices * PACKED_VERTEX_STRIDE as u64)
        );
        outln!(
            "                     {:>9.1} MB expanded @ {ENGINE_VERTEX_STRIDE} B  ({:.1}x)",
            mb(total.vertices * ENGINE_VERTEX_STRIDE as u64),
            ENGINE_VERTEX_STRIDE as f64 / PACKED_VERTEX_STRIDE as f64
        );
        outln!("  placed / identity  {:>12} / {}", total.placed, total.identity);
        outln!("  duplicate keys     {:>12}", total.duplicate_keys);
        outln!("  truncated names    {:>12}", total.truncated);
    }
    Ok(())
}

/// One bundle, or every `STREAM*.BUN` in a directory, in name order.
fn collect(path: &Path) -> Result<Vec<std::path::PathBuf>> {
    if path.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }
    let dir = std::fs::read_dir(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut out: Vec<_> = dir
        .filter_map(std::result::Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("STREAM") && n.ends_with(".BUN"))
        })
        .collect();
    out.sort();
    Ok(out)
}

#[derive(Default)]
struct Totals {
    objects: usize,
    vertices: u64,
    triangles: u64,
    identity: usize,
    placed: usize,
    duplicate_keys: usize,
    truncated: usize,
}

impl Totals {
    fn add(&mut self, s: &Stats) {
        self.objects += s.objects;
        self.vertices += s.vertices;
        self.triangles += s.triangles;
        self.identity += s.identity;
        self.placed += s.placed;
        self.duplicate_keys += s.duplicate_keys;
        self.truncated += s.truncated;
    }
}

struct Stats {
    objects: usize,
    vertices: u64,
    triangles: u64,
    identity: usize,
    placed: usize,
    negative_det: usize,
    truncated: usize,
    no_mesh: usize,
    duplicate_keys: usize,
    filler: BTreeMap<usize, usize>,
}

impl Stats {
    fn of(objects: &[WorldObjectInfo]) -> Self {
        let mut filler: BTreeMap<usize, usize> = BTreeMap::new();
        // The dedup key is deliberately coarse-but-exact: an instanced prop at a different place
        // has a different matrix and survives, while a genuinely duplicated mesh does not.
        let mut seen: BTreeMap<(u32, u32, u32, [u32; 6]), usize> = BTreeMap::new();
        let (mut identity, mut placed, mut neg, mut truncated, mut no_mesh) = (0, 0, 0, 0, 0);
        let (mut vertices, mut triangles) = (0u64, 0u64);

        for o in objects {
            *filler.entry(o.filler).or_default() += 1;
            vertices += u64::from(o.vertices);
            triangles += u64::from(o.triangles);
            if o.header.is_placed() {
                placed += 1;
            } else {
                identity += 1;
            }
            if o.header.basis_determinant() < 0.0 {
                neg += 1;
            }
            if !o.header.name_is_whole() {
                truncated += 1;
            }
            if o.vertices == 0 {
                no_mesh += 1;
            }
            let mut bbox = [0u32; 6];
            for (i, v) in o.header.bbox_min.iter().chain(&o.header.bbox_max).enumerate() {
                bbox[i] = v.to_bits();
            }
            *seen.entry((o.header.hash.0, o.vertices, o.triangles, bbox)).or_default() += 1;
        }
        let duplicate_keys = seen.values().filter(|&&n| n > 1).map(|n| n - 1).sum();

        Self {
            objects: objects.len(),
            vertices,
            triangles,
            identity,
            placed,
            negative_det: neg,
            truncated,
            no_mesh,
            duplicate_keys,
            filler,
        }
    }

    fn disk_bytes(&self) -> u64 {
        self.vertices * PACKED_VERTEX_STRIDE as u64
    }

    fn engine_bytes(&self) -> u64 {
        self.vertices * ENGINE_VERTEX_STRIDE as u64
    }

    fn print(&self, name: &str) {
        outln!("── {name} ──");
        outln!("  objects            {:>12}{}", self.objects, note(self.no_mesh, "without a mesh"));
        outln!("  vertices           {:>12}", self.vertices);
        outln!("  triangles          {:>12}", self.triangles);
        outln!(
            "  vertex data        {:>9.1} MB on disk @ {PACKED_VERTEX_STRIDE} B  ·  {:.1} MB expanded @ {ENGINE_VERTEX_STRIDE} B",
            mb(self.disk_bytes()),
            mb(self.engine_bytes())
        );
        let widths: Vec<String> =
            self.filler.iter().map(|(w, n)| format!("{w}:{n}")).collect();
        outln!("  0x11 filler        {:>12}", widths.join("  "));
        outln!(
            "  placement          {:>12}{}",
            format!("{} placed", self.placed),
            format!(", {} identity (vertices already world-space)", self.identity)
        );
        outln!("  negative det       {:>12}{}", self.negative_det, note(self.negative_det, "mirrored"));
        outln!("  truncated names    {:>12}{}", self.truncated, note(self.truncated, "name_is_whole() false"));
        outln!("  duplicate keys     {:>12}{}", self.duplicate_keys, note(self.duplicate_keys, "same hash+counts+bbox"));
        outln!();
    }
}

fn note(n: usize, what: &str) -> String {
    if n == 0 {
        String::new()
    } else {
        format!("   ({what})")
    }
}

fn mb(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}
