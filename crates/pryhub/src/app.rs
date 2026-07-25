//! The application: what is open, what is selected, and which screen is showing.
//!
//! The state here is deliberately thin. Everything derived from the file lives in [`Doc`], which
//! is computed once at open; this struct holds only what the user is currently doing. Selection
//! travels as a chunk *offset* — unique per node, and the same key the tree, the hex view and the
//! inspector all look up — so keeping the three in sync is a comparison, not a message.

use crate::doc::{Doc, Level, Note};
use crate::i18n::Lang;
use crate::theme::{self, token, Density};
use crate::screens;
use egui::{Align, Layout, RichText};

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
}

/// Which tab the centre area shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tab {
    /// Designed, not yet built.
    ThreeD,
    #[default]
    Hex,
    /// Designed, not yet built.
    Texture,
}

/// The state of the open file's textures.
///
/// Decoding is 73 images expanded to RGBA8, so it happens on the worker and the interface has to be
/// able to say "not yet" — which is a state, not a `None`.
#[derive(Default)]
pub enum Textures {
    /// Nobody has needed them yet.
    #[default]
    Unasked,
    /// A decode job is in flight.
    Decoding,
    /// Decoded — or decoded to nothing, which is what a file without textures gives.
    Ready(Option<std::sync::Arc<gizmo_nfs::Tpk>>),
}

impl Textures {
    /// The pack, if it is decoded and not empty.
    #[must_use]
    pub fn ready(&self) -> Option<&std::sync::Arc<gizmo_nfs::Tpk>> {
        match self {
            Textures::Ready(tpk) => tpk.as_ref(),
            _ => None,
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
    /// Files opened this session, most recent first.
    pub recents: Vec<std::path::PathBuf>,
    /// The welcome screen's path field.
    pub path_input: String,
    /// The selected chunk's parsed model, keyed by its offset so it is rebuilt only on a change.
    model: Option<(usize, gizmo_nfs::inspect::ChunkModel)>,
    /// eframe's wgpu device, for the 3D tab. `None` when the backend is not wgpu, in which case
    /// the tab says so instead of the app refusing to run.
    pub render_state: Option<eframe::egui_wgpu::RenderState>,
    /// The preview renderer, built lazily the first time the tab is opened.
    pub preview: Option<crate::gpu::preview::Preview>,
    /// Where the preview camera is looking from.
    pub camera: crate::panels::viewport3d::Camera,
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
    /// Set when the density or language changed and the style must be rebuilt.
    restyle: bool,
    /// `--shot <path>`: draw a few frames, save the window as a PNG, and exit. The tool renders
    /// through a GPU surface, so this is the only way to check the interface on a machine whose
    /// compositor will not hand out a screen grab — and it doubles as a way to keep a visual
    /// record of the design port.
    shot: Option<Shot>,
}

/// A pending `--shot` request.
struct Shot {
    path: std::path::PathBuf,
    /// Frames to draw before asking for the image: the first frame has no layout yet, and the
    /// font atlas is built lazily.
    warmup: u8,
    asked: bool,
}

impl PryHub {
    /// Build the app, optionally opening a file and/or saving a screenshot.
    #[must_use]
    pub fn new(
        ctx: &egui::Context,
        open: Option<String>,
        shot: Option<String>,
        screen: Option<String>,
    ) -> Self {
        let mut app = Self {
            screen: Screen::Welcome,
            tab: Tab::default(),
            lang: Lang::default(),
            density: Density::default(),
            log_filter: LogFilter::default(),
            doc: None,
            textures: Textures::default(),
            log: Vec::new(),
            jobs: crate::jobs::Jobs::start(ctx.clone()),
            selection: None,
            collapsed: std::collections::HashSet::new(),
            scroll_hex_to: None,
            error: None,
            recents: Vec::new(),
            path_input: String::new(),
            model: None,
            render_state: None,
            preview: None,
            camera: crate::panels::viewport3d::Camera::default(),
            names: crate::names::Names::load(),
            dict: crate::screens::dictionary::State::default(),
            diff: crate::screens::diff::State::default(),
            discover: crate::screens::discovery::State::default(),
            texture_selection: None,
            texture_cache: std::collections::HashMap::new(),
            restyle: false,
            shot: shot.map(|p| Shot { path: p.into(), warmup: 4, asked: false }),
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

    fn open_side(&mut self, path: &std::path::Path, side: crate::jobs::Side) {
        self.jobs.send(crate::jobs::Request::Open { path: path.to_path_buf(), side });
    }

    /// Take everything the worker has finished.
    fn collect_jobs(&mut self) {
        use crate::jobs::{Outcome, Side};
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
                Outcome::Decoded { for_path, tpk } => {
                    if self.doc.as_ref().is_some_and(|d| d.path == for_path) {
                        self.textures = Textures::Ready(tpk);
                    }
                }
                Outcome::Exported(result) => self.report_export(result),
                Outcome::Failed(message) => self.log.push(Note {
                    level: Level::Error,
                    chunk: None,
                    chunk_id: String::new(),
                    message,
                }),
            }
        }
    }

    /// Make a freshly parsed document the open one.
    fn adopt(&mut self, doc: Doc, path: &std::path::Path) {
        self.selection = doc.rows.first().map(|r| r.offset);
        self.collapsed.clear();
        self.error = None;
        self.recents.retain(|p| p != path);
        self.recents.insert(0, path.to_path_buf());
        self.recents.truncate(6);
        self.doc = Some(std::sync::Arc::new(doc));
        self.model = None;
        self.textures = Textures::Unasked;
        self.texture_selection = None;
        self.texture_cache.clear();
        self.screen = Screen::Workspace;
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

    /// Queue an export of what the centre area is showing.
    pub fn export_now(&mut self, kind: crate::export::Kind) {
        let Some(doc) = &self.doc else { return };
        let spec = crate::export::ExportSpec {
            kind,
            selection: self.selection,
            strings: self.lang.strings(),
            textures: self.textures.ready().map(std::sync::Arc::clone),
        };
        self.jobs.send(crate::jobs::Request::Export { doc: std::sync::Arc::clone(doc), spec });
    }

    /// Where the welcome screen's path field starts: the game root, when it is set.
    #[must_use]
    pub fn suggested_dir() -> String {
        std::env::var("NFSU2_ROOT").map(|r| format!("{r}/CARS/")).unwrap_or_default()
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
        if self.restyle {
            theme::apply(ui.ctx(), self.density);
            self.restyle = false;
        }
        self.collect_jobs();
        self.refresh_model();
        self.top_bar(ui);
        self.status_bar(ui);
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
        }
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
    }
}

impl PryHub {
    /// Brand · screen nav · open/export · language · density.
    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let t = self.lang.strings();
        // Reported by the button, acted on after the bar is drawn — the export needs the whole
        // app, and the bar is holding it while it draws.
        let mut exported = false;
        egui::Panel::top("topbar")
            .exact_size(44.0)
            .frame(egui::Frame::new().fill(token::SURFACE).inner_margin(egui::Margin::symmetric(12, 0)))
            .show_inside(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    let brand = ui.add(
                        egui::Label::new(
                            RichText::new("PryHUB").font(theme::font::heading(19.0)).color(token::TEXT),
                        )
                        .sense(egui::Sense::click()),
                    );
                    if brand.clicked() {
                        self.screen = Screen::Welcome;
                    }
                    ui.label(RichText::new(t.brand_sub).size(10.0).color(theme::muted(50)));
                    ui.add_space(theme::token::SPACE_2);

                    for (screen, label) in [
                        (Screen::Workspace, t.nav_workspace),
                        (Screen::Validation, t.nav_validation),
                        (Screen::Discovery, t.nav_discovery),
                        (Screen::Diff, t.nav_diff),
                        (Screen::Dictionary, t.nav_dict),
                    ] {
                        if nav_button(ui, label, self.screen == screen).clicked() {
                            self.screen = screen;
                        }
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button(self.density.label()).on_hover_text("yoğunluk / density").clicked() {
                            self.density = self.density.next();
                            self.restyle = true;
                        }
                        if ui.button(self.lang.label()).clicked() {
                            self.lang = self.lang.other();
                        }
                        ui.separator();
                        if ui.button(t.m_open).clicked() {
                            self.screen = Screen::Welcome;
                        }
                        // Enabled only with a file open: a button that can do nothing is worse
                        // than one that is visibly not for now.
                        let can = self.doc.is_some();
                        if ui
                            .add_enabled(can, egui::Button::new(t.m_export))
                            .on_hover_text(t.export_hint)
                            .clicked()
                        {
                            exported = true;
                        }
                    });
                });
            });
        if exported {
            // Which of the three the button means is a question about the interface, so it is
            // answered here rather than inside the job.
            let kind = match self.tab {
                Tab::Texture => crate::export::Kind::Textures,
                _ => crate::export::Kind::Model,
            };
            self.export_now(kind);
        }
    }

    /// Put an export's result in the log — the design's place for "işlem çıktıları", and the only
    /// place the written paths are stated. A failure is a log line too, not a modal: the file is
    /// still open and the user has lost nothing.
    pub fn report_export(&mut self, result: Result<crate::export::Written, String>) {
        let t = self.lang.strings();
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
                    message: format!("{} — {} → {where_to}", t.exported, w.summary),
                }
            }
            Err(e) => Note {
                level: Level::Error,
                chunk: None,
                chunk_id: String::new(),
                message: format!("{}: {e}", t.export_failed),
            },
        };
        self.log.push(note);
    }

    /// File · size · chunk count · selection · codec · scale.
    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let t = self.lang.strings();
        let busy = self.jobs.busy();
        egui::Panel::bottom("statusbar")
            .exact_size(22.0)
            .frame(egui::Frame::new().fill(token::SURFACE).inner_margin(egui::Margin::symmetric(10, 0)))
            .show_inside(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    let small = |ui: &mut egui::Ui, s: String, strong: bool| {
                        let mut txt = RichText::new(s).font(theme::font::mono(10.5));
                        txt = if strong { txt.color(token::TEXT) } else { txt.color(theme::muted(60)) };
                        ui.label(txt);
                    };
                    match &self.doc {
                        Some(doc) => {
                            small(ui, doc.file_name(), true);
                            small(ui, format!("{:.2} MB", doc.bytes.len() as f32 / (1024.0 * 1024.0)), false);
                            small(ui, format!("{} {}", doc.rows.len(), t.st_chunks), false);
                            if let Some(sel) = self.selection.and_then(|o| doc.node_at(o)) {
                                small(
                                    ui,
                                    format!("{} {:#010x} · {} B", t.st_sel, sel.header.id, sel.header.size),
                                    false,
                                );
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                small(ui, t.st_scale.to_string(), false);
                                // What the worker is doing, if anything. An immediate-mode
                                // interface cannot show a frozen frame, so it has to show a word.
                                if let Some(job) = busy {
                                    ui.label(
                                        RichText::new(format!("· {job}…"))
                                            .font(theme::font::mono(10.5))
                                            .color(token::ACCENT),
                                    );
                                }
                                small(ui, format!("{:?}", doc.codec), false);
                            });
                        }
                        None => match busy {
                            Some(job) => small(ui, format!("{job}…"), true),
                            None => small(ui, t.no_file.to_string(), false),
                        },
                    }
                });
            });
    }

}

impl PryHub {
    /// Drive a pending `--shot`: warm up, ask for the image, save it, quit.
    fn screenshot(&mut self, ctx: &egui::Context) {
        // Opening and decoding are jobs now, so a screenshot taken on frame four would catch an
        // empty window. Wait for the worker to go quiet first — the flag exists to check the
        // interface, and an interface that has not loaded anything yet is not the one to check.
        let busy = self.jobs.busy().is_some();
        let Some(shot) = &mut self.shot else { return };
        if busy {
            ctx.request_repaint();
            return;
        }
        if shot.warmup > 0 {
            shot.warmup -= 1;
            ctx.request_repaint();
            return;
        }
        if !shot.asked {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            shot.asked = true;
            ctx.request_repaint();
            return;
        }
        let image = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            let rgba: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
            let (w, h) = (image.width() as u32, image.height() as u32);
            match write_png(&shot.path, &rgba, w, h) {
                Ok(()) => eprintln!("pryhub: {} ({w}x{h})", shot.path.display()),
                Err(e) => eprintln!("pryhub: {}: {e}", shot.path.display()),
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint();
    }
}

/// Write RGBA8 as a PNG. Hand-rolled (stored deflate + CRC) so a debugging convenience does not
/// add a dependency to the tool everyone builds.
fn write_png(path: &std::path::Path, rgba: &[u8], w: u32, h: u32) -> std::io::Result<()> {
    use std::io::Write as _;
    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &b in data {
            crc ^= u32::from(b);
            for _ in 0..8 {
                crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
            }
        }
        !crc
    }
    fn chunk(out: &mut Vec<u8>, tag: &[u8; 4], body: &[u8]) {
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        let mut with_tag = tag.to_vec();
        with_tag.extend_from_slice(body);
        out.extend_from_slice(&with_tag);
        out.extend_from_slice(&crc32(&with_tag).to_be_bytes());
    }
    // Raw scanlines: filter byte 0 per row.
    let mut raw = Vec::with_capacity((w as usize * 4 + 1) * h as usize);
    for y in 0..h as usize {
        raw.push(0);
        let row = y * w as usize * 4;
        raw.extend_from_slice(&rgba[row..row + w as usize * 4]);
    }
    // zlib stream with stored (uncompressed) deflate blocks — valid, just not small.
    let mut z = vec![0x78, 0x01];
    let mut adler = (1u32, 0u32);
    for &b in &raw {
        adler.0 = (adler.0 + u32::from(b)) % 65521;
        adler.1 = (adler.1 + adler.0) % 65521;
    }
    for (i, block) in raw.chunks(65535).enumerate() {
        let last = u8::from((i + 1) * 65535 >= raw.len());
        z.push(last);
        z.extend_from_slice(&(block.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
        z.extend_from_slice(block);
    }
    z.extend_from_slice(&((adler.1 << 16) | adler.0).to_be_bytes());

    let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA
    chunk(&mut png, b"IHDR", &ihdr);
    chunk(&mut png, b"IDAT", &z);
    chunk(&mut png, b"IEND", &[]);
    std::fs::File::create(path)?.write_all(&png)
}

/// A top-bar navigation button: flat, with the accent underline the design uses for "current".
fn nav_button(ui: &mut egui::Ui, label: &str, active: bool) -> egui::Response {
    let color = if active { token::ACCENT } else { theme::muted(65) };
    let text = RichText::new(label).font(theme::font::heading(11.5)).color(color);
    let resp = ui.add(egui::Button::new(text).fill(egui::Color32::TRANSPARENT).frame(false));
    if active {
        let r = resp.rect;
        ui.painter().hline(r.x_range(), r.bottom() + 2.0, egui::Stroke::new(2.0_f32, token::ACCENT));
    }
    resp
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
        PryHub::new(&egui::Context::default(), None, None, None)
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
        assert_eq!(app.jobs.busy(), Some("open"), "the status bar has something to say");
        assert!(settle(&mut app, |a| a.jobs.busy().is_none()));
    }
}
