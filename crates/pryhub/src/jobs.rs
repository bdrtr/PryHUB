//! Work that must not happen on the frame the user is looking at.
//!
//! Opening a car is 7.6 MB of chunk walking, five validation rules and a geometry parse; decoding a
//! pack is every image in it expanded to RGBA8 — 73 and 30 ms for a car, and up to the 256 MB
//! `doc::DECODE_BUDGET` and a couple of seconds for a `VINYLS.BIN`; an export writes a `.glb`, an
//! OBJ and every texture it references. On the UI thread each of those is a freeze, and an
//! immediate-mode interface has nowhere to hide one — the frame simply does not get drawn.
//!
//! # Why threads and not `async`
//!
//! All of it is **CPU work with no waiting in it**. `async` is a way to manage waiting; it does not
//! make computation faster or a synchronous parser interruptible. Making the parser `async` would
//! infect every call with `.await`, put a runtime inside a crate whose whole value is that it has no
//! dependencies, and buy nothing at all. So: the parser stays a pure function of bytes, and this
//! module runs it somewhere else.
//!
//! # The shape
//!
//! One worker thread, a request channel, a result channel. The document is immutable once open and
//! travels as an [`Arc`], so a job reads it without copying and without a lock. Results carry the
//! identity of what they were computed *for*, so a decode that finishes after the user opened
//! another file is dropped rather than applied to the wrong document — which is the whole reason
//! this is not a bare `thread::spawn` per click.

use crate::doc::Doc;
use crate::export::{ExportSpec, Written};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

/// Which side of the compare screen an opened file belongs to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    /// The file the whole app is about.
    Main,
    /// The compare screen's other file.
    Other,
}

/// Something to do off the UI thread.
pub enum Request {
    Open { path: PathBuf, side: Side },
    /// Decode a document's textures — its own `TEXTURES.BIN`, or the one beside it.
    Decode(Arc<Doc>),
    /// Read a pack's texture *names* without decoding any pixels, for the hash dictionary.
    TextureNames(Arc<Doc>),
    Export { doc: Arc<Doc>, spec: ExportSpec },
    /// Read the game's paint palette out of `GLOBAL/GLOBALB.BUN`, found from the open file.
    ///
    /// A job because that bundle is 8 MB and holds 12,167 parts: the palette is worth a swatch row,
    /// not a stutter.
    Palette { beside: PathBuf },
    /// **Every** car record in the install's bundle, found from the open file. Same bundle as
    /// [`Self::Palette`] and the same 8 MB read, so it is a job for the same reason.
    ///
    /// All of them rather than the open one, because the 46 records are in a single file and reading
    /// it 46 times to look at 46 cars would be 46 times the work for none of the benefit. It is what
    /// lets the CARP screen offer a car list instead of making someone open another `GEOMETRY.BIN`
    /// to see another car's numbers.
    CarSpec { beside: PathBuf },
    /// Put a PNG into the open pack. Boxed for the same reason [`Outcome::Opened`] is — it carries
    /// three paths and this enum travels through a channel.
    ///
    /// A job because it is the heaviest thing here per byte: a PNG decode, a whole mip chain
    /// re-encoded, a re-compression, and then the written pack decoded again to check it. On a
    /// 512² vinyl that is not a frame's worth of work.
    Replace(Box<crate::replace::Spec>),
    /// Write the CARP screen's edits into the install's own car database. Boxed like
    /// [`Self::Replace`], and a job for the same reasons: an 8 MB read, a set of lane writes, a
    /// re-parse to check them, and two files written.
    Tune(Box<crate::tune::Spec>),
}

/// What came back.
pub enum Outcome {
    /// Boxed because a `Doc` is large and this enum is moved through a channel.
    Opened { result: Box<Result<Doc, String>>, side: Side, path: PathBuf },
    /// `None` means the file has no textures, which is an answer rather than a failure. `unread` is
    /// how many the byte budget never attempted — not the same as the ones that would not decode.
    Decoded { for_path: PathBuf, tpk: Option<Arc<gizmo_nfs::Tpk>>, unread: usize },
    /// A pack's texture names, read without decoding any pixels. Carries the path it was computed
    /// for, for the same reason [`Self::Decoded`] does.
    TextureNames { for_path: PathBuf, names: Arc<Vec<(gizmo_nfs::AssetHash, String)>> },
    Exported(Result<Written, String>),
    /// A texture written back. Carries the *document* it was asked for, the way [`Self::Decoded`]
    /// does: this one changes what is on screen, so landing it against a file the user has since
    /// replaced would repaint the wrong pack's contact sheet.
    Replaced { for_path: PathBuf, result: Box<Result<crate::replace::Done, String>> },
    /// A car's handling written back. Carries no document identity: the bundle belongs to the
    /// *install*, not to the open file, so a save that lands after the user opened another car is
    /// still true and still worth reporting.
    Tuned(Box<Result<crate::tune::Done, String>>),
    /// A job saying how far along it is. Intercepted by [`Jobs::poll`] rather than handed on: it is
    /// a fact about the *worker*, not about the document.
    Progress { done: usize, total: usize },
    /// The game's paint palette. Empty when no bundle was found, which is an answer: the swatch
    /// row then says the file is not in an install rather than showing nothing.
    Palette(Vec<gizmo_nfs::Colour>),
    /// The open car's record. `None` means the bundle was not found *or* holds no record under
    /// that name — the CARP screen says which by also knowing whether an install was reachable.
    CarSpec {
        /// Every record in the bundle, in file order. Empty when no install was reachable.
        cars: Arc<Vec<gizmo_nfs::CarTypeInfo>>,
        /// The file they were read out of — `GlobalB.lzc` where there is one. The CARP screen names
        /// it beside its Save button, so that the file about to be written is one the user was shown
        /// rather than one they find out about afterwards.
        bundle: Option<PathBuf>,
        /// Which of them the open file *is*, by the name of the directory it sits in. `None` when
        /// the open file is not a car, or is a car the bundle does not list — both of which are
        /// ordinary, and neither of which should stop the other 46 being browsable.
        opened: Option<String>,
    },
    /// A job panicked. The parser is panic-free by contract, but this layer is not the place to
    /// find out the hard way: the worker survives and says so.
    Failed(String),
}

/// What kind of work is in flight.
///
/// A *kind* rather than a word, because two very different readers want it: the status bar, which
/// must say it in whichever language the interface is in, and `PRYHUB_LOG`, which is the program's
/// own stream and speaks one. Handing out `"open"` served the second and quietly made the first
/// untranslatable — an English word in the corner of an otherwise Turkish window.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Open,
    Decode,
    TextureNames,
    Export,
    Palette,
    CarSpec,
    Replace,
    Tune,
}

impl Kind {
    /// For the program log, which is English by the same rule the parser is.
    fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Decode => "decode",
            Self::TextureNames => "texture names",
            Self::Export => "export",
            Self::Palette => "palette",
            Self::CarSpec => "car spec",
            Self::Replace => "replace",
            Self::Tune => "tune",
        }
    }
}

impl std::fmt::Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Outcome {
    /// Which kind of work a request is.
    fn label(request: &Request) -> Kind {
        match request {
            Request::Open { .. } => Kind::Open,
            Request::Decode(_) => Kind::Decode,
            Request::TextureNames(_) => Kind::TextureNames,
            Request::Export { .. } => Kind::Export,
            Request::Palette { .. } => Kind::Palette,
            Request::CarSpec { .. } => Kind::CarSpec,
            Request::Replace(_) => Kind::Replace,
            Request::Tune(_) => Kind::Tune,
        }
    }
}

/// The worker, its channels, and what is currently in flight.
pub struct Jobs {
    to_worker: Sender<Request>,
    from_worker: Receiver<(Kind, Outcome)>,
    /// How far the running job says it has got, when it can say.
    progress: Option<(usize, usize)>,
    /// Kinds of the jobs sent but not yet collected, so the interface can say what it is waiting
    /// for rather than only that it is waiting.
    in_flight: Vec<Kind>,
}

impl Jobs {
    /// Start the worker. `ctx` is used to wake the UI when a result lands — without it an idle
    /// interface would sit unpainted until the user moved the mouse.
    #[must_use]
    pub fn start(ctx: egui::Context) -> Self {
        let (to_worker, requests) = std::sync::mpsc::channel::<Request>();
        let (results, from_worker) = std::sync::mpsc::channel::<(Kind, Outcome)>();
        std::thread::Builder::new()
            .name("pryhub-worker".into())
            .spawn(move || {
                for request in requests {
                    let label = Outcome::label(&request);
                    let started = std::time::Instant::now();
                    log::debug!(target: "jobs", "{label} started: {}", describe(&request));
                    let tell = |done: usize, total: usize| {
                        let _ = results.send((label, Outcome::Progress { done, total }));
                        ctx.request_repaint();
                    };
                    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        run(request, &tell)
                    }))
                    .unwrap_or_else(|_| {
                        log::error!(target: "jobs", "{label} panicked; the worker is still up");
                        Outcome::Failed("a background job panicked; the worker is still up".into())
                    });
                    log::debug!(
                        target: "jobs",
                        "{label} finished in {:.1} ms",
                        started.elapsed().as_secs_f64() * 1000.0
                    );
                    if results.send((label, outcome)).is_err() {
                        break; // the app is gone
                    }
                    ctx.request_repaint();
                }
            })
            .expect("spawn the worker thread");
        Self { to_worker, from_worker, progress: None, in_flight: Vec::new() }
    }

    /// Queue a job.
    pub fn send(&mut self, request: Request) {
        self.in_flight.push(Outcome::label(&request));
        if self.to_worker.send(request).is_err() {
            self.in_flight.pop();
        }
    }

    /// Collect whatever has finished. Returns nothing when the worker is still busy — the caller
    /// keeps drawing.
    pub fn poll(&mut self) -> Vec<Outcome> {
        let mut done = Vec::new();
        while let Ok((label, outcome)) = self.from_worker.try_recv() {
            if let Outcome::Progress { done, total } = outcome {
                self.progress = Some((done, total));
                continue; // not a result: the job is still running
            }
            if let Some(i) = self.in_flight.iter().position(|l| *l == label) {
                self.in_flight.remove(i);
            }
            self.progress = None;
            done.push(outcome);
        }
        done
    }

    /// Whether anything is running, and what.
    #[must_use]
    pub fn busy(&self) -> Option<Kind> {
        self.in_flight.first().copied()
    }

    /// How far along it is, `0.0..=1.0`, for the jobs that can say. A job that cannot — a parse is
    /// one call into the parser — reports nothing and gets a spinner instead of a lying bar.
    #[must_use]
    pub fn progress(&self) -> Option<f32> {
        self.progress
            .filter(|(_, total)| *total > 0)
            .map(|(done, total)| done as f32 / total as f32)
    }

}

/// What a request is about, for the log. Enough to tell two runs apart, not the whole document.
fn describe(request: &Request) -> String {
    match request {
        Request::Open { path, side } => format!("{} ({side:?})", path.display()),
        Request::Decode(doc) | Request::TextureNames(doc) => doc.path.display().to_string(),
        Request::Export { doc, spec } => format!("{} as {}", doc.path.display(), spec.kind.name()),
        Request::Palette { beside } => format!("palette beside {}", beside.display()),
        Request::CarSpec { beside } => format!("car spec beside {}", beside.display()),
        Request::Replace(spec) => {
            format!("{} edits into {}", spec.edits.len(), spec.pack.display())
        }
        Request::Tune(spec) => {
            format!("{} lanes of {} beside {}", spec.edits.len(), spec.car, spec.beside.display())
        }
    }
}

/// Do the work. Runs on the worker thread; nothing here touches the interface except by saying how
/// far it has got through `tell`.
fn run(request: Request, tell: &dyn Fn(usize, usize)) -> Outcome {
    match request {
        Request::Open { path, side } => {
            let result = Doc::open(&path);
            Outcome::Opened { result: Box::new(result), side, path }
        }
        Request::Decode(doc) => {
            let decoded = doc.decode_textures(tell);
            let unread = decoded.as_ref().map_or(0, |(_, n)| *n);
            let tpk = decoded.map(|(tpk, _)| Arc::new(tpk));
            Outcome::Decoded { for_path: doc.path.clone(), tpk, unread }
        }
        Request::TextureNames(doc) => Outcome::TextureNames {
            for_path: doc.path.clone(),
            names: Arc::new(doc.texture_names(tell)),
        },
        Request::Export { doc, spec } => {
            Outcome::Exported(crate::export::run(&doc, &spec, tell))
        }
        Request::Replace(spec) => {
            // The document the replacement was asked for is the pack's *neighbour* as often as the
            // pack itself — a `GEOMETRY.BIN` is drawn with the `TEXTURES.BIN` next to it — so the
            // identity carried back is the one the app matches on, which the caller put here.
            let for_path = spec.doc.clone();
            Outcome::Replaced { for_path, result: Box::new(crate::replace::run(&spec)) }
        }
        Request::Tune(spec) => Outcome::Tuned(Box::new(crate::tune::run(&spec))),
        Request::Palette { beside } => Outcome::Palette(palette(&beside)),
        Request::CarSpec { beside } => {
            let (cars, bundle, opened) = car_spec(&beside);
            Outcome::CarSpec { cars: Arc::new(cars), bundle, opened }
        }
    }
}

/// `GLOBAL/GLOBALB.BUN` for a file inside a game install, decompressed.
///
/// Found from the open file rather than configured: a car's directory is two levels under the root,
/// which is the same walk `ug2` does. Anything that does not resolve returns `None` — a
/// `GEOMETRY.BIN` copied out of an install is a perfectly ordinary thing to open.
fn globalb_bytes(beside: &std::path::Path) -> Option<(std::path::PathBuf, Vec<u8>)> {
    let dir = beside.parent()?;
    let candidates = [
        dir.parent().and_then(|p| p.parent()).map(|r| r.join("GLOBAL").join("GLOBALB.BUN")),
        std::env::var_os("NFSU2_ROOT")
            .map(|r| std::path::PathBuf::from(r).join("GLOBAL").join("GLOBALB.BUN")),
    ];
    for path in candidates.into_iter().flatten() {
        if !path.is_file() {
            continue;
        }
        let Ok(raw) = std::fs::read(&path) else { continue };
        // The bundle ships uncompressed beside a `.lzc` twin, but detect anyway rather than assume.
        let bytes = match gizmo_nfs::compression::detect(&raw) {
            gizmo_nfs::compression::Codec::None => raw,
            _ => match gizmo_nfs::compression::decompress(&raw) {
                Ok(b) => b,
                Err(_) => continue,
            },
        };
        return Some((path, bytes));
    }
    None
}

/// The palette in the install's `GLOBALB.BUN`, or an empty one when there is no install.
fn palette(beside: &std::path::Path) -> Vec<gizmo_nfs::Colour> {
    let Some((path, bytes)) = globalb_bytes(beside) else { return Vec::new() };
    if let Ok(cp) = gizmo_nfs::CarParts::parse(&bytes) {
        log::debug!(target: "jobs", "palette: {} colours from {}", cp.palette().len(), path.display());
        return cp.palette();
    }
    Vec::new()
}

/// Every car record in the install's bundle, the file they came out of, and which one the open file
/// is.
///
/// `CARS/240SX/GEOMETRY.BIN` is the record called `240SX`. That is the only link there is: the
/// record carries no path, and the directory name is what `ug2 globalb <car>` matches on too. It
/// decides which car the screen *opens* on and nothing else — every other record is read anyway, so
/// a `GEOMETRY.BIN` copied out of an install still browses all 46.
///
/// **Read from `GlobalB.lzc` first**, which is the file the game opens — and, since the CARP screen
/// can now write, the one that has to be shown. The two ship identical and stay identical while
/// nothing edits them; the moment something does, reading the `.BUN` would show the car that is not
/// being driven. The `.BUN` remains the fallback for a `GEOMETRY.BIN` that has been copied out of an
/// install and put beside a lone bundle, which is an ordinary thing to have done.
fn car_spec(
    beside: &std::path::Path,
) -> (Vec<gizmo_nfs::CarTypeInfo>, Option<std::path::PathBuf>, Option<String>) {
    let opened = beside
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .map(|n| n.to_ascii_uppercase());
    let from_install = gizmo_nfs::globalb::install::find(beside).and_then(|bundle| {
        gizmo_nfs::globalb::install::read(&bundle).ok().map(|bytes| (bundle.lzc, bytes))
    });
    let Some((path, bytes)) = from_install.or_else(|| globalb_bytes(beside)) else {
        return (Vec::new(), None, opened);
    };
    let cars = gizmo_nfs::globalb::parse_cartypeinfos(&bytes);
    // The open file names a record only if the bundle actually holds one under that name.
    let opened = opened.filter(|want| cars.iter().any(|c| &c.name == want));
    log::debug!(
        target: "jobs",
        "car spec: {} records in {}, open file is {}",
        cars.len(),
        path.display(),
        opened.as_deref().unwrap_or("not one of them")
    );
    (cars, Some(path), opened)
}
