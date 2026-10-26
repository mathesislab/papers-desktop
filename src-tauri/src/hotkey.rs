//! The global shortcuts of the settings, each with what it does.

use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use crate::settings::Settings;

#[derive(Debug, Clone, Copy)]
enum Hotkey {
    /// show or hide the panel
    Panel,
}

impl Hotkey {
    const ALL: &[Hotkey] = &[Hotkey::Panel];

    /// Its keys in the settings.
    fn keys(self, settings: &Settings) -> &str {
        match self {
            Hotkey::Panel => &settings.hotkey,
        }
    }

    fn run(self, app: &AppHandle) {
        match self {
            Hotkey::Panel => crate::toggle(app),
        }
    }

    fn register(self, app: &AppHandle, keys: &str) -> Result<(), String> {
        app.global_shortcut()
            .on_shortcut(keys, move |app, _, event| {
                if event.state == ShortcutState::Pressed {
                    self.run(app);
                }
            })
            .map_err(|e| e.to_string())
    }

    /// Take new keys in place of the old ones, which stay if the new ones
    /// cannot be had (another app has them, or they are no shortcut).
    fn replace(self, app: &AppHandle, old: &str, new: &str) -> Result<(), String> {
        let _ = app.global_shortcut().unregister(old);
        if let Err(e) = self.register(app, new) {
            let _ = self.register(app, old);
            return Err(format!("{new} cannot be the hotkey: {e}"));
        }
        // the menu shows the hotkey of the panel
        match self {
            Hotkey::Panel => crate::show_hotkey(app, new),
        }
        Ok(())
    }
}

/// Take the hotkeys of the settings, at start.
pub fn setup(app: &AppHandle, settings: &Settings) {
    for &hotkey in Hotkey::ALL {
        let keys = hotkey.keys(settings);
        if let Err(e) = hotkey.register(app, keys) {
            eprintln!("papers-desktop: cannot register the hotkey {keys}: {e}");
        }
    }
}

/// Take the hotkeys of new settings in place of the old ones.
pub fn apply(app: &AppHandle, old: &Settings, new: &Settings) -> Result<(), String> {
    for &hotkey in Hotkey::ALL {
        let (old, new) = (hotkey.keys(old), hotkey.keys(new));
        if old != new {
            hotkey.replace(app, old, new)?;
        }
    }
    Ok(())
}
