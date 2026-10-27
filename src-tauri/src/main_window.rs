//! The main window: the library and the papers open. A normal window, in the
//! Dock and ⌘Tab while it is open; the app is in the menu bar only otherwise.

use std::fs;
use std::path::PathBuf;

use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

pub const MAIN: &str = "main";

/// Show the main window. It is made hidden the first time: its page shows
/// it once drawn.
pub fn show(app: &AppHandle) {
    crate::hide_window(app);
    match app.get_webview_window(MAIN) {
        Some(window) => {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        }
        None => make(app, "library"),
    }
}

/// Show the main window with a paper open in it: told to its page, or in
/// the URL of a window made for it ("library#ID").
pub fn open_paper(app: &AppHandle, id: &str) {
    if app.get_webview_window(MAIN).is_some() {
        show(app);
        let _ = app.emit_to(MAIN, "open", id);
    } else {
        crate::hide_window(app);
        make(app, &format!("library#{}", encode(id)));
    }
}

/// An id in a URL: a key can have a slash ("10.1002/xyz") or a hash ("key#2").
fn encode(id: &str) -> String {
    id.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn make(app: &AppHandle, url: &str) {
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    let window = WebviewWindowBuilder::new(app, MAIN, WebviewUrl::App(url.into()))
        .title("Papers")
        .inner_size(1200.0, 780.0)
        .min_inner_size(760.0, 420.0)
        .center()
        .visible(false)
        .build();
    if let Err(e) = window {
        eprintln!("papers-desktop: cannot open the main window: {e}");
    }
}

/// Back to the menu bar only, once the main window is closed.
pub fn closed(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

/// The layout of the main window, kept between sessions as its page gives
/// it: the widths of the panes, whether the details show.
pub struct Layout {
    path: PathBuf,
}

impl Layout {
    pub fn new(path: PathBuf) -> Layout {
        Layout { path }
    }

    pub fn load(&self) -> Option<Value> {
        let text = fs::read_to_string(&self.path).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Losing the layout is no error worth more than a line in the log.
    pub fn save(&self, layout: &Value) {
        let saved = fs::create_dir_all(self.path.parent().unwrap())
            .and_then(|()| fs::write(&self.path, serde_json::to_string_pretty(layout).unwrap()));
        if let Err(e) = saved {
            eprintln!("papers-desktop: cannot save {}: {e}", self.path.display());
        }
    }
}
