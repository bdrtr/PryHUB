//! Work that must not happen on the frame the user is looking at.
//!
//! Opening a car is 7.6 MB of chunk walking, five validation rules and a geometry parse; decoding a
//! pack is 73 images expanded to RGBA8; an export writes a `.glb`, an OBJ and every texture it
//! references. On the UI thread each of those is a freeze, and an immediate-mode interface has
//! nowhere to hide one — the frame simply does not get drawn.
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
    Export { doc: Arc<Doc>, spec: ExportSpec },
}

/// What came back.
pub enum Outcome {
    /// Boxed because a `Doc` is large and this enum is moved through a channel.
    Opened { result: Box<Result<Doc, String>>, side: Side, path: PathBuf },
    /// `None` means the file has no textures, which is an answer rather than a failure.
    Decoded { for_path: PathBuf, tpk: Option<Arc<gizmo_nfs::Tpk>> },
    Exported(Result<Written, String>),
    /// A job panicked. The parser is panic-free by contract, but this layer is not the place to
    /// find out the hard way: the worker survives and says so.
    Failed(String),
}

impl Outcome {
    /// A short word for the status bar while this kind of work is in flight.
    fn label(request: &Request) -> &'static str {
        match request {
            Request::Open { .. } => "open",
            Request::Decode(_) => "decode",
            Request::Export { .. } => "export",
        }
    }
}

/// The worker, its channels, and what is currently in flight.
pub struct Jobs {
    to_worker: Sender<Request>,
    from_worker: Receiver<(&'static str, Outcome)>,
    /// Labels of the jobs sent but not yet collected, so the interface can say what it is waiting
    /// for rather than only that it is waiting.
    in_flight: Vec<&'static str>,
}

impl Jobs {
    /// Start the worker. `ctx` is used to wake the UI when a result lands — without it an idle
    /// interface would sit unpainted until the user moved the mouse.
    #[must_use]
    pub fn start(ctx: egui::Context) -> Self {
        let (to_worker, requests) = std::sync::mpsc::channel::<Request>();
        let (results, from_worker) = std::sync::mpsc::channel::<(&'static str, Outcome)>();
        std::thread::Builder::new()
            .name("pryhub-worker".into())
            .spawn(move || {
                for request in requests {
                    let label = Outcome::label(&request);
                    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        run(request)
                    }))
                    .unwrap_or_else(|_| {
                        Outcome::Failed("a background job panicked; the worker is still up".into())
                    });
                    if results.send((label, outcome)).is_err() {
                        break; // the app is gone
                    }
                    ctx.request_repaint();
                }
            })
            .expect("spawn the worker thread");
        Self { to_worker, from_worker, in_flight: Vec::new() }
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
            if let Some(i) = self.in_flight.iter().position(|l| *l == label) {
                self.in_flight.remove(i);
            }
            done.push(outcome);
        }
        done
    }

    /// Whether anything is running, and what.
    #[must_use]
    pub fn busy(&self) -> Option<&'static str> {
        self.in_flight.first().copied()
    }

}

/// Do the work. Runs on the worker thread; nothing here touches the interface.
fn run(request: Request) -> Outcome {
    match request {
        Request::Open { path, side } => {
            let result = Doc::open(&path);
            Outcome::Opened { result: Box::new(result), side, path }
        }
        Request::Decode(doc) => {
            let tpk = doc.decode_textures().map(Arc::new);
            Outcome::Decoded { for_path: doc.path.clone(), tpk }
        }
        Request::Export { doc, spec } => Outcome::Exported(crate::export::run(&doc, &spec)),
    }
}
