//! Diagnostics — for whoever is debugging the tool, which is a different audience from the log panel.
//!
//! The two are deliberately separate and neither should grow into the other:
//!
//! * **The log panel** ([`crate::doc::Note`]) is what the *file* said: a chunk that would not parse,
//!   a solid that yielded no part, a rule that found something, where an export went. It is part of
//!   the interface, it is written in the interface's language, and every line names the chunk it is
//!   about so it can be clicked.
//! * **This** is what the *program* did: which job started and how long it took, which frame took
//!   too long, which texture cache missed, what the GPU backend had to say. Nobody using the tool
//!   should ever have to see it, and it goes to stderr where a terminal or a bug report can pick it
//!   up.
//!
//! Levels come from `PRYHUB_LOG`, in the shape `PRYHUB_LOG=info` or `PRYHUB_LOG=warn,jobs=debug`
//! — a default, then per-target overrides. Default is `warn`: quiet unless something is wrong.
//!
//! # Why the `log` facade rather than an in-house macro
//!
//! `log` is the interface every crate in this dependency tree already speaks. Installing a sink for
//! it means `wgpu`, `winit` and `eframe` diagnostics arrive in the same stream at the same levels —
//! which is exactly what is wanted the day a GPU refuses a surface, and something an in-house macro
//! could never give. The parser is the other way round: it takes no logging dependency at all and
//! *returns* its findings, so it stays usable from anywhere and testable without a logger.

use std::io::Write as _;
use std::time::Instant;

/// The sink: a level, some per-target overrides, and the moment the program started.
struct Sink {
    default: log::LevelFilter,
    targets: Vec<(String, log::LevelFilter)>,
    start: Instant,
}

impl log::Log for Sink {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= self.level_for(metadata.target())
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        // Seconds since start rather than a wall clock: what matters in a trace is how far apart two
        // lines are, and a date on every line of a frame trace is noise.
        let at = self.start.elapsed().as_secs_f64();
        let level = match record.level() {
            log::Level::Error => "ERROR",
            log::Level::Warn => "WARN ",
            log::Level::Info => "INFO ",
            log::Level::Debug => "DEBUG",
            log::Level::Trace => "TRACE",
        };
        // Locked once per line: two threads writing a line each must not interleave halves of them.
        let mut err = std::io::stderr().lock();
        let _ = writeln!(err, "{at:8.3} {level} {} · {}", record.target(), record.args());
    }

    fn flush(&self) {
        let _ = std::io::stderr().flush();
    }
}

impl Sink {
    /// The level for a target: the longest matching override, else the default. Longest-match so
    /// `PRYHUB_LOG=warn,jobs=debug,jobs::export=trace` does what it looks like.
    fn level_for(&self, target: &str) -> log::LevelFilter {
        self.targets
            .iter()
            .filter(|(name, _)| target == name || target.starts_with(&format!("{name}::")))
            .max_by_key(|(name, _)| name.len())
            .map_or(self.default, |(_, level)| *level)
    }
}

/// Read `PRYHUB_LOG` and install the sink. Called once, before anything else runs.
///
/// A bad filter is not worth refusing to start over: an unparsable level is reported on stderr and
/// the default is kept.
pub fn install() {
    let spec = std::env::var("PRYHUB_LOG").unwrap_or_default();
    let mut default = log::LevelFilter::Warn;
    let mut targets = Vec::new();
    for part in spec.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        match part.split_once('=') {
            None => match parse_level(part) {
                Some(level) => default = level,
                None => eprintln!("pryhub: PRYHUB_LOG: {part:?} is not a level"),
            },
            Some((target, level)) => match parse_level(level) {
                Some(level) => targets.push((target.to_string(), level)),
                None => eprintln!("pryhub: PRYHUB_LOG: {level:?} is not a level"),
            },
        }
    }
    // The facade needs the loudest level anyone asked for, or it filters the record out before the
    // sink ever sees which target it was for.
    let loudest = targets.iter().map(|(_, l)| *l).chain([default]).max().unwrap_or(default);
    let sink = Sink { default, targets, start: Instant::now() };
    if log::set_boxed_logger(Box::new(sink)).is_ok() {
        log::set_max_level(loudest);
    }
}

fn parse_level(s: &str) -> Option<log::LevelFilter> {
    match s.trim().to_ascii_lowercase().as_str() {
        "off" => Some(log::LevelFilter::Off),
        "error" => Some(log::LevelFilter::Error),
        "warn" | "warning" => Some(log::LevelFilter::Warn),
        "info" => Some(log::LevelFilter::Info),
        "debug" => Some(log::LevelFilter::Debug),
        "trace" => Some(log::LevelFilter::Trace),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use log::LevelFilter as L;

    fn sink(default: L, targets: &[(&str, L)]) -> Sink {
        Sink {
            default,
            targets: targets.iter().map(|(t, l)| ((*t).to_string(), *l)).collect(),
            start: Instant::now(),
        }
    }

    #[test]
    fn a_target_without_an_override_gets_the_default() {
        let s = sink(L::Warn, &[("jobs", L::Debug)]);
        assert_eq!(s.level_for("export"), L::Warn);
        assert_eq!(s.level_for("jobs"), L::Debug);
    }

    /// `jobs=debug` covers `jobs::export`, and a more specific override wins over it — otherwise
    /// turning one subsystem up would mean turning its parent up too.
    #[test]
    fn the_most_specific_override_wins() {
        let s = sink(L::Warn, &[("jobs", L::Debug), ("jobs::export", L::Trace)]);
        assert_eq!(s.level_for("jobs::export"), L::Trace);
        assert_eq!(s.level_for("jobs::decode"), L::Debug);
        // A target that merely starts with the same letters is not a child.
        assert_eq!(s.level_for("jobsomething"), L::Warn);
    }

    #[test]
    fn levels_are_read_the_way_people_write_them() {
        assert_eq!(parse_level("TRACE"), Some(L::Trace));
        assert_eq!(parse_level(" warning "), Some(L::Warn));
        assert_eq!(parse_level("off"), Some(L::Off));
        assert_eq!(parse_level("loud"), None);
    }
}
