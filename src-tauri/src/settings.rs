//! User settings: settings.json in the app config directory, written with
//! the defaults on first run so it can be edited.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use papers_core::Matching;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// the .bib files of the library (~ for the home directory); on first
    /// run, the default library of papers if there is one
    pub bibtex: Vec<PathBuf>,
    /// global shortcut that shows the panel: Alt+Space by default on macOS,
    /// Ctrl+Alt+Space elsewhere, where Alt+Space opens the window menu (set
    /// in the settings window, it applies at once; in the file, at the next start)
    pub hotkey: String,
    /// global shortcut that finds the selected text (the primary selection)
    /// in the panel; on macOS, the Services menu does it
    #[cfg(target_os = "linux")]
    pub selection_hotkey: String,
    /// "word-start" (default), or "inside-words" to match inside words too
    pub matching: Matching,
    /// a bundled CSL style, or the path of a .csl file
    pub citation_style: String,
    /// look for updates at start and twice a day
    pub check_updates: bool,
    /// what the main window shows after a paper is closed
    pub after_close: AfterClose,
    /// the papers command, which writes a papers library (~ for the home
    /// directory); empty: the one the login shell finds
    pub papers: PathBuf,
}

/// After a paper is closed in the main window: the library, or the next
/// paper open.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AfterClose {
    #[default]
    Library,
    Next,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            bibtex: Vec::new(),
            hotkey: if cfg!(target_os = "macos") {
                "Alt+Space"
            } else {
                "Ctrl+Alt+Space"
            }
            .into(),
            #[cfg(target_os = "linux")]
            selection_hotkey: "Ctrl+Alt+Shift+Space".into(),
            matching: Matching::default(),
            citation_style: crate::styles::DEFAULT.into(),
            check_updates: true,
            after_close: AfterClose::default(),
            papers: PathBuf::new(),
        }
    }
}

impl Settings {
    /// The settings of the file, or the defaults; without .bib files, those
    /// of the default library of papers are taken, and written in the file.
    pub fn load(path: &Path) -> Result<Settings, String> {
        let mut settings: Settings = match fs::read_to_string(path) {
            Ok(text) => {
                serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?
            }
            Err(e) if e.kind() == ErrorKind::NotFound => Settings::default(),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        if settings.bibtex.is_empty() {
            settings.bibtex = papers_library().into_iter().collect();
            settings.save(path)?;
        }
        Ok(settings)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let error = |e: std::io::Error| format!("{}: {e}", path.display());
        fs::create_dir_all(path.parent().unwrap()).map_err(error)?;
        let text = serde_json::to_string_pretty(self).unwrap();
        fs::write(path, text + "\n").map_err(error)
    }

    /// The .bib files, ~ expanded.
    pub fn bibs(&self) -> Vec<PathBuf> {
        self.bibtex
            .iter()
            .map(|p| expand(&p.to_string_lossy()))
            .collect()
    }

    /// The papers command, ~ expanded; empty: to look for.
    pub fn papers_command(&self) -> PathBuf {
        expand(&self.papers.to_string_lossy())
    }
}

/// A path, with ~ for the home directory.
pub fn expand(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), std::env::home_dir()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(path),
    }
}

/// The .bib file of the default library of papers, from its configuration:
/// the global one names the library, whose own one names the .bib file.
fn papers_library() -> Option<PathBuf> {
    let read = |path: PathBuf| -> Option<Value> {
        serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
    };
    let global = read(config_dir()?.join("papers/config.json"))?;
    let library = expand(global["default"].as_str()?);
    let config = read(library.join(".papers/config.json"))?;
    let bib = library.join(config["bibtex"].as_str()?);
    bib.exists().then_some(bib)
}

/// Where papers keeps its global configuration (platformdirs' user_config_dir).
fn config_dir() -> Option<PathBuf> {
    let home = std::env::home_dir()?;
    if cfg!(target_os = "macos") {
        Some(home.join("Library/Application Support"))
    } else {
        Some(
            std::env::var_os("XDG_CONFIG_HOME").map_or_else(|| home.join(".config"), PathBuf::from),
        )
    }
}
