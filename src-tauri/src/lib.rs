mod settings;

use std::path::PathBuf;
use std::sync::RwLock;

use papers_core::{Cite, Item, Library, Papers};
use serde::{Deserialize, Serialize};
use settings::Settings;
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, State, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tauri_plugin_opener::OpenerExt;

const PANEL: &str = "panel";
const MAX_RESULTS: usize = 50;

/// The library as the panel sees it, sent with the "status" event.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "lowercase")]
enum Status {
    Loading,
    Ready { count: usize },
    Failed { error: String },
}

/// What the copy action puts on the clipboard.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Format {
    Key,
    Pandoc,
    Latex,
    Bibtex,
}

struct AppState {
    settings_path: PathBuf,
    papers: RwLock<Papers>,
    library: RwLock<Library>,
    status: RwLock<Status>,
}

impl AppState {
    fn item(&self, key: &str) -> Result<Item, String> {
        let library = self.library.read().unwrap();
        library.get(key).cloned().ok_or_else(|| format!("{key}: not in the library"))
    }
}

#[tauri::command]
fn search(state: State<AppState>, query: String) -> Vec<Item> {
    let library = state.library.read().unwrap();
    library.search(&query, MAX_RESULTS).into_iter().cloned().collect()
}

#[tauri::command]
fn status(state: State<AppState>) -> Status {
    state.status.read().unwrap().clone()
}

#[tauri::command]
fn hide_panel(app: AppHandle) {
    hide(&app);
}

/// Open the PDF of an item, else its web page.
#[tauri::command]
fn open(app: AppHandle, state: State<AppState>, key: String) -> Result<(), String> {
    let item = state.item(&key)?;
    match (item.files.first(), item.link()) {
        (Some(file), _) => app.opener().open_path(file.to_string_lossy(), None::<&str>),
        (None, Some(link)) => app.opener().open_url(link, None::<&str>),
        (None, None) => return Err(format!("{key}: no file, DOI or URL")),
    }
    .map_err(|e| e.to_string())?;
    hide_window(&app);
    Ok(())
}

#[tauri::command]
fn open_link(app: AppHandle, state: State<AppState>, key: String) -> Result<(), String> {
    let link = state.item(&key)?.link().ok_or(format!("{key}: no DOI or URL"))?;
    app.opener().open_url(link, None::<&str>).map_err(|e| e.to_string())?;
    hide_window(&app);
    Ok(())
}

#[tauri::command]
fn reveal(app: AppHandle, state: State<AppState>, key: String) -> Result<(), String> {
    let item = state.item(&key)?;
    let file = item.files.first().ok_or(format!("{key}: no file"))?;
    app.opener().reveal_item_in_dir(file).map_err(|e| e.to_string())?;
    hide_window(&app);
    Ok(())
}

/// Copy a citation or the bibtex of an item; returns the text copied.
#[tauri::command]
async fn copy(app: AppHandle, key: String, format: Format) -> Result<String, String> {
    let item = app.state::<AppState>().item(&key)?;
    let text = match format {
        Format::Key => item.cite(Cite::Key),
        Format::Pandoc => item.cite(Cite::Pandoc),
        Format::Latex => item.cite(Cite::Latex),
        Format::Bibtex => bibtex(app.clone(), key).await?,
    };
    app.clipboard().write_text(text.clone()).map_err(|e| e.to_string())?;
    Ok(text)
}

/// The bibtex entry of a key, from papers.
#[tauri::command]
async fn bibtex(app: AppHandle, key: String) -> Result<String, String> {
    let papers = app.state::<AppState>().papers.read().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || papers.bibtex(&key))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Re-read the settings and the library, in the background.
fn reload(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        set_status(&app, Status::Loading);
        let state = app.state::<AppState>();
        let loaded = Settings::load(&state.settings_path).and_then(|settings| {
            let papers = settings.papers();
            *state.papers.write().unwrap() = papers.clone();
            papers.load().map_err(|e| e.to_string())
        });
        let status = match loaded {
            Ok(library) => {
                let count = library.len();
                *state.library.write().unwrap() = library;
                Status::Ready { count }
            }
            Err(error) => Status::Failed { error },
        };
        set_status(&app, status);
    });
}

fn set_status(app: &AppHandle, status: Status) {
    *app.state::<AppState>().status.write().unwrap() = status.clone();
    let _ = app.emit_to(PANEL, "status", status);
}

fn toggle(app: &AppHandle) {
    let Some(panel) = app.get_webview_window(PANEL) else {
        return;
    };
    if panel.is_visible().unwrap_or(false) && panel.is_focused().unwrap_or(false) {
        hide(app);
    } else {
        show(app);
    }
}

/// Show the panel on the screen of the mouse, centred, a fifth from the top.
fn show(app: &AppHandle) {
    let Some(panel) = app.get_webview_window(PANEL) else {
        return;
    };
    let monitor = panel
        .cursor_position()
        .ok()
        .and_then(|p| panel.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| panel.primary_monitor().ok().flatten());
    if let (Some(monitor), Ok(size)) = (monitor, panel.outer_size()) {
        let (origin, screen) = (monitor.position(), monitor.size());
        let x = origin.x + (screen.width as i32 - size.width as i32) / 2;
        let y = origin.y + screen.height as i32 / 5;
        let _ = panel.set_position(PhysicalPosition::new(x, y));
    }
    let _ = panel.show();
    let _ = panel.set_focus();
    let _ = panel.emit("shown", ());
}

/// Hide the panel and hand the focus back to the previous app.
fn hide(app: &AppHandle) {
    hide_window(app);
    #[cfg(target_os = "macos")]
    let _ = app.hide();
}

/// Hide the panel only: the app just opened takes the focus.
fn hide_window(app: &AppHandle) {
    if let Some(panel) = app.get_webview_window(PANEL) {
        let _ = panel.hide();
    }
}

fn tray(app: &AppHandle, hotkey: &str) -> tauri::Result<()> {
    let autostart = app.autolaunch().is_enabled().unwrap_or(false);
    let start_at_login = CheckMenuItem::with_id(app, "autostart", "Start at Login", true, autostart, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "search", "Search Papers", true, Some(hotkey))?,
            &MenuItem::with_id(app, "reload", "Reload Library", true, None::<&str>)?,
            &MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?,
            &start_at_login,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "quit", "Quit Papers", true, None::<&str>)?,
        ],
    )?;
    TrayIconBuilder::with_id("tray")
        .icon(Image::from_bytes(include_bytes!("../icons/tray.png"))?)
        .icon_as_template(true)
        .tooltip("Papers")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "search" => show(app),
            "reload" => reload(app),
            "settings" => {
                let path = app.state::<AppState>().settings_path.clone();
                let _ = app.opener().open_path(path.to_string_lossy(), None::<&str>);
            }
            "autostart" => {
                let launch = app.autolaunch();
                let enable = !launch.is_enabled().unwrap_or(false);
                let _ = if enable { launch.enable() } else { launch.disable() };
                let _ = start_at_login.set_checked(launch.is_enabled().unwrap_or(enable));
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // a second launch shows the panel of the running app
        .plugin(tauri_plugin_single_instance::init(|app, _, _| show(app)))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _, event| {
                    if event.state == ShortcutState::Pressed {
                        toggle(app);
                    }
                })
                .build(),
        )
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let settings_path = app.path().app_config_dir()?.join("settings.json");
            let settings = Settings::load(&settings_path).unwrap_or_default();
            app.manage(AppState {
                settings_path,
                papers: RwLock::new(settings.papers()),
                library: RwLock::new(Library::new(Vec::new())),
                status: RwLock::new(Status::Loading),
            });
            let hotkey = settings.hotkey;
            if let Err(e) = app.global_shortcut().register(hotkey.as_str()) {
                eprintln!("papers-desktop: cannot register the hotkey {hotkey}: {e}");
            }
            tray(app.handle(), &hotkey)?;
            reload(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == PANEL && matches!(event, WindowEvent::Focused(false)) {
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            search, status, hide_panel, open, open_link, reveal, copy, bibtex
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
