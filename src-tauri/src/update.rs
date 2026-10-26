//! Updates of the app: looked for now and then, installed when asked.

use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::AppState;
use crate::settings::Settings;

/// The first look, a while after the start, then twice a day.
const FIRST: Duration = Duration::from_secs(10);
const EVERY: Duration = Duration::from_secs(12 * 60 * 60);

/// The update found, kept to install it.
#[derive(Default)]
pub struct Updates {
    found: Mutex<Option<Update>>,
}

/// An update, as the settings window shows it.
#[derive(Debug, Clone, Serialize)]
pub struct Available {
    pub version: String,
    pub notes: Option<String>,
}

impl Updates {
    pub fn available(&self) -> Option<Available> {
        self.found.lock().unwrap().as_ref().map(|u| Available {
            version: u.version.clone(),
            notes: u.body.clone(),
        })
    }
}

/// Look for an update, and tell the menu and the settings window about it.
pub async fn check(app: &AppHandle) -> Result<Option<Available>, String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater.check().await.map_err(|e| e.to_string())?;
    let updates = app.state::<Updates>();
    *updates.found.lock().unwrap() = update;
    let available = updates.available();
    crate::show_update(app, available.as_ref());
    let _ = app.emit("update", &available);
    Ok(available)
}

/// Download the update found and install it, telling the progress
/// (bytes so far, and in all), then start the new version.
pub async fn install(app: &AppHandle) -> Result<(), String> {
    let update = app.state::<Updates>().found.lock().unwrap().take();
    let update = update.ok_or("no update to install")?;
    let mut downloaded = 0;
    let progress = |chunk: usize, total: Option<u64>| {
        downloaded += chunk as u64;
        let _ = app.emit("update-progress", (downloaded, total));
    };
    update
        .download_and_install(progress, || {})
        .await
        .map_err(|e| e.to_string())?;
    app.restart()
}

/// Look for updates now and then, unless the settings say not to.
pub fn watch(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(FIRST);
        loop {
            let path = &app.state::<AppState>().settings_path;
            if Settings::load(path).is_ok_and(|s| s.check_updates)
                && let Err(e) = tauri::async_runtime::block_on(check(&app))
            {
                eprintln!("papers-desktop: cannot look for updates: {e}");
            }
            std::thread::sleep(EVERY);
        }
    });
}
