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
    /// Indices into `doc.parts` of what the assembly tab has mounted, resolved when the button was
    /// pressed. Resolved *then* rather than read on the worker: the export is about the build the
    /// user was looking at, and they may go on toggling while it writes.
    pub build: Vec<usize>,
    /// What the dialog was set to when the button was pressed.
    pub choice: Choice,
    /// The interface's language, so the summary line reads in it.
    pub strings: &'static Strings,
    pub textures: Option<Arc<gizmo_nfs::Tpk>>,
    /// How many of the pack's textures were never decoded because of the budget, when `textures` is
    /// one the interface already had. Carried so the summary can say "not read" of them instead of
    /// "could not be decoded", which are different statements and only one of them blames the file.
    pub textures_unread: usize,
}

/// The dialog's answers: how much of the file, and in what.
///
/// It lives on the app rather than being rebuilt per opening, so the dialog reopens where it was
/// left — the design's state is component-level for the same reason.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Choice {
    pub scope: Scope,
    pub model: Model,
}

impl Default for Choice {
    fn default() -> Self {
        // The design's own defaults (`expScope:'sel', expModel:'gltf'`).
        Self { scope: Scope::Selection, model: Model::Gltf }
    }
}

/// How much of the file to write.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The solid the selection sits in — what the 3D tab is showing.
    Selection,
    /// Every part in the file, including the kit and widebody variants the showroom car leaves out.
    All,
    /// What the assembly tab has mounted — the car on screen, with whatever has been taken off it
    /// taken off. The point of building it is to be able to write *that* out.
    Build,
}

/// Which mesh files to write. Both formats come from the parser, so this only decides what is
/// *kept* — there is no second exporter behind either option.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Model {
    /// `.glb` — one file, with the materials and the hierarchy in it.
    Gltf,
    /// `.obj` + `.mtl` — geometry and material names, for the older tools around this game.
    Obj,
    Both,
}

impl Model {
    /// The word the dialog's own button says it is about to write.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Gltf => "GLB",
            Self::Obj => "OBJ",
            Self::Both => "GLB + OBJ",
        }
    }
}

/// The three things the button can mean.
#[derive(Clone, Copy)]
pub enum Kind {
    /// Every decoded texture in the pack.
    Textures,
    /// The model the 3D tab is showing, as glTF + OBJ + its textures.
    Model,
    /// Just the one image the preview pane has up.
    OneTexture(gizmo_nfs::AssetHash),
}

impl Kind {
    /// A word for the diagnostics log.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Kind::Textures => "textures",
            Kind::Model => "model",
            Kind::OneTexture(_) => "one texture",
        }
    }
}

/// Write what the centre area was showing when the button was pressed.
///
/// # Errors
/// Returns a human-readable message when there is nothing to write (no textures, no texture
/// selected, a car whose `GEOMETRY.BIN` holds no parts) or when a write fails.
pub fn run(
    doc: &Doc,
    spec: &ExportSpec,
    tell: &dyn Fn(usize, usize),
) -> Result<Written, String> {
    let out = out_dir(doc)?;
    // Decoding here rather than on the UI thread is the point of this being a job. A pack decoded
    // here carries its own "not read" count; one handed over by the interface carries the count the
    // interface was already showing, so the folder's summary and the contact sheet agree.
    let (decoded, unread) = match &spec.textures {
        Some(tpk) => (Some(Arc::clone(tpk)), spec.textures_unread),
        None => match doc.decode_textures() {
            Some((tpk, unread)) => (Some(Arc::new(tpk)), unread),
            None => (None, 0),
        },
    };
    let tpk = decoded.as_deref();
    let has_images = tpk.is_some_and(|t| !t.textures.is_empty());
    match spec.kind {
        Kind::OneTexture(hash) => one_texture(tpk, &out, hash),
        Kind::Textures => textures(tpk, &out, spec.strings, unread, tell),
        // A TPK has only its textures to give, whichever tab happens to be open — refusing to
        // export one because the hex tab was in front would be pedantry, not fidelity.
        Kind::Model if doc.parts.is_empty() && has_images => {
            textures(tpk, &out, spec.strings, unread, tell)
        }
        Kind::Model => model(doc, tpk, spec, &out, tell),
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
fn textures(
    tpk: Option<&gizmo_nfs::Tpk>,
    out: &Path,
    t: &Strings,
    unread: usize,
    tell: &dyn Fn(usize, usize),
) -> Result<Written, String> {
    let tpk = tpk.ok_or("no textures")?;
    if tpk.textures.is_empty() {
        return Err("no textures were decoded".into());
    }
    let dir = out.join("tex");
    create_dir(&dir)?;
    let mut files = Vec::new();
    let total = tpk.textures.len();
    for (i, tex) in tpk.textures.values().enumerate() {
        tell(i, total);
        let path = dir.join(export::png_name(tex));
        let bytes = export::png_bytes(tex).map_err(|e| format!("{}: {e}", path.display()))?;
        write(&path, &bytes)?;
        files.push(path);
    }
    // Say what was left behind: a folder with fewer files than the pack has textures should not
    // have to be noticed by counting. The two shortfalls are named apart for the same reason the
    // contact sheet names them apart — one is a texture the parser could not read, the other is one
    // this program declined to decode, and calling the second undecodable blames the file for a
    // limit set here.
    let undecoded = tpk.entries.len().saturating_sub(tpk.textures.len()).saturating_sub(unread);
    let mut summary = format!("{} PNG", files.len());
    if undecoded > 0 {
        summary.push_str(&format!(" ({undecoded} {})", t.textures_undecoded));
    }
    if unread > 0 {
        summary.push_str(&format!(" ({unread} {})", t.textures_unread));
    }
    files.sort();
    Ok(Written { summary, files })
}

/// The parts the 3D tab would show, as OBJ + MTL + the textures they reference.
fn model(
    doc: &Doc,
    tpk: Option<&gizmo_nfs::Tpk>,
    spec: &ExportSpec,
    out: &Path,
    tell: &dyn Fn(usize, usize),
) -> Result<Written, String> {
    // The spec whole rather than four of its fields: it grew a third way of choosing parts and the
    // argument list was already at the edge of being read rather than counted.
    let (choice, t) = (spec.choice, spec.strings);
    let parts = shown_parts(doc, spec.selection, choice.scope, &spec.build);
    if parts.is_empty() {
        return Err(t.no_parts.to_owned());
    }
    let stem = stem(doc);
    let mtl_name = format!("{stem}.mtl");

    let plan = MaterialPlan::build(&parts, tpk);

    create_dir(out)?;
    let mut files = Vec::new();
    // The `.glb` first: it is the one file someone can drag into a viewer and see the car, images
    // and all. The OBJ beside it is for the older tools around this game.
    if matches!(choice.model, Model::Gltf | Model::Both) {
        let glb_path = out.join(format!("{stem}.glb"));
        let glb =
            export::write_glb(&parts, tpk).map_err(|e| format!("{}: {e}", glb_path.display()))?;
        write(&glb_path, &glb)?;
        files.push(glb_path);
    }
    if matches!(choice.model, Model::Obj | Model::Both) {
        let obj_text = export::write_obj(&parts, &mtl_name, |p, run| plan.name_for(p, run));
        let mtl_text = export::write_mtl(&plan.materials);
        let obj_path = out.join(format!("{stem}.obj"));
        let mtl_path = out.join(&mtl_name);
        write(&obj_path, obj_text.as_bytes())?;
        write(&mtl_path, mtl_text.as_bytes())?;
        files.push(obj_path);
        files.push(mtl_path);
    }
    let mesh_files = files.len();

    if let Some(tpk) = tpk {
        let dir = out.join("tex");
        create_dir(&dir)?;
        let total = plan.textures.len() + mesh_files; // the mesh files are already written
        tell(mesh_files, total);
        for (i, hash) in plan.textures.iter().enumerate() {
            tell(i + mesh_files, total);
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
            "{} · {} {} · {tris} {} · {} {} · {} PNG",
            choice.model.label(),
            parts.len(),
            t.ex_parts.of(parts.len()),
            t.ex_triangles.of(tris),
            plan.materials.len(),
            t.ex_materials.of(plan.materials.len()),
            files.len().saturating_sub(mesh_files)
        ),
        files,
    })
}

/// Which parts an export covers.
///
/// [`Scope::Selection`] is what the 3D tab is showing — the solid the selection sits in, else the
/// showroom car — and is kept in step with `panels::viewport3d` on purpose, since an export that
/// wrote something else would make the viewport a lie. [`Scope::All`] is the file itself: every kit,
/// widebody and style variant, which is *more* than any one car ever wears at once.
fn shown_parts<'a>(
    doc: &'a Doc,
    selection: Option<usize>,
    scope: Scope,
    build: &[usize],
) -> Vec<&'a NfsMeshPart> {
    if scope == Scope::All {
        return doc.parts.iter().collect();
    }
    if scope == Scope::Build {
        // An index the document no longer has is dropped rather than panicked on: the two were
        // resolved together, but nothing in the type system says so.
        return build.iter().filter_map(|&i| doc.parts.get(i)).collect();
    }
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
///
/// Public to the crate because the dialog shows the path *before* the job runs: a button that says
/// where the files will go is worth more than a log line that says where they went.
pub(crate) fn out_dir(doc: &Doc) -> Result<PathBuf, String> {
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
    log::trace!(target: "export", "{} ({} bytes)", path.display(), bytes.len());
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}
