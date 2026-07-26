//! PryHUB — a Need for Speed: Underground 2 asset inspector.
//!
//! Open a `GEOMETRY.BIN` (or any chunked NFSU2 file), walk its chunk tree, read the bytes behind
//! any node, see the parsed fields beside them — and be told what looks wrong. It ships no game
//! data and only reads what you point it at.
//!
//! The interface is a native port of the PryHUB design: five regions (tree · centre tabs ·
//! inspector · log · status bar) over one open file, with the selection shared by all of them.

mod app;
mod chrome;
mod doc;
mod export;
mod gpu;
mod i18n;
mod jobs;
mod logging;
mod logo;
mod names;
mod panels;
mod picker;
mod replace;
mod screens;
mod settings;
mod shot;
mod theme;
mod widget;

fn main() -> eframe::Result {
    // Before anything else, so a failure during setup has somewhere to go.
    logging::install();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1360.0, 840.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title("PryHUB — nfsu2"),
        ..Default::default()
    };
    eframe::run_native(
        "pryhub",
        options,
        Box::new(|cc| {
            let chosen = settings::Settings::load();
            theme::install_fonts(&cc.egui_ctx);
            theme::apply(&cc.egui_ctx, chosen.density);
            // A path on the command line opens straight into the workspace, so the tool can be
            // wired into a shell loop the way `ug2` is.
            // `pryhub <file> [--shot out.png]` — the flag draws a few frames, writes the window
            // as a PNG and exits, which is how the interface gets checked on this machine.
            let args: Vec<String> = std::env::args().skip(1).collect();
            let shot = args.iter().position(|a| a == "--shot").and_then(|i| args.get(i + 1)).cloned();
            let screen = args.iter().position(|a| a == "--screen").and_then(|i| args.get(i + 1)).cloned();
            let tab = args.iter().position(|a| a == "--tab").and_then(|i| args.get(i + 1)).cloned();
            // `--select <offset>` (hex or decimal) preselects a chunk, so a screenshot can show a
            // screen reading something other than the root.
            let select = args
                .iter()
                .position(|a| a == "--select")
                .and_then(|i| args.get(i + 1))
                .and_then(|v| match v.strip_prefix("0x") {
                    Some(hex) => usize::from_str_radix(hex, 16).ok(),
                    None => v.parse().ok(),
                });
            // `--compare <file>` loads the compare screen's other side, which is also the only way
            // to put it in a screenshot on this machine.
            let compare = args
                .iter()
                .position(|a| a == "--compare")
                .and_then(|i| args.get(i + 1))
                .cloned();
            // `--stage <png>` stages an image against the first texture in the pack, once it has
            // decoded. The same reason `--select` and `--compare` exist: staging is a click and a
            // desktop file chooser, and neither is available to a screenshot on a machine whose
            // compositor will not hand out a screen grab.
            let stage = args
                .iter()
                .position(|a| a == "--stage")
                .and_then(|i| args.get(i + 1))
                .cloned();
            let file = args.first().filter(|a| !a.starts_with("--")).cloned();
            let mut app = app::PryHub::new(&cc.egui_ctx, chosen, file, shot, screen);
            if let Some(tab) = tab {
                app.tab = match tab.as_str() {
                    "3d" => app::Tab::ThreeD,
                    "tex" | "texture" | "doku" => app::Tab::Texture,
                    "asm" | "assembly" | "montaj" => app::Tab::Assembly,
                    _ => app::Tab::Hex,
                };
            }
            if let Some(offset) = select {
                // Applied when the parse lands, not now: there is no document to select in yet.
                app.pending_selection = Some(offset);
            }
            if let Some(png) = stage {
                // Applied when the *pack* lands, which is later still: staging needs the texture's
                // dimensions to check the image against.
                app.pending_stage = Some(std::path::PathBuf::from(png));
            }
            if let Some(other) = compare {
                app.open_other(std::path::Path::new(&other));
            }
            app.render_state = cc.wgpu_render_state.clone();
            Ok(Box::new(app))
        }),
    )
}
