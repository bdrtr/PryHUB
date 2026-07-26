//! `ug2` — read Need for Speed: Underground 2 car assets from the command line.
//!
//! One entry point over the whole parser: summarise a car, list the parts a configuration
//! selects, print the raw chunk tree of any asset file, and export geometry + textures to
//! formats other tools read (OBJ/MTL + PNG).
//!
//! It ships no game data and reads only what you point it at — you must own your copy of the
//! game. Build it with `--features tools`:
//!
//! ```text
//! cargo run -p gizmo-nfs --features tools --bin ug2 -- info  "$NFSU2_ROOT/CARS/240SX"
//! cargo run -p gizmo-nfs --features tools --bin ug2 -- export "$NFSU2_ROOT/CARS/240SX" -o out/
//! ```

/// Print a line to stdout, treating a closed pipe as "the reader has seen enough" rather than a
/// panic. `ug2 parts CARS/240SX | head` is ordinary use, and Rust's `println!` panics on EPIPE.
///
/// Declared before the modules below so they all see it (macros are textually scoped).
macro_rules! outln {
    () => { outln!("") };
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        if writeln!(std::io::stdout(), $($arg)*).is_err() {
            std::process::exit(0);
        }
    }};
}

mod diff;
mod dump;
mod export;
mod fields;
mod globalb;
mod import;
mod info;
mod parts;
mod paths;
mod poke;
mod probe;
mod profile;
mod replace;
mod textures;

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;

/// Read Need for Speed: Underground 2 car assets — inspect them, or export them to OBJ/PNG.
#[derive(Parser)]
#[command(name = "ug2", version, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// A car configuration, shared by the commands that assemble one.
#[derive(clap::Args, Clone, Copy)]
struct ConfigArgs {
    /// Body kit `KIT##` (front + rear bumper + side skirt); 0 = stock.
    #[arg(long, default_value_t = 0, value_name = "N")]
    kit: u8,
    /// Hood design `STYLE##`; 0 = stock.
    #[arg(long, default_value_t = 0, value_name = "N")]
    hood: u8,
    /// Head/tail-light design `STYLE##`; 0 = stock.
    #[arg(long, default_value_t = 0, value_name = "N")]
    light: u8,
    /// Widebody kit `KITW##` (body + doors); 0 = stock.
    #[arg(long, default_value_t = 0, value_name = "N")]
    wide: u8,
}

impl From<ConfigArgs> for gizmo_nfs::CarConfig {
    fn from(a: ConfigArgs) -> Self {
        Self { body_kit: a.kit, hood_style: a.hood, light_style: a.light, widebody: a.wide }
    }
}

#[derive(Subcommand)]
enum Command {
    /// Summarise a car: its parts, the variants it ships, its dimensions and wheel mounts.
    Info {
        /// A car directory (`CARS/240SX`) or its `GEOMETRY.BIN`.
        car: PathBuf,
        #[command(flatten)]
        config: ConfigArgs,
    },
    /// List a car's parts, grouped by customization namespace.
    Parts {
        /// A car directory or its `GEOMETRY.BIN`.
        car: PathBuf,
        /// Show only the parts the given configuration selects, with their material group.
        #[arg(long)]
        selected: bool,
        #[command(flatten)]
        config: ConfigArgs,
    },
    /// Read a chunk back as labelled fields — the same model PryHUB's inspector shows.
    Fields {
        /// A car directory or its `GEOMETRY.BIN`.
        car: PathBuf,
        /// A single chunk, by the hex offset of its header (`--at 0x1b8`).
        #[arg(long, value_name = "OFFSET")]
        at: Option<String>,
        /// Otherwise: every chunk whose type name or id contains this.
        #[arg(long, value_name = "SUBSTR")]
        filter: Option<String>,
    },
    /// Print the chunk tree of any asset file (or list a BIGF/VIV archive).
    Dump {
        /// The file to inspect; any codec is detected and decompressed first.
        file: PathBuf,
        /// How deep to print.
        #[arg(long, default_value_t = 64, value_name = "N")]
        max_depth: u32,
        /// Leading payload bytes to show as hex per leaf.
        #[arg(long, default_value_t = 16, value_name = "N")]
        hex: usize,
    },
    /// List a car's textures and how its material runs map onto them.
    Textures {
        /// A car directory or its `GEOMETRY.BIN`.
        car: PathBuf,
        /// Only show parts whose name contains this.
        #[arg(long, value_name = "SUBSTR")]
        filter: Option<String>,
    },
    /// Put an edited model back into a `GEOMETRY.BIN` — the far end of `export`.
    ///
    /// Reads an OBJ, undoes the V flip and the part's placement, and writes the mesh into a solid.
    /// Which solid is the one thing the model file cannot say: 24.8% of the install's solids share
    /// their name with another in the same file, so a name is accepted only when it names one and
    /// the candidates are otherwise listed by header offset.
    Import {
        /// The car's `GEOMETRY.BIN`.
        file: PathBuf,
        /// The model to read.
        #[arg(long, value_name = "FILE")]
        obj: PathBuf,
        /// Which part: its name, or its solid's header offset as `0x…`. Not needed when the model
        /// holds exactly one mesh and its name matches.
        #[arg(long, value_name = "NAME|0xOFFSET")]
        part: Option<String>,
        /// Where to write the edited car. Never the input unless `--force`.
        #[arg(short, long, value_name = "FILE")]
        out: PathBuf,
        /// Allow `-o` to be the file being read.
        #[arg(long)]
        force: bool,
        /// Which way up the model is: `file` (Z up, what `export` writes) or `yup` (Blender's
        /// default `Forward -Z, Up Y`). Worked out from the file when not given.
        #[arg(long, value_name = "file|yup")]
        axes: Option<String>,
    },
    /// Put a PNG back into a texture pack — the one command here that writes an asset file.
    ///
    /// The replacement keeps the texture's own dimensions and its own pixel format: they are
    /// recorded in the blob's embedded header, which the pack's descriptor locates as a distance
    /// from the end, so a differently-sized image would move it without saying so.
    Replace {
        /// The pack to edit (`TEXTURES.BIN` / `VINYLS.BIN`), or a car directory to find one in.
        pack: PathBuf,
        /// Which texture: its `DebugName`, or its hash as `0x…`.
        #[arg(long, value_name = "NAME|0xHASH")]
        texture: String,
        /// The image to put in.
        #[arg(long, value_name = "FILE")]
        png: PathBuf,
        /// Where to write the edited pack. Never the input unless `--force`.
        #[arg(short, long, value_name = "FILE")]
        out: PathBuf,
        /// Allow `-o` to be the file being read.
        #[arg(long)]
        force: bool,
    },
    /// Change one number in one car's handling record, so the game can say what it was.
    ///
    /// 222 of the record's 4-byte lanes vary per car and nothing here reads them. Their meaning is
    /// not in the file, and guessing produces a label that is wrong; the experiment that settles it
    /// is to change one and drive. Without `--set` it only reports what every car has there.
    Poke {
        /// `GLOBAL/GLOBALB.BUN`, or a car directory / game root to find it from.
        path: PathBuf,
        /// Which car's record, by the name it carries.
        #[arg(long, value_name = "NAME")]
        car: String,
        /// The lane, as a byte offset into the 2192-byte record.
        #[arg(long, value_name = "0xNNN", value_parser = parse_offset)]
        at: usize,
        /// The value to write. Omit to only look.
        #[arg(long, value_name = "FLOAT")]
        set: Option<f32>,
        /// Where to write the edited bundle.
        #[arg(short, long, value_name = "FILE", default_value = "GLOBALB.BUN")]
        out: PathBuf,
    },
    /// Read a player profile: which performance products a car has fitted, and their totals.
    Profile {
        /// The profile file, or the directory holding it.
        path: PathBuf,
    },
    /// Print the wheel mounts, radius and mass recorded in `GLOBALB.BUN`, and with `--parts` the
    /// `CarParts` tables and the game's paint palette.
    Globalb {
        /// `GLOBAL/GLOBALB.BUN`, or a car directory / game root to find it from.
        path: PathBuf,
        /// Only show cars whose name contains this.
        #[arg(long, value_name = "SUBSTR")]
        filter: Option<String>,
        /// Show the `CarParts` tables instead: part names, attribute keys, and the paint palette.
        #[arg(long)]
        parts: bool,
        /// Show what each record says about how the car drives: rpm limits, the torque curve, and
        /// the four gearboxes (stock plus the three upgrade levels).
        #[arg(long)]
        handling: bool,
    },
    /// Probe a car's raw solids: vertex/triangle counts, buffer sizes, mesh-header words.
    ///
    /// The reverse-engineering view — this is what tells a car whose vertex layout the parser
    /// does not decode from one it silently skips.
    Probe {
        /// A car directory or its `GEOMETRY.BIN`.
        car: PathBuf,
        /// Only show solids whose name contains this.
        #[arg(long, value_name = "SUBSTR")]
        filter: Option<String>,
        /// Also classify each part's local matrix (placement / baked pose / reflection).
        #[arg(long)]
        matrices: bool,
    },
    /// Compare two asset files chunk by chunk.
    Diff {
        /// The file to read as the "before".
        left: PathBuf,
        /// The file to read as the "after".
        right: PathBuf,
        /// Also list the chunks that are the same.
        #[arg(long)]
        all: bool,
        /// Stop after this many lines.
        #[arg(long, value_name = "N", default_value_t = 200)]
        max: usize,
    },
    /// Export a car to glTF (`.glb`) and OBJ + MTL, with its textures as PNG.
    Export {
        /// A car directory or its `GEOMETRY.BIN`.
        car: PathBuf,
        /// Directory to write into (created if missing).
        #[arg(short, long, value_name = "DIR")]
        out: PathBuf,
        #[command(flatten)]
        config: ConfigArgs,
        /// Export every part in the file, not just the selected configuration.
        #[arg(long)]
        all: bool,
        /// Skip textures (geometry only).
        #[arg(long)]
        no_textures: bool,
        /// Threads to export a folder of cars on (default: cores, capped at 8).
        #[arg(long, value_name = "N")]
        jobs: Option<usize>,
        /// Write only one of the two formats.
        #[arg(long, value_name = "FMT", value_parser = ["glb", "obj", "both"], default_value = "both")]
        format: String,
    },
}

/// `0x284` or `644` — the record's lanes are named in hex far more often than not.
fn parse_offset(s: &str) -> std::result::Result<usize, String> {
    match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(hex) => usize::from_str_radix(hex, 16).map_err(|e| e.to_string()),
        None => s.parse().map_err(|e: std::num::ParseIntError| e.to_string()),
    }
}

/// `GLOBALB.BUN` itself, or the bundle reachable from a car directory or a game root.
fn gizmo_nfs_globalb_path(path: &std::path::Path) -> PathBuf {
    if path.is_file() {
        return path.to_path_buf();
    }
    for candidate in [path.join("GLOBAL/GLOBALB.BUN"), path.join("GLOBALB.BUN")] {
        if candidate.is_file() {
            return candidate;
        }
    }
    paths::globalb_beside(path).unwrap_or_else(|| path.to_path_buf())
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Info { car, config } => info::run(&car, config.into()),
        Command::Parts { car, selected, config } => parts::run(&car, selected, config.into()),
        Command::Fields { car, at, filter } => fields::run(&car, at, filter.as_deref()),
        Command::Dump { file, max_depth, hex } => dump::run(&file, max_depth, hex),
        Command::Textures { car, filter } => textures::run(&car, filter.as_deref()),
        Command::Globalb { path, filter, parts, handling } => {
            globalb::run(&path, filter.as_deref(), parts, handling)
        }
        Command::Profile { path } => profile::run(&path),
        Command::Poke { path, car, at, set, out } => {
            let bundle = gizmo_nfs_globalb_path(&path);
            poke::run(&bundle, &car, at, set, &out)
        }
        Command::Replace { pack, texture, png, out, force } => {
            replace::run(&pack, &texture, &png, &out, force)
        }
        Command::Import { file, obj, part, out, force, axes } => {
            let axes = match axes.as_deref() {
                None => Ok(None),
                Some("file") | Some("zup") => Ok(Some(import::Axes::File)),
                Some("yup") | Some("blender") => Ok(Some(import::Axes::YUp)),
                Some(other) => Err(format!("{other}: axes are `file` or `yup`")),
            };
            axes.and_then(|a| import::run(&file, &obj, part.as_deref(), &out, force, a))
        }
        Command::Probe { car, filter, matrices } => probe::run(&car, filter.as_deref(), matrices),
        Command::Diff { left, right, all, max } => diff::run(&left, &right, all, max),
        Command::Export { car, out, config, all, no_textures, format, jobs } => {
            export::run(&car, &out, config.into(), all, !no_textures, &format, jobs)
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ug2: {e}");
            ExitCode::FAILURE
        }
    }
}
