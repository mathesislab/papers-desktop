//! The text size: the zoom of the web views of all windows, as a browser
//! zooms a page, so that everything scales and the layout keeps its
//! proportions. One level, shared by the windows, kept in the settings and
//! applied at once; the windows of a fixed size (the panel, the settings)
//! are scaled with it. The native PDF view of macOS has its own zoom.

use tauri::{AppHandle, Emitter, LogicalSize, Manager, Monitor, WebviewWindow};

use crate::settings::{Settings, TextSize};
use crate::{AppState, SETTINGS, SETTINGS_HEIGHT, SETTINGS_WIDTH};

/// The zoom of the web views now.
pub fn factor(app: &AppHandle) -> f64 {
    app.state::<AppState>().text_size.read().unwrap().factor()
}

/// A window just made, zoomed as the others.
pub fn prepare(app: &AppHandle, window: &WebviewWindow) {
    let _ = window.set_zoom(factor(app));
}

/// A size in logical pixels, grown with the zoom but within the screen.
pub fn scaled(monitor: Option<Monitor>, width: f64, height: f64, factor: f64) -> LogicalSize<f64> {
    let (width, height) = (width * factor, height * factor);
    match monitor {
        Some(m) => {
            let screen = m.size().to_logical::<f64>(m.scale_factor());
            LogicalSize::new(width.min(0.95 * screen.width), height.min(0.9 * screen.height))
        }
        None => LogicalSize::new(width, height),
    }
}

/// The screen of a window.
pub fn monitor(window: &WebviewWindow) -> Option<Monitor> {
    window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten())
}

/// Take a size at once: kept in the state, applied to the windows open, told
/// to their pages.
pub fn apply(app: &AppHandle, size: TextSize) {
    *app.state::<AppState>().text_size.write().unwrap() = size;
    for window in app.webview_windows().values() {
        let _ = window.set_zoom(size.factor());
    }
    let _ = crate::resize_panel(app);
    if let Some(window) = app.get_webview_window(SETTINGS) {
        let size = scaled(monitor(&window), SETTINGS_WIDTH, SETTINGS_HEIGHT, size.factor());
        let _ = window.set_size(size);
    }
    let _ = app.emit("text-size", size);
}

/// A level up or down (`steps`), or the default (0), kept in the settings.
pub fn step(app: &AppHandle, steps: i32) -> Result<(), String> {
    let state = app.state::<AppState>();
    let old = *state.text_size.read().unwrap();
    let new = if steps == 0 { TextSize::default() } else { old.step(steps) };
    if new != old {
        let mut settings = Settings::load(&state.settings_path)?;
        settings.text_size = new;
        settings.save(&state.settings_path)?;
        apply(app, new);
    }
    Ok(())
}

/// ⌘+ and ⌘- of the panel and the settings window; ⌘0, the default.
#[tauri::command]
pub fn text_size_step(app: AppHandle, steps: i32) -> Result<(), String> {
    step(&app, steps)
}
