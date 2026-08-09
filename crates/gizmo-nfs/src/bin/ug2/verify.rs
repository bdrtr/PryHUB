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
use gizmo_nfs::world::{manifest, meshes};
use std::path::Path;

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
    }

    fn print(&self, name: &str) {
        outln!("── {name} ──");
        outln!("  solids             {:>12}", self.solids);
        outln!("  declared           {:>12} vertices · {} triangles", self.declared_v, self.declared_t);
        outln!("  decoded            {:>12} vertices · {} triangles", self.decoded_v, self.decoded_t);
        outln!("  no mesh declared   {:>12}", self.no_mesh);
        outln!("  DROPPED            {:>12}   (declared geometry, decoded none)", self.empty);
        outln!("  SHORT              {:>12}   (decoded a different count)", self.partial);
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

        let mut t = Tally { solids: declared.len(), ..Tally::default() };
        for (info, mesh) in declared.iter().zip(&decoded) {
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

    if shorts.is_empty() {
        outln!("every solid decoded to what it declared.");
        return Ok(());
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
    Err(format!("{} solid(s) lost geometry the file declares", shorts.len()))
}
