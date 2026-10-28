//! User settings: settings.json in the app config directory, written with
//! the defaults on first run so it can be edited.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use papers_core::Matching;
use papers_rs::fetch::openalex::OpenAlex;
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
    /// the size of the text, as a zoom of every window (see `text_size.rs`)
    pub text_size: TextSize,
    /// what writes the papers libraries: papers-rs, built in, or a papers command
    pub writer: WriterChoice,
    /// the papers command, for `writer` "command" (~ for the home
    /// directory); empty: the one the login shell finds
    pub papers: PathBuf,
    /// an e-mail address sent to doi.org and OpenAlex, which answer faster to
    /// programs that give one; empty (the default): nothing is sent. Never
    /// filled in by the app
    pub email: String,
    /// a key of OpenAlex (free), for more searches a day than without; empty
    /// (the default): none. Sent to OpenAlex only
    pub openalex_key: String,
    /// the tags pinned to the keys 1–9 of the main window (index = key − 1);
    /// set from the sidebar, not in the settings window
    pub pinned_tags: Vec<Option<String>>,
}

/// What writes a papers library: papers-rs, which the app has built in, or a
/// command of the papers standard (papers-cli, the `papers-rs` program, ...).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WriterChoice {
    #[default]
    BuiltIn,
    Command,
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

/// The size of the text, as a level of the zoom of the windows; the default
/// is one step above what the app used before the setting (now "smaller").
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TextSize {
    Smallest,
    Smaller,
    #[default]
    Default,
    Larger,
    Largest,
}

impl TextSize {
    const LEVELS: [TextSize; 5] = [
        TextSize::Smallest,
        TextSize::Smaller,
        TextSize::Default,
        TextSize::Larger,
        TextSize::Largest,
    ];

    /// The zoom of the web views.
    pub fn factor(self) -> f64 {
        match self {
            TextSize::Smallest => 0.9,
            TextSize::Smaller => 1.0,
            TextSize::Default => 1.1,
            TextSize::Larger => 1.25,
            TextSize::Largest => 1.4,
        }
    }

    /// The level `steps` above (below if negative), the first or last at the ends.
    pub fn step(self, steps: i32) -> TextSize {
        let at = Self::LEVELS.iter().position(|&l| l == self).unwrap() as i32;
        Self::LEVELS[(at + steps).clamp(0, 4) as usize]
    }
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
            text_size: TextSize::default(),
            writer: WriterChoice::default(),
            papers: PathBuf::new(),
            email: String::new(),
            openalex_key: String::new(),
            pinned_tags: Vec::new(),
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

    /// The e-mail address to give the services, if one is set.
    pub fn mailto(&self) -> Option<&str> {
        Some(self.email.trim()).filter(|e| !e.is_empty())
    }

    /// How a search on OpenAlex identifies itself: the email and the key,
    /// each when set. For the calls of papers-rs that take it: the lookup of
    /// a free PDF.
    pub fn openalex(&self) -> OpenAlex {
        OpenAlex {
            mailto: self.mailto().map(str::to_string),
            api_key: Some(self.openalex_key.trim())
                .filter(|k| !k.is_empty())
                .map(str::to_string),
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_size_steps_stop_at_the_ends() {
        assert_eq!(TextSize::default(), TextSize::Default);
        assert_eq!(TextSize::Default.step(1), TextSize::Larger);
        assert_eq!(TextSize::Default.step(-2), TextSize::Smallest);
        assert_eq!(TextSize::Largest.step(1), TextSize::Largest);
        assert_eq!(TextSize::Smallest.step(-1), TextSize::Smallest);
        let json = serde_json::to_string(&TextSize::Smallest).unwrap();
        assert_eq!(json, "\"smallest\"");
        // the sizes grow
        assert!(TextSize::LEVELS.windows(2).all(|w| w[0].factor() < w[1].factor()));
    }

    #[test]
    fn no_email_by_default() {
        assert_eq!(Settings::default().mailto(), None);
        // a file from before the setting existed
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings.mailto(), None);
    }

    #[test]
    fn email_trimmed_and_blank_is_none() {
        let with = |email: &str| Settings {
            email: email.into(),
            ..Settings::default()
        };
        assert_eq!(with(" me@example.org ").mailto(), Some("me@example.org"));
        assert_eq!(with("  ").mailto(), None);
    }

    #[test]
    fn openalex_access_from_the_settings() {
        assert_eq!(Settings::default().openalex(), OpenAlex::default());
        let settings = Settings {
            email: "me@example.org".into(),
            openalex_key: " KEY ".into(),
            ..Settings::default()
        };
        assert_eq!(
            settings.openalex(),
            OpenAlex {
                mailto: Some("me@example.org".into()),
                api_key: Some("KEY".into()),
            }
        );
    }
}
