//! What the user chose, and where it is kept.
//!
//! Two things live here — the interface's language and its size — and they are the two the design
//! always had switches for. What is new is that they survive a restart: a settings panel whose
//! choices are forgotten the moment the window closes is a switch, not a setting.
//!
//! The file is `$XDG_CONFIG_HOME/pryhub/settings.tsv`, one `key<TAB>value` per line, beside the
//! hash dictionary and in the same spirit: plain text, hand-editable, greppable, and readable by a
//! person who has never seen this program's source.
//!
//! Defaults, when there is no file: **English**, **medium**. English because the tool is for anyone
//! who owns the game (the source's comments stay in the project's own language, but a stranger's
//! first window should not be in a language they did not choose); medium because the design's own
//! compact setting is right for someone who already knows the tool and wrong for a first look.

use crate::i18n::Lang;
use crate::theme::Density;
use std::path::PathBuf;

/// The persisted choices.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Settings {
    pub lang: Lang,
    pub density: Density,
}

impl Default for Settings {
    fn default() -> Self {
        Self { lang: Lang::En, density: Density::Balanced }
    }
}

impl Settings {
    /// Read the file, falling back to the defaults for anything missing or unreadable — a settings
    /// file is not worth refusing to start over.
    #[must_use]
    pub fn load() -> Self {
        Self::path().map(|p| Self::load_from(&p)).unwrap_or_default()
    }

    /// The same from a given file, which is what the tests read.
    #[must_use]
    pub fn load_from(path: &std::path::Path) -> Self {
        let mut settings = Self::default();
        let Ok(text) = std::fs::read_to_string(path) else { return settings };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('\t') else { continue };
            match (key.trim(), value.trim()) {
                ("language", "tr") => settings.lang = Lang::Tr,
                ("language", "en") => settings.lang = Lang::En,
                ("size", "small") => settings.density = Density::Compact,
                ("size", "medium") => settings.density = Density::Balanced,
                ("size", "large") => settings.density = Density::Roomy,
                _ => log::debug!(target: "settings", "ignoring {line:?}"),
            }
        }
        settings
    }

    /// Where the file lives: `$XDG_CONFIG_HOME/pryhub/settings.tsv`, else `~/.config/…`.
    #[must_use]
    pub fn path() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(base.join("pryhub").join("settings.tsv"))
    }

    /// Write the file. Called when a setting changes, not at exit: a choice that is lost because the
    /// window was closed the wrong way is worse than no setting at all.
    pub fn save(self) {
        let Some(path) = Self::path() else { return };
        if let Some(dir) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(dir) {
                log::warn!(target: "settings", "{}: {e}", dir.display());
                return;
            }
        }
        let text = format!(
            "# PryHUB settings — one <key>\\t<value> per line.\n\
             # language: tr | en          size: small | medium | large\n\
             language\t{}\nsize\t{}\n",
            match self.lang {
                Lang::Tr => "tr",
                Lang::En => "en",
            },
            self.size_key(),
        );
        match std::fs::write(&path, text) {
            Ok(()) => log::debug!(target: "settings", "written to {}", path.display()),
            Err(e) => log::warn!(target: "settings", "{}: {e}", path.display()),
        }
    }

    fn size_key(self) -> &'static str {
        match self.density {
            Density::Compact => "small",
            Density::Balanced => "medium",
            Density::Roomy => "large",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two defaults this exists to establish.
    #[test]
    fn a_fresh_install_is_english_and_medium() {
        let s = Settings::default();
        assert_eq!(s.lang, Lang::En);
        assert_eq!(s.density, Density::Balanced);
        // And a file that does not exist is a fresh install, not an error.
        assert_eq!(Settings::load_from(std::path::Path::new("/nonexistent/settings.tsv")), s);
    }

    #[test]
    fn a_saved_choice_reads_back() {
        let dir = std::env::temp_dir().join(format!("pryhub-settings-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("settings.tsv");
        std::fs::write(&path, "# a note\n\nlanguage\ttr\nsize\tlarge\n").expect("write");

        let read = Settings::load_from(&path);
        assert_eq!(read.lang, Lang::Tr);
        assert_eq!(read.density, Density::Roomy);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A hand-edited file with a typo keeps the rest rather than resetting everything.
    #[test]
    fn an_unknown_value_leaves_that_setting_alone() {
        let dir = std::env::temp_dir().join(format!("pryhub-settings-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("settings.tsv");
        std::fs::write(&path, "language\tklingon\nsize\tsmall\n").expect("write");

        let read = Settings::load_from(&path);
        assert_eq!(read.lang, Lang::En, "the default survives a value nobody understands");
        assert_eq!(read.density, Density::Compact, "the line that did parse still counts");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
