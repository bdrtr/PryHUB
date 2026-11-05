//! The real interface, driven by real pointer events, over a copy of the install.
//!
//! The unit tests around this crate exercise the workers. Nothing else asks whether pressing the
//! button reaches them, and a whole class of bug lives in that gap: clicking a stride chip on the
//! Discovery screen used to take the program down, with every test green.
//!
//! Skipped without `NFSU2_ROOT`, like the golden tests. The grid of clicks takes minutes, so it is
//! `#[ignore]`d and run by hand:
//!
//! ```text
//! NFSU2_ROOT=... cargo test -p pryhub click_tests -- --ignored
//! ```
//!
//! A grid only looks for panics, the worker's included; it says nothing about whether a click did
//! the right thing. That is what the named flows are for. There is no GPU here, so neither covers
//! the 3D view's own drawing or its drags.

use crate::app::{PryHub, Screen, Tab};
use egui::accesskit::Role;
use egui::{Event, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

const WIDE: [f32; 2] = [1360.0, 840.0];
/// What this project's tiling compositor actually handed the window.
const NARROW: [f32; 2] = [948.0, 492.0];
const GEOMETRY: &str = "CARS/240SX/GEOMETRY.BIN";
const TEXTURES: &str = "CARS/240SX/TEXTURES.BIN";
const BUNDLE: &str = "GLOBAL/GlobalB.lzc";

/// One test at a time holds the process's working directory and its settings path.
static PROCESS: Mutex<()> = Mutex::new(());

/// A throwaway install that the process is pointed into for as long as this lives: the buttons
/// write beside the working directory, into the settings path and over "the original".
struct Install {
    dir: PathBuf,
    cwd: PathBuf,
    config: Option<std::ffi::OsString>,
    _process: MutexGuard<'static, ()>,
}

impl Install {
    fn new(name: &str) -> Option<Self> {
        let Some(real) = std::env::var_os("NFSU2_ROOT").map(PathBuf::from) else {
            eprintln!("NFSU2_ROOT unset — skipping");
            return None;
        };
        let process = PROCESS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let dir = std::env::temp_dir().join(format!("pryhub-clicks-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for car in ["240SX", "RX7"] {
            let (from, to) = (real.join("CARS").join(car), dir.join("CARS").join(car));
            std::fs::create_dir_all(&to).expect("the copy's car folder");
            for file in ["GEOMETRY.BIN", "TEXTURES.BIN"] {
                std::fs::copy(from.join(file), to.join(file)).expect("a car file");
            }
        }
        std::fs::create_dir_all(dir.join("GLOBAL")).expect("the copy's GLOBAL");
        for file in ["GlobalB.lzc", "GLOBALB.BUN"] {
            std::fs::copy(real.join("GLOBAL").join(file), dir.join("GLOBAL").join(file)).expect("a bundle");
        }
        std::fs::create_dir_all(dir.join("cwd")).expect("a working directory");
        let cwd = std::env::current_dir().expect("the working directory");
        std::env::set_current_dir(dir.join("cwd")).expect("into the copy");
        let config = std::env::var_os("XDG_CONFIG_HOME");
        std::env::set_var("XDG_CONFIG_HOME", dir.join("config"));
        Some(Self { dir, cwd, config, _process: process })
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.dir.join(relative)
    }

    fn files_written(&self) -> Vec<PathBuf> {
        fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
            for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                if entry.path().is_dir() {
                    walk(&entry.path(), out);
                } else {
                    out.push(entry.path());
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.dir.join("cwd"), &mut out);
        out
    }
}

impl Drop for Install {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.cwd);
        match &self.config {
            Some(value) => std::env::set_var("XDG_CONFIG_HOME", value),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn open(file: Option<&Path>, screen: Option<&str>, size: [f32; 2], stage: Option<PathBuf>) -> Harness<'static, PryHub> {
    let file = file.map(|p| p.to_string_lossy().into_owned());
    let screen = screen.map(str::to_owned);
    let mut h = Harness::builder().with_size(size).build_eframe(|cc| {
        let chosen = crate::settings::Settings::default();
        crate::theme::install_fonts(&cc.egui_ctx);
        crate::theme::apply(&cc.egui_ctx, chosen.density);
        let mut app = PryHub::new(&cc.egui_ctx, chosen, file, None, screen);
        app.pending_stage = stage;
        app
    });
    settle(&mut h);
    h
}

/// Draw until the worker has been quiet for a few frames.
fn settle(h: &mut Harness<'static, PryHub>) {
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut quiet = 0;
    while quiet < 4 {
        h.step();
        if h.state().jobs.busy().is_some() {
            quiet = 0;
            std::thread::sleep(Duration::from_millis(5));
        } else {
            quiet += 1;
        }
        assert!(Instant::now() < deadline, "the worker never went quiet");
    }
}

fn click_at(h: &mut Harness<'static, PryHub>, pos: Pos2) {
    let press = |pressed| Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    h.event(Event::PointerMoved(pos));
    h.step();
    h.event(press(true));
    h.step();
    h.event(press(false));
    h.step();
}

/// Click the last button whose label contains `text`: a dialog is drawn after what it covers.
fn press(h: &mut Harness<'static, PryHub>, text: &str) {
    let pos = {
        let buttons: Vec<_> = h.get_all_by_role(Role::Button).collect();
        let hit = buttons.iter().rfind(|n| n.accesskit_node().label().is_some_and(|l| l.contains(text)));
        match hit {
            Some(node) => node.rect().center(),
            None => {
                let names: Vec<_> =
                    buttons.iter().filter_map(|n| n.accesskit_node().label().map(|l| l.to_string())).collect();
                panic!("no button with {text:?}, only {names:?}");
            }
        }
    };
    click_at(h, pos);
    settle(h);
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// A value typed into the field the way a person does it, then Save.
#[test]
fn save_carp_writes_the_bundle_and_keeps_a_backup() {
    let Some(install) = Install::new("carp") else { return };
    let bundle = || gizmo_nfs::globalb::install::find(&install.dir).expect("the bundle");
    let before = gizmo_nfs::globalb::install::read(&bundle()).expect("reads");
    let mut h = open(Some(&install.path(BUNDLE)), Some("carp"), WIDE, None);
    h.state_mut().carp.section = crate::screens::carp::section_index("ENGINE").expect("a section");
    settle(&mut h);

    let field = h.get_all_by_role(Role::SpinButton).next().expect("an editable value").rect().center();
    click_at(&mut h, field);
    h.key_press_modifiers(Modifiers::COMMAND, egui::Key::A);
    h.event(Event::Text("7777".to_owned()));
    h.step();
    h.key_press(egui::Key::Enter);
    settle(&mut h);
    press(&mut h, "Save CARP");

    match h.state().carp_saved.as_ref() {
        Some(Ok(done)) => assert_eq!(done.changed, 1),
        Some(Err(e)) => panic!("the save failed: {e}"),
        None => panic!("the save never reported back"),
    }
    let after = gizmo_nfs::globalb::install::read(&bundle()).expect("still reads");
    assert_eq!(before.len(), after.len());
    let changed = before.iter().zip(&after).filter(|(a, b)| a != b).count();
    assert!(changed > 0 && changed <= 4, "one f32 lane, and {changed} bytes moved");
    assert!(install.path("GLOBAL/GlobalB.lzc.bak").is_file(), "no backup of what the game reads");
}

/// The pack's first texture with its colours inverted, as a file to stage: the same size as what it
/// replaces, and certain to differ.
fn inverted_png(install: &Install) -> PathBuf {
    let pack = gizmo_nfs::Tpk::parse(&read(&install.path(TEXTURES))).expect("the pack parses");
    let (_, texture) = pack.textures.iter().find(|(_, t)| t.name == "240SX_BADGING").expect("the badging");
    let mut inverted = texture.clone();
    for px in inverted.rgba.as_chunks_mut::<4>().0.iter_mut() {
        for channel in &mut px[..3] {
            *channel = 255 - *channel;
        }
    }
    let path = install.path("stage.png");
    std::fs::write(&path, gizmo_nfs::export::png_bytes(&inverted).expect("encodes")).expect("writes");
    path
}

fn textures_changed(before: &[u8], after: &[u8]) -> usize {
    let (a, b) = (gizmo_nfs::Tpk::parse(before).expect("parses"), gizmo_nfs::Tpk::parse(after).expect("parses"));
    assert_eq!(a.textures.len(), b.textures.len());
    a.textures.iter().filter(|(hash, t)| b.textures[*hash].rgba != t.rgba).count()
}

#[test]
fn a_texture_written_to_a_copy_leaves_the_pack_alone() {
    let Some(install) = Install::new("copy") else { return };
    let pack = install.path(TEXTURES);
    let original = read(&pack);
    let mut h = open(Some(&pack), Some("replace"), WIDE, Some(inverted_png(&install)));
    press(&mut h, "Write ·");
    assert_eq!(read(&pack), original, "the pack itself moved");
    let written = install.files_written();
    let copy = written.iter().find(|p| p.extension().is_some_and(|e| e == "BIN"));
    let copy = copy.unwrap_or_else(|| panic!("no copy among {written:?}"));
    assert_eq!(textures_changed(&original, &read(copy)), 1);
}

#[test]
fn a_texture_written_over_the_original_keeps_a_backup() {
    let Some(install) = Install::new("over") else { return };
    let pack = install.path(TEXTURES);
    let original = read(&pack);
    let mut h = open(Some(&pack), Some("replace"), WIDE, Some(inverted_png(&install)));
    press(&mut h, "Over the original");
    press(&mut h, "Write ·");
    assert_eq!(textures_changed(&original, &read(&pack)), 1);
    assert_eq!(read(&install.path("CARS/240SX/TEXTURES.BIN.bak")), original);
}

#[test]
fn the_export_dialog_writes_a_car_on_four_wheels() {
    let Some(install) = Install::new("export") else { return };
    let mut h = open(Some(&install.path(GEOMETRY)), None, WIDE, None);
    press(&mut h, "Export");
    assert!(h.state().show_export, "the dialog did not open");
    press(&mut h, "Export ·");
    let written = install.files_written();
    let glb = written.iter().find(|p| p.extension().is_some_and(|e| e == "glb"));
    let glb = read(glb.unwrap_or_else(|| panic!("no .glb among {written:?}")));
    let json_len = u32::from_le_bytes([glb[12], glb[13], glb[14], glb[15]]) as usize;
    let json = String::from_utf8_lossy(&glb[20..20 + json_len]);
    for wheel in ["_WHEEL_A_FL", "_WHEEL_A_FR", "_WHEEL_A_RR", "_WHEEL_A_RL"] {
        assert!(json.contains(wheel), "{wheel} is missing");
    }
}

#[test]
fn every_nav_button_arrives_and_the_language_is_remembered() {
    let Some(install) = Install::new("nav") else { return };
    let mut h = open(Some(&install.path(GEOMETRY)), None, WIDE, None);
    for (label, want) in [
        ("CARP", Screen::Carp),
        ("Validation", Screen::Validation),
        ("Discovery", Screen::Discovery),
        ("Compare", Screen::Diff),
        ("Dictionary", Screen::Dictionary),
        ("Workspace", Screen::Workspace),
    ] {
        press(&mut h, label);
        assert_eq!(h.state().screen, want, "{label}");
    }
    press(&mut h, "Settings");
    let turkish = h.get_by_label_contains("Türkçe").rect().center();
    click_at(&mut h, turkish);
    settle(&mut h);
    assert_eq!(h.state().lang, crate::i18n::Lang::Tr);
    let saved = std::fs::read_to_string(crate::settings::Settings::path().expect("a path")).expect("saved");
    assert!(saved.contains("language\ttr"), "{saved}");
}

/// Every chip, in both directions: the ones that grow the column list are what used to panic.
#[test]
fn every_stride_chip_survives_being_pressed() {
    let Some(install) = Install::new("chips") else { return };
    let mut h = open(Some(&install.path(GEOMETRY)), Some("discovery"), WIDE, None);
    for chip in ["48×", "4×", "12×", "24×", "16×", "8×", "48×"] {
        press(&mut h, chip);
    }
    press(&mut h, "guess");
    assert!(!h.state().discover.schema.columns.is_empty());
}

static PANICS: AtomicUsize = AtomicUsize::new(0);

/// Click a grid over one view, staying on it. Only panics are looked for.
///
/// The bars are left out and the view is put back after every click: otherwise the first row of a
/// grid lands on the nav, every click after it lands on whatever that opened, and the test passes
/// having looked at one screen.
fn grid(name: &str, file: Option<&str>, screen: Option<&str>, tab: Option<Tab>, size: [f32; 2]) {
    static HOOK: std::sync::Once = std::sync::Once::new();
    let Some(install) = Install::new(name) else { return };
    HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            PANICS.fetch_add(1, Ordering::SeqCst);
            previous(info);
        }));
    });
    PANICS.store(0, Ordering::SeqCst);

    let path = file.map(|f| install.path(f));
    let mut h = open(path.as_deref(), screen, size, None);
    if let Some(tab) = tab {
        h.state_mut().tab = tab;
        settle(&mut h);
    }
    let (home_screen, home_tab) = (h.state().screen, h.state().tab);
    let (columns, rows) = (40, 26);
    let (top, height) = (48.0, size[1] - 48.0 - 28.0);
    let mut clicks = 0;
    for y in 0..rows {
        for x in 0..columns {
            let across = (x as f32 + 0.5) * size[0] / columns as f32;
            click_at(&mut h, Pos2::new(across, top + (y as f32 + 0.5) * height / rows as f32));
            clicks += 1;
            if clicks % 37 == 0 {
                h.event(Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -120.0),
                    modifiers: Modifiers::NONE,
                    phase: egui::TouchPhase::Move,
                });
                h.step();
            }
            if clicks % 9 == 0 {
                settle(&mut h);
            }
            let state = h.state_mut();
            state.screen = home_screen;
            state.tab = home_tab;
        }
    }
    settle(&mut h);
    assert_eq!(PANICS.load(Ordering::SeqCst), 0, "{name} panicked");
}

#[test]
#[ignore = "minutes of clicking"]
fn grid_welcome() {
    grid("welcome", None, None, None, WIDE);
}

#[test]
#[ignore = "minutes of clicking"]
fn grid_3d() {
    grid("3d", Some(GEOMETRY), None, Some(Tab::ThreeD), WIDE);
}

#[test]
#[ignore = "minutes of clicking"]
fn grid_hex() {
    grid("hex", Some(GEOMETRY), None, Some(Tab::Hex), WIDE);
}

#[test]
#[ignore = "minutes of clicking"]
fn grid_assembly() {
    grid("assembly", Some(GEOMETRY), None, Some(Tab::Assembly), WIDE);
}

#[test]
#[ignore = "minutes of clicking"]
fn grid_texture() {
    grid("texture", Some(TEXTURES), None, Some(Tab::Texture), WIDE);
}

#[test]
#[ignore = "minutes of clicking"]
fn grid_validation() {
    grid("validation", Some(GEOMETRY), Some("validation"), None, WIDE);
}

#[test]
#[ignore = "minutes of clicking"]
fn grid_discovery() {
    grid("discovery", Some(GEOMETRY), Some("discovery"), None, WIDE);
}

#[test]
#[ignore = "minutes of clicking"]
fn grid_compare() {
    grid("compare", Some(GEOMETRY), Some("diff"), None, WIDE);
}

#[test]
#[ignore = "minutes of clicking"]
fn grid_dictionary() {
    grid("dictionary", Some(GEOMETRY), Some("dictionary"), None, WIDE);
}

#[test]
#[ignore = "minutes of clicking"]
fn grid_carp() {
    grid("carp", Some(BUNDLE), Some("carp"), None, WIDE);
}

#[test]
#[ignore = "minutes of clicking"]
fn grid_narrow_assembly() {
    grid("narrow-assembly", Some(GEOMETRY), None, Some(Tab::Assembly), NARROW);
}

#[test]
#[ignore = "minutes of clicking"]
fn grid_narrow_carp() {
    grid("narrow-carp", Some(BUNDLE), Some("carp"), None, NARROW);
}
