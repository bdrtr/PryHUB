//! `Dışa Aktar` — writing what is on screen to disk.
//!
//! PryHUB carries no format knowledge of its own: the OBJ/MTL text and the PNG bytes come from
//! [`gizmo_nfs::export`], the same code `ug2 export` runs, so the two tools cannot end up
//! disagreeing about what a car is. What lives here is only *where* the files go and *what is on
//! screen* — and the second question is answered exactly as the 3D tab answers it, so an export
//! writes the thing the viewport was showing.
//!
//! There is no file dialog (see `Cargo.toml`): the files land under `pryhub-export/` in the
//! working directory and the log says the full path, the same way `ug2` prints what it wrote.

use crate::doc::Doc;
use crate::i18n::Strings;
use gizmo_nfs::export::{self, MaterialPlan};
use gizmo_nfs::NfsMeshPart;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// What an export produced.
pub struct Written {
    /// A one-line account of the contents, for the log.
    pub summary: String,
    /// Where it went — the first path is what the log points at.
    pub files: Vec<PathBuf>,
}

/// What to write, decided on the UI thread and carried to the worker.
///
/// It holds no borrows: the job runs while the interface keeps drawing, so anything it reads has to
/// be owned or shared. The already-decoded textures come along when there are some, and the job
/// decodes them itself when there are not — which is fine off the UI thread, where blocking is
/// what the thread is for.
pub struct ExportSpec {
    pub kind: Kind,
    /// The selected chunk, which decides *which* model is written.
    pub selection: Option<usize>,
    /// The interface's language, so the summary line reads in it.
    pub strings: &'static Strings,
    pub textures: Option<Arc<gizmo_nfs::Tpk>>,
}

/// The three things the button can mean.
pub enum Kind {
    /// Every decoded texture in the pack.
    Textures,
    /// The model the 3D tab is showing, as glTF + OBJ + its textures.
    Model,
    /// Just the one image the preview pane has up.
    OneTexture(gizmo_nfs::AssetHash),
}

/// Write what the centre area was showing when the button was pressed.
///
/// # Errors
/// Returns a human-readable message when there is nothing to write (no textures, no texture
/// selected, a car whose `GEOMETRY.BIN` holds no parts) or when a write fails.
pub fn run(doc: &Doc, spec: &ExportSpec) -> Result<Written, String> {
    let out = out_dir(doc)?;
    // Decoding here rather than on the UI thread is the point of this being a job.
    let decoded = match &spec.textures {
        Some(tpk) => Some(Arc::clone(tpk)),
        None => doc.decode_textures().map(Arc::new),
    };
    let tpk = decoded.as_deref();
    let has_images = tpk.is_some_and(|t| !t.textures.is_empty());
    match spec.kind {
        Kind::OneTexture(hash) => one_texture(tpk, &out, hash),
        Kind::Textures => textures(tpk, &out, spec.strings),
        // A TPK has only its textures to give, whichever tab happens to be open — refusing to
        // export one because the hex tab was in front would be pedantry, not fidelity.
        Kind::Model if doc.parts.is_empty() && has_images => textures(tpk, &out, spec.strings),
        Kind::Model => model(doc, tpk, spec.selection, &out, spec.strings),
    }
}

/// One texture as a PNG — what the preview pane is showing.
fn one_texture(
    tpk: Option<&gizmo_nfs::Tpk>,
    out: &Path,
    hash: gizmo_nfs::AssetHash,
) -> Result<Written, String> {
    let tpk = tpk.ok_or("no textures")?;
    let tex = tpk.texture(hash).ok_or("no such texture")?;
    create_dir(out)?;
    let path = out.join(export::png_name(tex));
    let bytes = export::png_bytes(tex).map_err(|e| format!("{}: {e}", path.display()))?;
    write(&path, &bytes)?;
    Ok(Written {
        summary: format!("{} × {} PNG", tex.width, tex.height),
        files: vec![path],
    })
}

/// Every decoded texture in the pack, as PNGs in one folder.
fn textures(tpk: Option<&gizmo_nfs::Tpk>, out: &Path, t: &Strings) -> Result<Written, String> {
    let tpk = tpk.ok_or("no textures")?;
    if tpk.textures.is_empty() {
        return Err("no textures were decoded".into());
    }
    let dir = out.join("tex");
    create_dir(&dir)?;
    let mut files = Vec::new();
    for tex in tpk.textures.values() {
        let path = dir.join(export::png_name(tex));
        let bytes = export::png_bytes(tex).map_err(|e| format!("{}: {e}", path.display()))?;
        write(&path, &bytes)?;
        files.push(path);
    }
    // Say what was left behind: entries the parser could not decode are not written, and a folder
    // with fewer files than the pack has textures should not have to be noticed by counting.
    let undecoded = tpk.entries.len().saturating_sub(tpk.textures.len());
    let mut summary = format!("{} PNG", files.len());
    if undecoded > 0 {
        summary.push_str(&format!(" ({undecoded} {})", t.textures_undecoded));
    }
    files.sort();
    Ok(Written { summary, files })
}

/// The parts the 3D tab would show, as OBJ + MTL + the textures they reference.
fn model(
    doc: &Doc,
    tpk: Option<&gizmo_nfs::Tpk>,
    selection: Option<usize>,
    out: &Path,
    t: &Strings,
) -> Result<Written, String> {
    let parts = shown_parts(doc, selection);
    if parts.is_empty() {
        return Err("this file holds no parts to export".into());
    }
    let stem = stem(doc);
    let mtl_name = format!("{stem}.mtl");

    let plan = MaterialPlan::build(&parts, tpk);
    let obj_text = export::write_obj(&parts, &mtl_name, |p, run| plan.name_for(p, run));
    let mtl_text = export::write_mtl(&plan.materials);

    create_dir(out)?;
    // The `.glb` first: it is the one file someone can drag into a viewer and see the car, images
    // and all. The OBJ beside it is for the older tools around this game.
    let glb_path = out.join(format!("{stem}.glb"));
    let glb = export::write_glb(&parts, tpk).map_err(|e| format!("{}: {e}", glb_path.display()))?;
    write(&glb_path, &glb)?;
    let obj_path = out.join(format!("{stem}.obj"));
    let mtl_path = out.join(&mtl_name);
    write(&obj_path, obj_text.as_bytes())?;
    write(&mtl_path, mtl_text.as_bytes())?;
    let mut files = vec![glb_path, obj_path, mtl_path];

    if let Some(tpk) = tpk {
        let dir = out.join("tex");
        create_dir(&dir)?;
        for hash in &plan.textures {
            if let Some(tex) = tpk.texture(*hash) {
                let path = dir.join(export::png_name(tex));
                let bytes = export::png_bytes(tex).map_err(|e| format!("{}: {e}", path.display()))?;
                write(&path, &bytes)?;
                files.push(path);
            }
        }
    }

    let tris: usize = parts.iter().map(|p| p.triangle_count()).sum();
    Ok(Written {
        summary: format!(
            "GLB + OBJ · {} {} · {tris} {} · {} {} · {} PNG",
            parts.len(),
            t.ex_parts,
            t.ex_triangles,
            plan.materials.len(),
            t.ex_materials,
            files.len().saturating_sub(3)
        ),
        files,
    })
}

/// What the 3D tab is showing: the solid the selection sits in, else the showroom car. Kept in
/// step with `panels::viewport3d` on purpose — an export that wrote something else would make the
/// viewport a lie.
fn shown_parts(doc: &Doc, selection: Option<usize>) -> Vec<&NfsMeshPart> {
    match selection.and_then(|o| doc.solid_of(o)) {
        Some(solid) => {
            let name = solid
                .find(gizmo_nfs::geometry::format::SOLID_HEADER)
                .map(|h| gizmo_nfs::geometry::part_name(h.data(&doc.bytes)))
                .unwrap_or_default();
            doc.parts.iter().filter(|p| p.name == name).collect()
        }
        None => gizmo_nfs::select_stock_car(&doc.parts),
    }
}

/// `pryhub-export/<car>_<file>/` under the working directory. The car folder is in the name
/// because every car's geometry file is called `GEOMETRY.BIN`, and two exports must not land on
/// top of each other.
fn out_dir(doc: &Doc) -> Result<PathBuf, String> {
    let cwd = std::env::current_dir().map_err(|e| format!("working directory: {e}"))?;
    Ok(cwd.join("pryhub-export").join(stem(doc)))
}

fn stem(doc: &Doc) -> String {
    let file = doc.path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let car = doc
        .path
        .parent()
        .and_then(Path::file_name)
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let name = if car.is_empty() { file } else { format!("{car}_{file}") };
    name.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').collect()
}

fn create_dir(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}
