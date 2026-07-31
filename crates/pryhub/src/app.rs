//! The application: what is open, what is selected, and which screen is showing.
//!
//! The state here is deliberately thin. Everything derived from the file lives in [`Doc`], which
//! is computed once at open; this struct holds only what the user is currently doing. Selection
//! travels as a chunk *offset* — unique per node, and the same key the tree, the hex view and the
//! inspector all look up — so keeping the three in sync is a comparison, not a message.

use crate::doc::{Doc, Level, Note, NoteKind};
use crate::i18n::Lang;
use crate::theme::{self, Density};
use crate::screens;


/// Which screen the top bar has selected.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Screen {
    #[default]
    Welcome,
    Workspace,
    Validation,
    Discovery,
    Diff,
    Dictionary,
    /// The car's CARP parameters. Second in the nav, as the design places it.
    Carp,
}

/// Which tab the centre area shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tab {
    /// The model, through eframe's own wgpu device into an offscreen target.
    ThreeD,
    #[default]
    Hex,
    /// The contact sheet over the open file's pack.
    Texture,
    /// The build: which parts are mounted, and the car they make.
    Assembly,
}

/// A replacement chosen but not yet written.
///
/// Held in the interface rather than applied on the spot, so several of them become one rewrite of
/// the pack — and so a second edit builds on the first rather than starting from the file again.
pub struct Pending {
    pub hash: gizmo_nfs::AssetHash,
    /// What the texture is called on screen, for the list in the save dialog.
    pub name: String,
    pub png: std::path::PathBuf,
}

/// Whether two paths name the same file on disk.
///
/// Resolved rather than compared as written: the two sides come from different places — one from the
/// document, one from a job's spec — and "the same file spelt differently" has to count as the same
/// file, because what hangs on the answer is whether the thing on screen is now stale.
fn same_file(a: &std::path::Path, b: &std::path::Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        // Unresolvable means it is not a file that is there to be the same as anything.
        _ => false,
    }
}

/// What the desktop's file chooser was opened for.
///
/// One chooser at a time and one slot to hold it, so the slot has to say what the answer means. It
/// arrives from another thread several frames after the click that asked for it, and by then there
/// is nothing else left to tell "this is the file to open" from "this is the image to put in".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Picking {
    /// A document, for one side of the compare screen.
    Open(crate::jobs::Side),
    /// An image, to be staged against a particular texture. The hash travels with it because the
    /// answer arrives frames later and the selection may have moved on — staging it against
    /// whatever is selected *then* would put a user's image into a texture they were no longer
    /// looking at when they chose it.
    Image(gizmo_nfs::AssetHash),
}

/// The state of the open file's textures.
///
/// Decoding is a whole pack expanded to RGBA8 — 73 images for a car, 1,786 for its vinyls — so it
/// happens on the worker and the interface has to be able to say "not yet", which is a state and
/// not a `None`.
#[derive(Default)]
pub enum Textures {
    /// Nobody has needed them yet.
    #[default]
    Unasked,
    /// A decode job is in flight.
    Decoding,
    /// Decoded — or decoded to nothing, which is what a file without textures gives.
    ///
    /// `unread` is how many of the pack's entries the byte budget never attempted. It is carried
    /// beside the pack rather than derived from it because the pack cannot tell you: a `Tpk` holds
    /// what was declared and what came back, and "did not decode" and "was not read" subtract to
    /// the same number while meaning opposite things about the file.
    Ready { tpk: Option<std::sync::Arc<gizmo_nfs::Tpk>>, unread: usize },
}

impl Textures {
    /// The pack, if it is decoded and not empty.
    #[must_use]
    pub fn ready(&self) -> Option<&std::sync::Arc<gizmo_nfs::Tpk>> {
        match self {
            Textures::Ready { tpk, .. } => tpk.as_ref(),
            _ => None,
        }
    }

    /// How many of the pack's textures were never attempted, because the budget stopped first.
    #[must_use]
    pub fn unread(&self) -> usize {
        match self {
            Textures::Ready { unread, .. } => *unread,
            _ => 0,
        }
    }
}

/// Which log levels the bottom panel shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LogFilter {
    #[default]
    All,
    Warn,
    Error,
    Info,
}

impl LogFilter {
    /// Whether a note at `level` passes this filter.
    #[must_use]
    pub fn accepts(self, level: Level) -> bool {
        match self {
            Self::All => true,
            Self::Warn => level == Level::Warn,
            Self::Error => level == Level::Error,
            Self::Info => level == Level::Info,
        }
    }
}

/// The whole application.
pub struct PryHub {
    pub screen: Screen,
    pub tab: Tab,
    pub lang: Lang,
    pub density: Density,
    pub log_filter: LogFilter,
    /// The open file, if any. Immutable once parsed and shared by `Arc`, so a background job can
    /// read it while the interface keeps drawing.
    pub doc: Option<std::sync::Arc<Doc>>,
    /// The open file's decoded textures, and whether they have been asked for.
    pub textures: Textures,
    /// The open pack's texture names, read without its pixels. `None` until the dictionary asks.
    pub texture_names: Option<std::sync::Arc<Vec<(gizmo_nfs::AssetHash, String)>>>,
    /// Whether that job has been sent, so a screen drawn every frame does not queue sixty of them.
    names_asked: bool,
    /// What happened *this session* — export results, background failures. The document's own
    /// notes are what parsing found; these are what the user did, and they outlive nothing.
    pub log: Vec<Note>,
    /// The worker thread and its channels.
    pub jobs: crate::jobs::Jobs,
    /// The selected chunk, by header offset.
    pub selection: Option<usize>,
    /// Collapsed containers, by header offset. Absent = expanded, so a freshly opened file shows
    /// its structure rather than a single closed root.
    pub collapsed: std::collections::HashSet<usize>,
    /// Set when the hex view should scroll to the selection (one frame after a tree click).
    pub scroll_hex_to: Option<usize>,
    /// The last open error, shown on the welcome screen.
    pub error: Option<String>,
    /// A selection asked for on the command line, applied when the file finally arrives. Opening is
    /// a job now, so `--select` used to be overwritten by the root the moment the parse landed.
    pub pending_selection: Option<usize>,
    /// The same, for the texture tab, and used for one thing: a file *reopened* because this program
    /// just rewrote it. A reload is a new document and rightly forgets what was selected in the old
    /// one — but the texture the user replaced is the one they are looking at, and putting them back
    /// on the first image of the sheet would be the reload showing.
    pub pending_texture: Option<gizmo_nfs::AssetHash>,
    /// `--stage <png>`: an image to stage against the pack's first texture once it has decoded.
    /// Only a way in for screenshots and tests — staging is otherwise a click and a file chooser.
    pub pending_stage: Option<std::path::PathBuf>,
    /// Files opened this session, most recent first.
    pub recents: Vec<std::path::PathBuf>,
    /// The welcome screen's path field.
    pub path_input: String,
    /// A desktop file chooser waiting to be answered, and which side asked for it. It runs in its
    /// own thread — the chooser does not return until the user has decided, and the interface may
    /// not stop drawing meanwhile. One slot, because two choosers at once is not a thing anyone
    /// means to open.
    pub picking: Option<(Picking, std::sync::mpsc::Receiver<Option<std::path::PathBuf>>)>,
    /// The selected chunk's parsed model, keyed by its offset so it is rebuilt only on a change.
    model: Option<(usize, gizmo_nfs::inspect::ChunkModel)>,
    /// eframe's wgpu device, for the 3D tab. `None` when the backend is not wgpu, in which case
    /// the tab says so instead of the app refusing to run.
    pub render_state: Option<eframe::egui_wgpu::RenderState>,
    /// The project's mark, decoded once at startup. `None` if the PNG would not decode, which
    /// costs the interface a picture and nothing else.
    pub logo: Option<crate::logo::Logo>,
    /// The preview renderer, built lazily the first time the tab is opened.
    pub preview: Option<crate::gpu::preview::Preview>,
    /// Where the preview camera is looking from.
    pub camera: crate::panels::viewport3d::Camera,
    /// Whether the 3D tab draws edges instead of surfaces — the design's `Tel kafes` / `Wireframe`.
    pub wire: bool,
    /// The game's paint palette, read from `GLOBALB.BUN` on first need. `None` until asked for.
    pub palette: Option<Vec<gizmo_nfs::Colour>>,
    /// Every car record in the install's bundle. `None` = not asked yet; `Some` of an empty list =
    /// asked, and there was no install to read. The CARP screen needs both to tell the user which,
    /// and the whole list because it offers a car picker over it.
    pub cars: Option<std::sync::Arc<Vec<gizmo_nfs::CarTypeInfo>>>,
    /// Which record the open file *is*, if it is one. Only decides which car the screen opens on.
    pub car_opened: Option<String>,
    /// The bundle that record came out of — the file a Save would write. `None` until the job has
    /// landed, or when there is no install to have read one from.
    pub car_bundle: Option<std::path::PathBuf>,
    /// Which CARP section is selected, which upgrade level is highlighted, and every number the
    /// user has changed but not yet written.
    pub carp: crate::screens::carp::State,
    /// What the last handling save did, or why it did not. Shown in the CARP header rather than
    /// only in the log — a Save whose only feedback is a line in another panel reads as one that
    /// did nothing.
    pub carp_saved: Option<Result<crate::tune::Done, String>>,
    /// The colour the body is painted, or `None` for the material group's own.
    pub paint: Option<gizmo_nfs::Colour>,
    /// Parts the assembly tab has switched **off**, by display key. Off rather than on, so a file
    /// that has just been opened is fully built without anyone having to enumerate it first.
    pub unmounted: std::collections::HashSet<String>,
    /// Names the user has given to asset hashes, loaded from disk at startup.
    pub names: crate::names::Names,
    /// The dictionary screen's filter and in-progress edits.
    pub dict: crate::screens::dictionary::State,
    /// The compare screen's other file, and what comparing it said.
    pub diff: crate::screens::diff::State,
    /// The discovery screen's schema, and the chunk it was made for.
    pub discover: crate::screens::discovery::State,
    /// The texture the texture tab is showing.
    pub texture_selection: Option<gizmo_nfs::AssetHash>,
    /// Uploaded texture handles, keyed by hash and by thumbnail-or-full-image.
    pub texture_cache: std::collections::HashMap<(u32, bool), egui::TextureHandle>,
    /// Whether the export dialog is up, and what it was last set to. The choice outlives the
    /// dialog on purpose: someone who exports twice in a session almost always means the same
    /// thing the second time.
    pub show_export: bool,
    pub export_choice: crate::export::Choice,
    /// Replacements chosen but not yet written, in the order they were staged.
    ///
    /// The whole reason they are held rather than written on the spot: a write reads the pack from
    /// disk, so two edits written one at a time into a *copy* each start from the original and the
    /// second discards the first. Held here they go into one rewrite, which is also one file
    /// operation instead of N over an 8.7 MB pack. It is the design's own CARP shape — edit into a
    /// set, see what is dirty, Save or Revert — applied to the tab that needed it.
    pub pending: Vec<Pending>,
    /// Whether the save dialog is up, and whether it has been told to write over the game's own
    /// file. The overwrite flag does not outlive the dialog: someone who overwrote once has not
    /// thereby asked to do it again.
    pub show_replace: bool,
    pub replace_over: bool,
    /// Set when the density or language changed and the style must be rebuilt.
    pub(crate) restyle: bool,
    /// `--shot <path>`: draw a few frames, save the window as a PNG, and exit. The tool renders
    /// through a GPU surface, so this is the only way to check the interface on a machine whose
    /// compositor will not hand out a screen grab — and it doubles as a way to keep a visual
    /// record of the design port.
    pub(crate) shot: Option<crate::shot::Shot>,
}


impl PryHub {
    /// Build the app, optionally opening a file and/or saving a screenshot.
    #[must_use]
    pub fn new(
        ctx: &egui::Context,
        chosen: crate::settings::Settings,
        open: Option<String>,
        shot: Option<String>,
        screen: Option<String>,
    ) -> Self {
        let mut app = Self {
            screen: Screen::Welcome,
            tab: Tab::default(),
            lang: chosen.lang,
            density: chosen.density,
            log_filter: LogFilter::default(),
            doc: None,
            textures: Textures::default(),
            texture_names: None,
            names_asked: false,
            log: Vec::new(),
            jobs: crate::jobs::Jobs::start(ctx.clone()),
            selection: None,
            collapsed: std::collections::HashSet::new(),
            scroll_hex_to: None,
            error: None,
            pending_selection: None,
            pending_texture: None,
            pending_stage: None,
            recents: Vec::new(),
            path_input: String::new(),
            picking: None,
            model: None,
            render_state: None,
            logo: crate::logo::Logo::load(ctx),
            preview: None,
            camera: crate::panels::viewport3d::Camera::default(),
            wire: false,
            palette: None,
            cars: None,
            car_opened: None,
            car_bundle: None,
            carp: crate::screens::carp::State::default(),
            carp_saved: None,
            paint: None,
            unmounted: std::collections::HashSet::new(),
            names: crate::names::Names::load(),
            dict: crate::screens::dictionary::State::default(),
            diff: crate::screens::diff::State::default(),
            discover: crate::screens::discovery::State::default(),
            texture_selection: None,
            texture_cache: std::collections::HashMap::new(),
            show_export: false,
            export_choice: crate::export::Choice::default(),
            pending: Vec::new(),
            show_replace: false,
            replace_over: false,
            restyle: false,
            shot: shot.map(|p| crate::shot::Shot {
                path: p.into(),
                warmup: 4,
                settled_at: None,
                asked: false,
            }),
        };
        if let Some(path) = open {
            app.open(std::path::Path::new(&path));
        }
        // `--screen validation` opens straight there; without it a screenshot could only ever
        // capture the workspace.
        if let Some(name) = screen {
            app.screen = match name.as_str() {
                "welcome" => Screen::Welcome,
                "validation" => Screen::Validation,
                "discovery" => Screen::Discovery,
                "diff" => Screen::Diff,
                "dictionary" => Screen::Dictionary,
                "carp" => Screen::Carp,
                // The dialog is not a screen, but it is a *view* — and it is the one thing in the
                // interface a screenshot could otherwise never reach.
                "export" => {
                    app.show_export = true;
                    Screen::Workspace
                }
                // The same, for the other dialog. It sits over the texture tab because that is
                // where it is reached from and what it is about — a screenshot of it over the hex
                // view would be a picture of a state the program cannot be in.
                "replace" => {
                    app.show_replace = true;
                    app.tab = Tab::Texture;
                    Screen::Workspace
                }
                _ => Screen::Workspace,
            };
        }
        app
    }

    /// Ask for a file to be opened. Returns immediately; the parse happens on the worker and lands
    /// in [`PryHub::collect_jobs`] a frame or two later.
    pub fn open(&mut self, path: &std::path::Path) {
        self.open_side(path, crate::jobs::Side::Main);
    }

    /// The same for the compare screen's other file.
    pub fn open_other(&mut self, path: &std::path::Path) {
        self.open_side(path, crate::jobs::Side::Other);
    }

    pub(crate) fn open_side(&mut self, path: &std::path::Path, side: crate::jobs::Side) {
        self.jobs.send(crate::jobs::Request::Open { path: path.to_path_buf(), side });
    }

    /// Take everything the worker has finished.
    fn collect_jobs(&mut self) {
        use crate::jobs::{Outcome, Side};
        // The file chooser, if one is open. `try_recv` rather than a wait: it is a whole desktop
        // window away and may never be answered at all.
        if let Some((what, rx)) = &self.picking {
            let what = *what;
            match rx.try_recv() {
                Ok(Some(path)) => {
                    self.picking = None;
                    match what {
                        Picking::Open(side) => self.open_side(&path, side),
                        // Staged, not written: the target — a copy, or the game's own file — is
                        // still to be chosen, and is chosen once for the whole set.
                        Picking::Image(hash) => self.stage_replacement(hash, &path),
                    }
                }
                // Cancelled, or the thread went away with it.
                Ok(None) | Err(std::sync::mpsc::TryRecvError::Disconnected) => self.picking = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        for outcome in self.jobs.poll() {
            match outcome {
                Outcome::Opened { result, side: Side::Main, path } => match *result {
                    Ok(doc) => self.adopt(doc, &path),
                    Err(e) => self.error = Some(e),
                },
                Outcome::Opened { result, side: Side::Other, path } => {
                    self.diff.adopt(*result, &path);
                }
                // A decode that finished for a file the user has since replaced is dropped: it is
                // the right answer to the wrong question.
                Outcome::Decoded { for_path, tpk, unread } => {
                    if self.doc.as_ref().is_some_and(|d| d.path == for_path) {
                        self.textures = Textures::Ready { tpk, unread };
                        // `--stage` waits for exactly this: the dimensions to check against.
                        if let Some(png) = self.pending_stage.take() {
                            // By name, then hash — the sheet's own order, so `--stage` acts on the
                            // texture the window opens showing rather than on whichever one happens
                            // to sort first by hash.
                            if let Some(hash) = self.textures.ready().and_then(|p| {
                                p.textures
                                    .values()
                                    .min_by(|a, b| a.name.cmp(&b.name).then(a.hash.0.cmp(&b.hash.0)))
                                    .map(|t| t.hash)
                            }) {
                                self.texture_selection = Some(hash);
                                self.stage_replacement(hash, &png);
                            }
                        }
                    }
                }
                Outcome::TextureNames { for_path, names } => {
                    if self.doc.as_ref().is_some_and(|d| d.path == for_path) {
                        self.texture_names = Some(names);
                    }
                }
                Outcome::Palette(colours) => self.palette = Some(colours),
                Outcome::CarSpec { cars, bundle, opened } => {
                    self.cars = Some(cars);
                    self.car_bundle = bundle;
                    self.car_opened = opened;
                }
                Outcome::Exported(result) => self.report_export(result),
                // A write that landed for a file the user has since closed is reported anyway — it
                // happened, and the log is what happened — but only the document it was for gets
                // its pack re-read and its thumbnails dropped.
                Outcome::Replaced { for_path, result } => {
                    let mine = self.doc.as_ref().is_some_and(|d| d.path == for_path);
                    self.report_replace(*result, mine);
                }
                Outcome::Tuned(result) => self.report_tune(*result),
                // `poll` keeps progress to itself; this arm exists so the compiler says something
                // if that ever changes.
                Outcome::Progress { .. } => {}
                Outcome::Failed(message) => self.log.push(Note {
                    level: Level::Error,
                    chunk: None,
                    chunk_id: String::new(),
                    kind: NoteKind::Diagnostic(message),
                }),
            }
        }
    }

    /// Make a freshly parsed document the open one.
    fn adopt(&mut self, doc: Doc, path: &std::path::Path) {
        log::info!(
            target: "doc",
            "{}: {} bytes, {} chunks, {} parts, {} notes",
            path.display(),
            doc.bytes.len(),
            doc.rows.len(),
            doc.parts.len(),
            doc.notes.len()
        );
        self.selection = self.pending_selection.take().or_else(|| doc.rows.first().map(|r| r.offset));
        self.collapsed.clear();
        self.error = None;
        self.recents.retain(|p| p != path);
        self.recents.insert(0, path.to_path_buf());
        self.recents.truncate(6);
        self.doc = Some(std::sync::Arc::new(doc));
        self.model = None;
        self.textures = Textures::Unasked;
        self.texture_names = None;
        self.names_asked = false;
        self.texture_selection = self.pending_texture.take();
        self.texture_cache.clear();
        // Everything below describes the file that was open, in terms that mean something different
        // in the one that just arrived. A chunk offset, a stride, a mesh key: all of them are still
        // *valid numbers* against the new bytes, which is exactly why none of them announce
        // themselves as stale — the tab simply keeps drawing the previous car and the table keeps
        // reading the new payload through the old layout.
        self.discover = crate::screens::discovery::State::default();
        self.unmounted.clear();
        self.paint = None;
        // The palette belongs to the *install*, not to the file, so it is kept across an open.
        if let Some(preview) = &mut self.preview {
            preview.forget_mesh();
        }
        self.camera.framed = None;
        // Only take the user to the workspace if they had nowhere else to be. Opening used to be
        // synchronous, so this ran before the command line could pick a screen and before anyone
        // could navigate; as a job it lands later, and forcing the workspace then overrode both.
        if self.screen == Screen::Welcome {
            self.screen = Screen::Workspace;
        }
    }

    /// Ask for the paint palette, unless it is here or already coming.
    pub fn want_palette(&mut self) {
        if self.palette.is_some() {
            return;
        }
        if let Some(doc) = &self.doc {
            // Marked as asked by answering it empty now; the job overwrites that when it lands.
            self.palette = Some(Vec::new());
            self.jobs.send(crate::jobs::Request::Palette { beside: doc.path.clone() });
        }
    }

    /// Ask for the install's car records, unless they are here or already coming.
    pub fn want_car_spec(&mut self) {
        if self.cars.is_some() {
            return;
        }
        if let Some(doc) = &self.doc {
            // Marked as asked by answering it empty now; the job overwrites that when it lands.
            self.cars = Some(std::sync::Arc::new(Vec::new()));
            self.jobs.send(crate::jobs::Request::CarSpec { beside: doc.path.clone() });
        }
    }

    /// Ask for the open file's textures, unless they are already coming.
    ///
    /// Called from panels that need them, every frame — hence the guard: a request per frame would
    /// queue sixty decodes a second.
    pub fn want_textures(&mut self) {
        if matches!(self.textures, Textures::Unasked) {
            if let Some(doc) = &self.doc {
                self.textures = Textures::Decoding;
                self.jobs.send(crate::jobs::Request::Decode(std::sync::Arc::clone(doc)));
            }
        }
    }

    /// Ask for the open file's texture *names*, unless they are already coming.
    ///
    /// The dictionary's counterpart to [`Self::want_textures`], and a different job on purpose: it
    /// wants what the textures are called, which costs a decompression each, and not what they look
    /// like, which costs the budget's worth of RGBA8 on top. Asking the decode job for it made
    /// opening the dictionary allocate 256 MB to read some strings.
    pub fn want_texture_names(&mut self) {
        if self.texture_names.is_none() && !self.names_asked {
            if let Some(doc) = &self.doc {
                self.names_asked = true;
                self.jobs.send(crate::jobs::Request::TextureNames(std::sync::Arc::clone(doc)));
            }
        }
    }

    /// Queue an export of what the centre area is showing.
    pub fn export_now(&mut self, kind: crate::export::Kind) {
        let Some(doc) = &self.doc else { return };
        let build = crate::panels::assembly::mounted_indices(self, &doc.parts);
        let spec = crate::export::ExportSpec {
            kind,
            selection: self.selection,
            build,
            choice: self.export_choice,
            strings: self.lang.strings(),
            textures: self.textures.ready().map(std::sync::Arc::clone),
            textures_unread: self.textures.unread(),
        };
        self.jobs.send(crate::jobs::Request::Export { doc: std::sync::Arc::clone(doc), spec });
    }

    /// Where the welcome screen's path field starts: the game root, when it is set.
    #[must_use]
    pub fn suggested_dir() -> String {
        std::env::var("NFSU2_ROOT").map(|r| format!("{r}/CARS/")).unwrap_or_default()
    }

    /// Ask the desktop for a file to *open*, for `side`. Does nothing if a chooser is already up, or
    /// if this machine has none — in which case the caller's own path field remains the way in.
    pub(crate) fn ask_for_file(&mut self, ctx: &egui::Context, side: crate::jobs::Side) -> bool {
        self.ask(ctx, Picking::Open(side), crate::picker::Filter::Assets)
    }

    /// Ask for an image to stage against `hash`. The same chooser and a different answer: this one
    /// stages a replacement rather than opening a document.
    pub(crate) fn ask_for_image(
        &mut self,
        ctx: &egui::Context,
        hash: gizmo_nfs::AssetHash,
    ) -> bool {
        self.ask(ctx, Picking::Image(hash), crate::picker::Filter::Images)
    }

    /// Stage an image against a texture, or say why it cannot go there.
    ///
    /// The dimension check happens **here**, when the file is chosen, and it reads the PNG's header
    /// rather than its pixels — `png_size` is the first twenty-four bytes and two integers. Decoding
    /// a 512² image to compare two numbers would put the cost of the whole replacement on the frame
    /// that opened a file chooser, and leaving the check to the save would let a user stage six
    /// images and find out at the end that the second one was never going to fit.
    fn stage_replacement(&mut self, hash: gizmo_nfs::AssetHash, png: &std::path::Path) {
        let Some(tex) = self.textures.ready().and_then(|p| p.texture(hash).cloned()) else { return };
        let complain = |app: &mut Self, error: String| {
            app.log.push(Note {
                level: Level::Error,
                chunk: None,
                chunk_id: String::new(),
                kind: NoteKind::ReplaceFailed { error },
            });
        };
        let bytes = match std::fs::read(png) {
            Ok(b) => b,
            Err(e) => return complain(self, format!("{}: {e}", png.display())),
        };
        match gizmo_nfs::export::png_size(&bytes) {
            Ok((w, h)) if (w, h) == (tex.width, tex.height) => {}
            Ok((w, h)) => {
                let name = png.file_name().unwrap_or_default().to_string_lossy().to_string();
                return complain(
                    self,
                    format!("{name} is {w}×{h}, {} is {}×{}", tex.name, tex.width, tex.height),
                );
            }
            Err(e) => return complain(self, format!("{}: {e}", png.display())),
        }
        // One image per texture: staging a second for the same one replaces it rather than queueing
        // both, because two answers to one question is not a set anyone meant to build.
        self.pending.retain(|p| p.hash != hash);
        self.pending.push(Pending { hash, name: tex.name.clone(), png: png.to_path_buf() });
    }

    /// One chooser at a time, and it remembers what it was opened for.
    ///
    /// The purpose has to travel with it because the answer arrives frames later, from another
    /// thread, with nothing else to say what it is. It used to be a `Side` and every answer went to
    /// `open_side` — which was right while the only question was "which file shall I open", and
    /// would have quietly tried to parse a PNG as a document the moment it was not.
    fn ask(&mut self, ctx: &egui::Context, what: Picking, filter: crate::picker::Filter) -> bool {
        if self.picking.is_some() {
            return true;
        }
        let start = self.start_dir();
        match crate::picker::open(ctx, start, filter) {
            Some(rx) => {
                self.picking = Some((what, rx));
                true
            }
            None => false,
        }
    }

    /// Where a file chooser should open: beside the file that is already open, else the install if
    /// `NFSU2_ROOT` names one. Somewhere useful beats the process's working directory.
    #[must_use]
    pub fn start_dir(&self) -> Option<std::path::PathBuf> {
        if let Some(doc) = &self.doc {
            if let Some(dir) = doc.path.parent() {
                return Some(dir.to_path_buf());
            }
        }
        let root = std::env::var("NFSU2_ROOT").ok()?;
        let cars = std::path::PathBuf::from(root).join("CARS");
        cars.is_dir().then_some(cars)
    }

    /// Select a chunk and make the hex view follow it.
    pub fn select(&mut self, offset: usize) {
        self.selection = Some(offset);
        self.scroll_hex_to = Some(offset);
        self.model = None; // rebuilt on the next frame, for the new selection
    }

    /// The selected chunk.
    #[must_use]
    pub fn selected_node(&self) -> Option<&gizmo_nfs::chunk::ChunkNode> {
        self.doc.as_ref()?.node_at(self.selection?)
    }

    /// The name of the solid the selection sits in, when it has one.
    #[must_use]
    pub fn selected_solid_name(&self) -> Option<String> {
        let doc = self.doc.as_ref()?;
        let solid = doc.solid_of(self.selection?)?;
        let header = solid.find(gizmo_nfs::geometry::format::SOLID_HEADER)?;
        let name = gizmo_nfs::geometry::part_name(header.data(&doc.bytes));
        (!name.is_empty()).then_some(name)
    }

    /// The parsed model of the selected chunk, built once per selection rather than per frame.
    #[must_use]
    pub fn selected_model(&self) -> Option<&gizmo_nfs::inspect::ChunkModel> {
        self.model.as_ref().filter(|(off, _)| Some(*off) == self.selection).map(|(_, m)| m)
    }

    /// Build the model for the current selection if it is missing. Called once a frame, before
    /// the panels draw, so the inspector and the hex view read the same one.
    pub fn refresh_model(&mut self) {
        let Some(offset) = self.selection else {
            self.model = None;
            return;
        };
        if self.model.as_ref().is_some_and(|(o, _)| *o == offset) {
            return;
        }
        self.model = self.doc.as_ref().and_then(|doc| {
            let node = doc.node_at(offset)?;
            let solid = doc.solid_of(offset);
            Some((offset, gizmo_nfs::inspect::model(node, solid, &doc.bytes)))
        });
    }
}

impl eframe::App for PryHub {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let frame_start = std::time::Instant::now();
        if self.restyle {
            theme::apply(ui.ctx(), self.density);
            self.restyle = false;
        }
        self.collect_jobs();
        self.refresh_model();
        let after_jobs = frame_start.elapsed();
        self.top_bar(ui);
        self.status_bar(ui);
        let after_bars = frame_start.elapsed();
        match self.screen {
            Screen::Welcome => screens::welcome::show(self, ui),
            Screen::Workspace => screens::workspace::show(self, ui),
            Screen::Validation => {
                // A finding names a chunk; clicking it goes there, which is the point of a
                // validation screen that sits beside a browser.
                if let Some(offset) = screens::validation::show(self, ui) {
                    self.select(offset);
                    self.screen = Screen::Workspace;
                }
            }
            Screen::Discovery => {
                // The tree is on this screen too, so a chunk can be chosen without leaving it.
                if let Some(offset) = screens::discovery::show(self, ui) {
                    self.select(offset);
                }
            }
            Screen::Diff => {
                // A row names a chunk in the *left* file; clicking it goes there.
                if let Some(offset) = screens::diff::show(self, ui) {
                    self.select(offset);
                    self.screen = Screen::Workspace;
                }
            }
            Screen::Dictionary => screens::dictionary::show(self, ui),
            Screen::Carp => screens::carp::show(self, ui),
        }
        // Last, and over whatever the screen drew: the dialog is modal, and it opens the workspace
        // under itself so it is always over the thing it is about to export.
        screens::export_dialog::show(self, ui.ctx());
        screens::replace_dialog::show(self, ui.ctx());
        // A file dropped on the window opens it — the welcome screen's drop target, everywhere.
        // On the compare screen it loads the other side instead, which is what dropping a second
        // file onto a comparison plainly means.
        let dropped = ui.ctx().input(|i| i.raw.dropped_files.clone());
        if let Some(path) = dropped.into_iter().find_map(|f| f.path) {
            if screens::diff::accepts_drop(self) {
                self.open_other(&path);
            } else {
                self.open(&path);
            }
        }
        self.screenshot(ui.ctx());
        // `PRYHUB_LOG=frame=trace`. Behind a level check rather than an env var of its own: the
        // check is a load of an atomic, and one env var for every kind of diagnostic does not scale.
        // "The interface feels slow" is not something to fix by guessing — this is what said the
        // tree panel was drawing all 7,246 rows every frame.
        if log::log_enabled!(target: "frame", log::Level::Trace) {
            let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.0;
            log::trace!(
                target: "frame",
                "{:.2} ms · jobs {:.2} · bars {:.2} · screen {:.2}",
                ms(frame_start.elapsed()),
                ms(after_jobs),
                ms(after_bars - after_jobs),
                ms(frame_start.elapsed() - after_bars),
            );
        }
    }
}

impl PryHub {
    /// Put an export's result in the log — the design's place for "işlem çıktıları", and the only
    /// place the written paths are stated. A failure is a log line too, not a modal: the file is
    /// still open and the user has lost nothing.
    pub fn report_export(&mut self, result: Result<crate::export::Written, String>) {
        let note = match result {
            Ok(w) => {
                let where_to = w
                    .files
                    .first()
                    .and_then(|p| p.parent())
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                Note {
                    level: Level::Info,
                    chunk: None,
                    chunk_id: String::new(),
                    kind: NoteKind::Exported { summary: w.summary, into: where_to },
                }
            }
            Err(e) => Note {
                level: Level::Error,
                chunk: None,
                chunk_id: String::new(),
                kind: NoteKind::ExportFailed { error: e },
            },
        };
        self.log.push(note);
    }

    /// Queue every staged replacement as one write.
    ///
    /// Everything it needs is resolved **here**, on the click: which pack, which textures, which
    /// files, and whether to write over the original. The worker is handed a decision rather than a
    /// question, for the same reason the export snapshots the mounted build — the user is free to go
    /// on staging while it runs, and what they pressed the button on is what should be written.
    pub fn replace_now(&mut self) {
        let Some(doc) = &self.doc else { return };
        let Some(pack) = doc.pack_path() else { return };
        if self.pending.is_empty() {
            return;
        }
        let edits = self
            .pending
            .iter()
            .map(|p| crate::replace::Edit {
                hash: p.hash,
                name: p.name.clone(),
                png: p.png.clone(),
            })
            .collect();
        let spec =
            crate::replace::Spec { doc: doc.path.clone(), pack, edits, over: self.replace_over };
        self.jobs.send(crate::jobs::Request::Replace(Box::new(spec)));
    }

    /// Show a write that landed, by reloading exactly as much as it invalidated.
    ///
    /// There are two cases and they are not the same, which is the thing this got wrong first time
    /// round. When the pack is the file *beside* the open one — a `GEOMETRY.BIN` drawn with the
    /// `TEXTURES.BIN` next to it — dropping the decoded pack is enough, because the decode re-reads
    /// that file from disk. When the pack **is** the open document, it is not: `Doc::bytes` is the
    /// snapshot taken at open and `decode_textures` reads it rather than the disk, so asking for the
    /// textures again returns the pre-write image — and the interface would redraw the old pixels
    /// under a log line saying it had written new ones, which is precisely the lie this is here to
    /// avoid. It is also broader than the pack: the chunk tree, the hex view and the validation
    /// report all describe bytes that have just changed. So the document is reopened, carrying the
    /// selection and the replaced texture across so the reload does not show.
    fn refresh_after(&mut self, done: &crate::replace::Done) {
        let is_open = self.doc.as_ref().is_some_and(|d| same_file(&d.path, &done.into));
        if is_open {
            self.pending_selection = self.selection;
            self.pending_texture = Some(done.hash);
            let path = done.into.clone();
            self.open(&path);
        } else {
            self.textures = Textures::Unasked;
            self.texture_cache.clear();
        }
    }

    /// Take a replacement's result: say what happened, and show it.
    ///
    /// `mine` is whether the document it was computed for is still the open one.
    pub fn report_replace(&mut self, result: Result<crate::replace::Done, String>, mine: bool) {
        let note = match result {
            Ok(done) => {
                // Staged edits are cleared on success and only on success: a set that could not be
                // written is still the set the user built, and throwing it away would make them
                // choose six files again to find out whether the second one fits this time.
                self.pending.clear();
                if mine {
                    self.refresh_after(&done);
                }
                if let Some(bak) = &done.backup {
                    log::info!(target: "jobs", "backed up to {}", bak.display());
                }
                Note {
                    level: Level::Info,
                    chunk: None,
                    chunk_id: String::new(),
                    kind: NoteKind::Replaced {
                        count: done.count,
                        into: done.into.display().to_string(),
                        moved: done.moved,
                        psnr: done.psnr,
                    },
                }
            }
            Err(e) => Note {
                level: Level::Error,
                chunk: None,
                chunk_id: String::new(),
                kind: NoteKind::ReplaceFailed { error: e },
            },
        };
        self.log.push(note);
    }

    /// Send the CARP screen's pending edits to the worker.
    ///
    /// The car is the *record's* own name rather than the directory's, so a save cannot land on a
    /// neighbouring car because someone renamed a folder. Nothing is cleared here: the edits stay
    /// pending until the write comes back saying it happened, for the same reason the texture set
    /// does — a set that could not be written is still the set the user built.
    pub fn save_handling(&mut self) {
        let Some(doc) = &self.doc else { return };
        let Some(car) = self.carp.selected().map(str::to_owned) else { return };
        let edits = self.carp.pending();
        if edits.is_empty() {
            return;
        }
        self.carp_saved = None;
        self.jobs.send(crate::jobs::Request::Tune(Box::new(crate::tune::Spec {
            beside: doc.path.clone(),
            car,
            edits,
        })));
    }

    /// Take in what a save did.
    ///
    /// On success the record is **re-read from disk** rather than assumed: the numbers on screen
    /// then come from the file the game will open, so a lane that did not go in the way it was meant
    /// to shows here instead of in the car.
    fn report_tune(&mut self, result: Result<crate::tune::Done, String>) {
        let note = match &result {
            Ok(done) => {
                self.carp.clear_edits();
                self.cars = None; // re-read on the next frame the screen draws
                for bak in &done.backups {
                    log::info!(target: "jobs", "backed up to {}", bak.display());
                }
                format!(
                    "{}: {} lane(s) written to {}",
                    done.car,
                    done.changed,
                    done.files.iter().map(|f| f.display().to_string()).collect::<Vec<_>>().join(", ")
                )
            }
            Err(e) => format!("handling not written: {e}"),
        };
        self.log.push(Note {
            level: if result.is_ok() { Level::Info } else { Level::Error },
            chunk: None,
            chunk_id: String::new(),
            kind: NoteKind::Diagnostic(note),
        });
        self.carp_saved = Some(result);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_filter_matches_the_designs_four_buttons() {
        assert!(LogFilter::All.accepts(Level::Info) && LogFilter::All.accepts(Level::Error));
        assert!(LogFilter::Warn.accepts(Level::Warn) && !LogFilter::Warn.accepts(Level::Info));
        assert!(LogFilter::Error.accepts(Level::Error) && !LogFilter::Error.accepts(Level::Warn));
    }

    /// An app with a live worker, for tests that need one.
    fn app() -> PryHub {
        PryHub::new(&egui::Context::default(), crate::settings::Settings::default(), None, None, None)
    }

    /// Drive the frame loop's job collection until `done`, or give up. The worker is a thread, so a
    /// test has to wait for it — but never forever, or a broken channel becomes a hung suite.
    fn settle(app: &mut PryHub, done: impl Fn(&PryHub) -> bool) -> bool {
        for _ in 0..500 {
            app.collect_jobs();
            if done(app) {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        false
    }

    /// Staging: the dimension check, and one image per texture.
    ///
    /// Both rules exist because of what happens without them. The check is here rather than at Save
    /// so a person who stages six images is not told at the end that the second one was never going
    /// to fit; and staging a second image for one texture *replaces* the first, because two answers
    /// to one question is not a set anybody meant to build — and `replace_images` refuses it anyway.
    #[test]
    fn staging_checks_the_size_and_keeps_one_image_per_texture() {
        let Some(root) = std::env::var_os("NFSU2_ROOT").map(std::path::PathBuf::from) else {
            eprintln!("NFSU2_ROOT unset — skipping");
            return;
        };
        let mut app = app();
        app.open(&root.join("CARS/240SX/TEXTURES.BIN"));
        assert!(settle(&mut app, |a| a.doc.is_some()), "the file never opened");
        app.want_textures();
        assert!(settle(&mut app, |a| a.textures.ready().is_some()), "the pack never decoded");

        let pack = app.textures.ready().expect("a pack").clone();
        let mut texs: Vec<_> = pack.textures.values().collect();
        texs.sort_by_key(|t| t.hash.0);
        let tex = texs.first().expect("a texture");
        let other = texs.iter().find(|t| (t.width, t.height) != (tex.width, tex.height));

        let dir = std::env::temp_dir().join(format!("pryhub-stage-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let right = dir.join("right.png");
        std::fs::write(&right, gizmo_nfs::export::png_bytes(tex).expect("encode")).expect("write");

        app.stage_replacement(tex.hash, &right);
        assert_eq!(app.pending.len(), 1, "a matching image stages");

        // A second image for the same texture replaces it rather than queueing beside it.
        let again = dir.join("again.png");
        std::fs::write(&again, gizmo_nfs::export::png_bytes(tex).expect("encode")).expect("write");
        app.stage_replacement(tex.hash, &again);
        assert_eq!(app.pending.len(), 1, "one image per texture");
        assert_eq!(app.pending[0].png, again, "and it is the newer one");

        // A differently-sized image is refused, said out loud, and staged nowhere.
        if let Some(wrong_size) = other {
            let wrong = dir.join("wrong.png");
            std::fs::write(&wrong, gizmo_nfs::export::png_bytes(wrong_size).expect("encode"))
                .expect("write");
            let before = app.log.len();
            app.stage_replacement(tex.hash, &wrong);
            assert_eq!(app.pending.len(), 1, "nothing was staged for the wrong size");
            assert!(app.log.len() > before, "and the user was told");
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn selecting_asks_the_hex_view_to_follow() {
        let mut app = app();
        app.select(0x1B8);
        assert_eq!(app.selection, Some(0x1B8));
        assert_eq!(app.scroll_hex_to, Some(0x1B8), "a tree click must pull the hex view along");
    }

    /// Opening is a job now, so the failure arrives through the channel rather than from the call.
    /// What must not change: the user is told, and is not stranded on an empty workspace.
    #[test]
    fn a_failed_open_reports_rather_than_clearing_the_current_file() {
        let mut app = app();
        app.open(std::path::Path::new("/nonexistent/GEOMETRY.BIN"));
        assert!(settle(&mut app, |a| a.error.is_some()), "the worker never reported the failure");
        assert!(app.doc.is_none());
        assert_eq!(app.screen, Screen::Welcome, "a failure must not strand the user on an empty workspace");
    }

    /// The interface must not block on the request itself — that is the whole point of the layer.
    #[test]
    fn asking_to_open_returns_immediately_and_says_it_is_working() {
        let mut app = app();
        let started = std::time::Instant::now();
        app.open(std::path::Path::new("/nonexistent/GEOMETRY.BIN"));
        assert!(
            started.elapsed() < std::time::Duration::from_millis(50),
            "queueing a job took {:?}",
            started.elapsed()
        );
        assert_eq!(app.jobs.busy(), Some(crate::jobs::Kind::Open), "the status bar has something to say");
        assert!(settle(&mut app, |a| a.jobs.busy().is_none()));
    }
}
