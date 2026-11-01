//! The switches of the main window (`Toggle`): its Feeds and Search tabs shown, an item's details
//! beside the list. The settings keep them (`Toggles`); they are set from the settings window and
//! the Window menu, the windows are told at once ([`EVENT`]) and the menu follows.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::AppState;
use crate::settings::{Settings, Toggle, Toggles};

/// A switch set, to every window: which, and whether on.
pub const EVENT: &str = "toggle";

#[derive(Clone, Serialize)]
struct Toggled {
    toggle: Toggle,
    on: bool,
}

/// The switches as the settings keep them.
pub fn toggles(app: &AppHandle) -> Result<Toggles, String> {
    Ok(Settings::load(&app.state::<AppState>().settings_path)?
        .elsewhere
        .toggles)
}

/// Whether a switch is on.
#[tauri::command]
pub fn toggle(app: AppHandle, toggle: Toggle) -> Result<bool, String> {
    Ok(toggles(&app)?.get(toggle))
}

/// A switch set, kept in the settings: the windows are told, the menu follows.
#[tauri::command]
pub fn set_toggle(app: AppHandle, toggle: Toggle, on: bool) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut settings = Settings::load(&state.settings_path)?;
    settings.elsewhere.toggles.set(toggle, on);
    settings.save(&state.settings_path)?;
    #[cfg(target_os = "macos")]
    crate::menu::sync(&app, &settings.elsewhere.toggles);
    let _ = app.emit(EVENT, Toggled { toggle, on });
    Ok(())
}
