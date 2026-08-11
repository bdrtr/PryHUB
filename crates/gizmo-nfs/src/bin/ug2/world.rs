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

/// Write the city as OBJ + MTL + PNG, for opening in a modeller.
///
/// **Why this is worth having beyond the novelty.** Every question this crate answered about the
/// city was answered by rendering it and squinting, which is slow and gets things wrong: two roofs
/// on one car and a distance imposter standing in open ground both went unnoticed in a running
/// frame and are obvious the moment the geometry is in front of you. A modeller is the instrument
/// that was missing.
///
/// `y_up` rotates the result for importers left on their defaults — see [`write_world_obj`] for why
/// "NFSU2 is Z-up and so is Blender" is true and still lands the city on its side.
///
/// Byte-identical repeats are dropped — 14,847 of the eight bundles' 28,985 objects are one, and
/// leaving them in gives every surface a twin to z-fight with.
fn export(path: &Path, out: &Path, y_up: bool) -> Result<()> {
    use gizmo_nfs::export::{png_bytes, png_name, write_mtl, write_world_obj, Material};
    use gizmo_nfs::types::AssetHash;
    use gizmo_nfs::world::{meshes, packs};
    use std::collections::{BTreeMap, HashSet};

    let bundles = collect(path)?;
    if bundles.is_empty() {
        return Err(format!("{}: no STREAM*.BUN here", path.display()));
    }
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;

    let bytes: Vec<Vec<u8>> = bundles.iter().map(|b| crate::paths::read(b)).collect::<Result<_>>()?;
    let mut all = Vec::new();
    let mut all_packs = Vec::new();
    for b in &bytes {
        all.extend(meshes(b).map_err(|e| format!("{}: {e}", path.display()))?);
        all_packs.extend(packs(b).map_err(|e| format!("{}: {e}", path.display()))?);
    }
    let declared = all.len();
    let mut seen = HashSet::new();
    all.retain(|m| {
        let bits = |v: [f32; 3]| [v[0].to_bits(), v[1].to_bits(), v[2].to_bits()];
        seen.insert((
            m.header.hash.0,
            m.positions.len(),
            m.indices.len(),
            bits(m.header.bbox_min),
            bits(m.header.bbox_max),
            m.header.matrix.map(|r| r.map(f32::to_bits)),
        ))
    });
    println!("{declared} objects, {} after dropping byte-identical repeats", all.len());

    // One material per texture key, plus one for the runs whose key resolves nowhere — those draw
    // grey rather than vanishing, the same decision the renderer makes and for the same reason.
    // `NfsMaterialRange::hash` is already the resolved key: the file names a run's texture by
    // *index* into the solid's `0x00134012` list and the reader turns that into the hash while it
    // walks. Treating it as an index again happens to work — a 32-bit hash never lands inside a
    // slot list of a few entries, so the lookup misses and the fallback returns the hash — and it
    // is the kind of accident that survives until one hash is small.
    let key_of = |m: &gizmo_nfs::world::WorldMesh, run: usize| -> Option<AssetHash> {
        if m.groups.is_empty() {
            return m.texture_slots.first().copied();
        }
        m.groups.get(run).map(|g| g.hash)
    };
    let mut images: BTreeMap<u32, String> = BTreeMap::new();
    let mut materials = Vec::new();
    for m in &all {
        for run in 0..m.groups.len().max(1) {
            let Some(k) = key_of(m, run) else { continue };
            if images.contains_key(&k.0) {
                continue;
            }
            let decoded = all_packs.iter().find_map(|p| p.get(k).and_then(|t| p.decode(t).ok()));
            let Some(img) = decoded else { continue };
            let name = png_name(&img);
            let png = png_bytes(&img).map_err(|e| format!("{name}: {e}"))?;
            std::fs::write(out.join(&name), png).map_err(|e| format!("{name}: {e}"))?;
            materials.push(Material {
                name: format!("tex_{:08X}", k.0),
                map: Some(name.clone()),
                map_is_cutout: img.rgba.chunks_exact(4).any(|p| p[3] < 250),
                diffuse: [1.0, 1.0, 1.0],
            });
            images.insert(k.0, name);
        }
    }
    materials.push(Material {
        name: "unresolved".into(),
        map: None,
        map_is_cutout: false,
        diffuse: [0.35, 0.35, 0.38],
    });
    println!("{} textures written, {} materials", images.len(), materials.len());

    let obj = write_world_obj(&all, "city.mtl", y_up, |m, run| {
        key_of(m, run)
            .filter(|k| images.contains_key(&k.0))
            .map_or_else(|| "unresolved".to_string(), |k| format!("tex_{:08X}", k.0))
    });
    std::fs::write(out.join("city.obj"), obj).map_err(|e| format!("city.obj: {e}"))?;
    std::fs::write(out.join("city.mtl"), write_mtl(&materials))
        .map_err(|e| format!("city.mtl: {e}"))?;
    println!("wrote {}/city.obj and city.mtl", out.display());
    Ok(())
}

pub fn run(path: &Path, csv: bool, out: Option<&Path>, y_up: bool) -> Result<()> {
    if let Some(out) = out {
        return export(path, out, y_up);
    }
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
        outln!(
            "  objects            {:>12}{}",
            total.objects,
            count_note(total.no_mesh, "without a mesh")
        );
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
        // Summed per bundle, not recomputed over the union: a solid shipped by two regions is a
        // duplicate no line here counts. Saying so beats a reader taking this for a city-wide
        // figure — a consumer that dedups across all eight bundles at once drops far more than
        // this, and the difference is not a bug in either.
        outln!("  duplicate keys     {:>12}   (summed per bundle)", total.duplicate_keys);
        outln!("  truncated names    {:>12}", total.truncated);
    }
    Ok(())
}

/// One bundle, or every `STREAM*.BUN` in a directory, in name order.
pub(super) fn collect(path: &Path) -> Result<Vec<std::path::PathBuf>> {
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
    /// Solids that declared no vertices. Carried into the totals for the same reason it is on the
    /// per-bundle line: it is the one figure that says how much of a region did not become geometry.
    no_mesh: usize,
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
        self.no_mesh += s.no_mesh;
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
        //
        // The matrix has to be *in* the key for that sentence to be true, and it was not. The box
        // does not stand in for it: `bbox_min`/`bbox_max` are in the solid's own frame, and a
        // placed solid — 16,071 of the city's 28,985 — has a local box and a matrix that puts it
        // somewhere. Two copies of one prop on two corners were counted as a duplicate.
        type Key = (u32, u32, u32, [u32; 6], [[u32; 4]; 4]);
        let mut seen: BTreeMap<Key, usize> = BTreeMap::new();
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
            let matrix = o.header.matrix.map(|row| row.map(f32::to_bits));
            *seen.entry((o.header.hash.0, o.vertices, o.triangles, bbox, matrix)).or_default() += 1;
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
        outln!(
            "  objects            {:>12}{}",
            self.objects,
            count_note(self.no_mesh, "without a mesh")
        );
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
        outln!(
            "  duplicate keys     {:>12}{}",
            self.duplicate_keys,
            note(self.duplicate_keys, "same hash+counts+bbox+matrix")
        );
        outln!();
    }
}

/// A trailing `(label)` when `n` is non-zero, for a statistic whose count already has its own
/// column — `n` is only the test for whether the label is worth printing.
fn note(n: usize, what: &str) -> String {
    if n == 0 {
        String::new()
    } else {
        format!("   ({what})")
    }
}

/// A trailing `(n label)` when `n` is non-zero, for a statistic that has **no** column of its own.
///
/// `no_mesh` is the only one: it shares the `objects` line, whose `{:>12}` slot is taken by the
/// object count. It used to be printed with [`note`], which takes `n` only to compare it against
/// zero — so a bundle could say it had meshless solids but never how many, and the one number that
/// says how much of a region this reader could not turn into geometry was unreportable. Ask for a
/// count and the count travels with the label.
fn count_note(n: usize, what: &str) -> String {
    if n == 0 {
        String::new()
    } else {
        format!("   ({n} {what})")
    }
}

fn mb(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}
