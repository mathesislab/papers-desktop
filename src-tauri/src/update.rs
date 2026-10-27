//! Updates of the app: looked for now and then, installed when asked.

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::AppState;
use crate::settings::Settings;

/// The first look, a while after the start, then twice a day.
const FIRST: Duration = Duration::from_secs(10);
const EVERY: Duration = Duration::from_secs(12 * 60 * 60);

/// The update found, kept to install it, and the one installed just before
/// this start.
pub struct Updates {
    found: Mutex<Option<Update>>,
    installed: Option<Installed>,
}

/// An update, as the settings window shows it.
#[derive(Debug, Clone, Serialize)]
pub struct Available {
    pub version: String,
}

/// An update installed, as the version before it leaves a note of it for the
/// new one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Installed {
    pub version: String,
    pub from: String,
}

impl Updates {
    /// The updates of this start: the note of an update installed for this
    /// version is read once, then dropped.
    pub fn new(app: &AppHandle) -> Self {
        Updates {
            found: Mutex::new(None),
            installed: note(app).ok().and_then(|path| {
                let text = fs::read_to_string(&path).ok()?;
                let _ = fs::remove_file(&path);
                let installed: Installed = serde_json::from_str(&text).ok()?;
                (installed.version == app.package_info().version.to_string()).then_some(installed)
            }),
        }
    }

    pub fn available(&self) -> Option<Available> {
        self.found.lock().unwrap().as_ref().map(|u| Available {
            version: u.version.clone(),
        })
    }

    pub fn installed(&self) -> Option<Installed> {
        self.installed.clone()
    }
}

/// Where an update leaves its note for the version it installs.
fn note(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    Ok(dir.join("updated.json"))
}

/// Leave the note of an update installed. Failing that, the new version
/// starts without telling of the update.
fn leave(app: &AppHandle, installed: &Installed) -> Result<(), String> {
    let path = note(app)?;
    fs::create_dir_all(path.parent().unwrap())
        .and_then(|()| fs::write(&path, serde_json::to_string(installed).unwrap()))
        .map_err(|e| e.to_string())
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
/// (bytes so far, and in all), then start the new version, which shows it.
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
    let installed = Installed {
        version: update.version.clone(),
        from: update.current_version.clone(),
    };
    if let Err(e) = leave(app, &installed) {
        eprintln!("papers-desktop: cannot leave the note of the update: {e}");
    }
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
