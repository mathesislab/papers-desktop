//! User settings: settings.json in the app config directory, written with
//! the defaults on first run so it can be edited.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use papers_core::{Matching, Papers};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// the papers executable; found on the shell PATH when missing
    pub papers: Option<PathBuf>,
    /// global shortcut that shows the panel (read at startup only)
    pub hotkey: String,
    /// "word-start" (default), or "inside-words" to match inside words too
    pub matching: Matching,
    /// a bundled CSL style, or the path of a .csl file
    pub citation_style: String,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            papers: None,
            hotkey: "Alt+Space".into(),
            matching: Matching::default(),
            citation_style: crate::styles::DEFAULT.into(),
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Result<Settings, String> {
        let error = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
        let mut settings: Settings = match fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| error(&e))?,
            Err(e) if e.kind() == ErrorKind::NotFound => Settings::default(),
            Err(e) => return Err(error(&e)),
        };
        if settings.papers.is_none() {
            settings.papers = Papers::locate();
            if !path.exists() {
                fs::create_dir_all(path.parent().unwrap()).map_err(|e| error(&e))?;
                let text = serde_json::to_string_pretty(&settings).unwrap();
                fs::write(path, text + "\n").map_err(|e| error(&e))?;
            }
        }
        Ok(settings)
    }

    pub fn papers(&self) -> Papers {
        Papers::new(self.papers.clone().unwrap_or_else(|| "papers".into()))
    }
}
