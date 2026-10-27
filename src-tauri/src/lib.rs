#[cfg(target_os = "macos")]
mod focus;
mod hotkey;
mod main_window;
#[cfg(target_os = "macos")]
mod menu;
mod recent;
mod selection;
mod settings;
mod styles;
mod update;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, RwLock};

use main_window::{Layout, MAIN};
use papers_core::{BibWatch, Cite, Found, Item, Library, Matching, Row, TagCount};
use recent::Recent;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use settings::Settings;
use styles::Style;
use tauri::image::Image;
use tauri::ipc::Response;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, State, WebviewUrl,
    WebviewWindowBuilder, WindowEvent, Wry,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_window_state::StateFlags;
use update::{Available, Installed, Updates};

pub(crate) const PANEL: &str = "panel";
const SETTINGS: &str = "settings";
const MAX_RESULTS: usize = 200;
/// panel size in logical pixels, as in tauri.conf.json; the preview widens it
const PANEL_WIDTH: f64 = 720.0;
const PANEL_HEIGHT: f64 = 460.0;
const PREVIEW_WIDTH: f64 = 500.0;
const REPO: &str = "https://github.com/mathesislab/papers-desktop";

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
    matching: RwLock<Matching>,
    citation_style: RwLock<String>,
    library: RwLock<Library>,
    status: RwLock<Status>,
    /// the .bib files of the library, replaced when the files change
    watches: Mutex<Vec<BibWatch>>,
    /// counts the reloads: one that ends after a newer one started is dropped
    reloads: AtomicU64,
    /// the menu item that shows the hotkey
    search_item: Mutex<Option<MenuItem<Wry>>>,
    /// the menu item that tells about an update
    update_item: Mutex<Option<MenuItem<Wry>>>,
    recent: Recent,
    layout: Layout,
}

impl AppState {
    fn item(&self, id: &str) -> Result<Item, String> {
        let library = self.library.read().unwrap();
        library
            .get(id)
            .cloned()
            .ok_or_else(|| format!("{id}: not in the library"))
    }
}

/// The matches of a query; without one, the recent papers.
#[tauri::command]
fn search(state: State<AppState>, query: String) -> Found {
    let library = state.library.read().unwrap();
    if query.trim().is_empty() {
        let hits = state.recent.hits(&library);
        return Found {
            total: hits.len(),
            hits,
            years: Vec::new(),
        };
    }
    let matching = *state.matching.read().unwrap();
    library.search(&query, matching, MAX_RESULTS)
}

/// The rows of the table of the main window: all the items. Async: the
/// linked files are looked for.
#[tauri::command]
async fn rows(app: AppHandle) -> Vec<Row> {
    app.state::<AppState>().library.read().unwrap().rows()
}

/// The ids of the items that match a query, newest first: all the items
/// without one.
#[tauri::command]
fn matching(state: State<AppState>, query: String) -> Vec<String> {
    let matching = *state.matching.read().unwrap();
    state.library.read().unwrap().matching(&query, matching)
}

#[tauri::command]
fn tags(state: State<AppState>) -> Vec<TagCount> {
    state.library.read().unwrap().tags()
}

/// The groups of items that may be the same paper.
#[tauri::command]
fn duplicates(state: State<AppState>) -> Vec<Vec<String>> {
    state.library.read().unwrap().duplicates()
}

/// The ids of the recent papers, the last first.
#[tauri::command]
fn recent(state: State<AppState>) -> Vec<String> {
    let library = state.library.read().unwrap();
    let hits = state.recent.hits(&library);
    hits.into_iter().map(|h| h.item.id).collect()
}

/// A paper opened in the main window: one of the recent papers.
#[tauri::command]
fn opened(state: State<AppState>, id: String) -> Result<(), String> {
    state.recent.add(&state.item(&id)?);
    Ok(())
}

/// An item, for the details of the main window.
#[tauri::command]
fn item(state: State<AppState>, id: String) -> Result<Item, String> {
    state.item(&id)
}

/// The entries closest to a reference, pasted.
#[tauri::command]
fn closest(state: State<AppState>, reference: String) -> Found {
    let library = state.library.read().unwrap();
    library.closest(&reference, MAX_RESULTS)
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
fn open(app: AppHandle, state: State<AppState>, id: String) -> Result<(), String> {
    let item = state.item(&id)?;
    match (item.files.first(), item.link()) {
        (Some(file), _) => app.opener().open_path(file.to_string_lossy(), None::<&str>),
        (None, Some(link)) => app.opener().open_url(link, None::<&str>),
        (None, None) => return Err(format!("{id}: no file, DOI or URL")),
    }
    .map_err(|e| e.to_string())?;
    state.recent.add(&item);
    hide_window(&app);
    Ok(())
}

#[tauri::command]
fn open_link(app: AppHandle, state: State<AppState>, id: String) -> Result<(), String> {
    let item = state.item(&id)?;
    let link = item.link().ok_or(format!("{id}: no DOI or URL"))?;
    app.opener()
        .open_url(link, None::<&str>)
        .map_err(|e| e.to_string())?;
    state.recent.add(&item);
    hide_window(&app);
    Ok(())
}

#[tauri::command]
fn reveal(app: AppHandle, state: State<AppState>, id: String) -> Result<(), String> {
    let item = state.item(&id)?;
    let file = item.files.first().ok_or(format!("{id}: no file"))?;
    app.opener()
        .reveal_item_in_dir(file)
        .map_err(|e| e.to_string())?;
    state.recent.add(&item);
    hide_window(&app);
    Ok(())
}

/// Copy a citation or the bibtex of an item; returns the text copied.
#[tauri::command]
fn copy(
    app: AppHandle,
    state: State<AppState>,
    id: String,
    format: Format,
) -> Result<String, String> {
    let item = state.item(&id)?;
    let text = match format {
        Format::Key => item.cite(Cite::Key),
        Format::Pandoc => item.cite(Cite::Pandoc),
        Format::Latex => item.cite(Cite::Latex),
        Format::Bibtex => state
            .library
            .read()
            .unwrap()
            .bibtex(&id)
            .ok_or_else(|| format!("{id}: not in the library"))?,
    };
    app.clipboard()
        .write_text(text.clone())
        .map_err(|e| e.to_string())?;
    state.recent.add(&item);
    Ok(text)
}

/// What the settings window shows.
#[derive(Serialize)]
struct SettingsView {
    settings: Settings,
    /// the bundled citation styles
    styles: Vec<&'static str>,
    start_at_login: bool,
    /// to write paths with ~
    home: Option<PathBuf>,
}

#[tauri::command]
fn get_settings(app: AppHandle, state: State<AppState>) -> Result<SettingsView, String> {
    Ok(SettingsView {
        settings: Settings::load(&state.settings_path)?,
        styles: styles::names(),
        start_at_login: app.autolaunch().is_enabled().unwrap_or(false),
        home: std::env::home_dir(),
    })
}

/// Save the settings of the settings window, and apply them at once.
#[tauri::command]
fn save_settings(app: AppHandle, state: State<AppState>, settings: Settings) -> Result<(), String> {
    let old = Settings::load(&state.settings_path)?;
    hotkey::apply(&app, &old, &settings)?;
    settings.save(&state.settings_path)?;
    reload(&app);
    Ok(())
}

/// Start the app at login, or not; returns whether it does.
#[tauri::command]
fn set_start_at_login(app: AppHandle, on: bool) -> Result<bool, String> {
    let launch = app.autolaunch();
    if on {
        launch.enable()
    } else {
        launch.disable()
    }
    .map_err(|e| e.to_string())?;
    launch.is_enabled().map_err(|e| e.to_string())
}

/// Look for an update now.
#[tauri::command]
async fn check_update(app: AppHandle) -> Result<Option<Available>, String> {
    update::check(&app).await
}

/// The update found by the last look, if any.
#[tauri::command]
fn found_update(updates: State<Updates>) -> Option<Available> {
    updates.available()
}

/// The update installed just before this start, if any.
#[tauri::command]
fn installed_update(updates: State<Updates>) -> Option<Installed> {
    updates.installed()
}

/// The notes of a release, on GitHub.
#[tauri::command]
fn release_notes(app: AppHandle, version: String) -> Result<(), String> {
    app.opener()
        .open_url(format!("{REPO}/releases/tag/v{version}"), None::<&str>)
        .map_err(|e| e.to_string())
}

/// Install the update found, and restart.
#[tauri::command]
async fn install_update(app: AppHandle) -> Result<(), String> {
    update::install(&app).await
}

/// The settings window, from the panel (⌘,).
#[tauri::command]
async fn open_settings(app: AppHandle) {
    settings_window(&app, None);
}

/// The main window, from the panel (⌘O).
#[tauri::command]
async fn open_library(app: AppHandle) {
    main_window::show(&app);
}

/// The main window with a paper open in it, from the panel (⌘I).
#[tauri::command]
async fn open_paper(app: AppHandle, id: String) {
    main_window::open_paper(&app, &id);
}

/// The layout of the main window saved last.
#[tauri::command]
fn layout(state: State<AppState>) -> Option<Value> {
    state.layout.load()
}

#[tauri::command]
fn save_layout(state: State<AppState>, layout: Value) {
    state.layout.save(&layout);
}

/// The feedback form, from the settings.
#[tauri::command]
fn send_feedback(app: AppHandle) -> Result<(), String> {
    feedback(&app)
}

/// Put a text of item `id` on the clipboard, with its HTML for a formatted
/// paste: what the share menu of the preview copies, and the citations.
#[tauri::command]
fn copy_text(
    app: AppHandle,
    state: State<AppState>,
    id: String,
    text: String,
    html: Option<String>,
) -> Result<(), String> {
    let item = state.item(&id)?;
    let clipboard = app.clipboard();
    match html {
        Some(html) => clipboard.write_html(html, Some(text)),
        None => clipboard.write_text(text),
    }
    .map_err(|e| e.to_string())?;
    state.recent.add(&item);
    Ok(())
}

/// Forget the recent papers (from the settings).
#[tauri::command]
fn clear_recent(state: State<AppState>) {
    state.recent.clear();
}

/// The BibTeX of an item: formatted, or `raw` as written in its .bib file.
#[tauri::command]
fn bibtex(state: State<AppState>, id: String, raw: bool) -> Result<String, String> {
    let library = state.library.read().unwrap();
    let bibtex = if raw {
        library.raw(&id).map(String::from)
    } else {
        library.bibtex(&id)
    };
    bibtex.ok_or_else(|| format!("{id}: not in the library"))
}

/// The CSL-JSON of an item, for the citation processor of the panel.
#[tauri::command]
fn csl(state: State<AppState>, id: String) -> Result<Value, String> {
    let library = state.library.read().unwrap();
    library
        .csl(&id)
        .ok_or_else(|| format!("{id}: not in the library"))
}

/// The CSL style of the settings.
#[tauri::command]
fn citation_style(state: State<AppState>) -> Result<Style, String> {
    Style::load(&state.citation_style.read().unwrap())
}

/// The first PDF of an item, for the preview.
#[tauri::command]
async fn pdf(app: AppHandle, id: String) -> Result<Response, String> {
    let item = app.state::<AppState>().item(&id)?;
    let file = item
        .files
        .into_iter()
        .find(|f| f.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")))
        .ok_or(format!("{id}: no PDF"))?;
    let bytes = tauri::async_runtime::spawn_blocking(move || std::fs::read(file))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    Ok(Response::new(bytes))
}

/// Widen the panel for the preview, or narrow it back, around its centre.
#[tauri::command]
fn set_preview(app: AppHandle, on: bool) -> Result<(), String> {
    let panel = app.get_webview_window(PANEL).ok_or("no panel")?;
    let width = if on {
        PANEL_WIDTH + PREVIEW_WIDTH
    } else {
        PANEL_WIDTH
    };
    let resize = || -> tauri::Result<()> {
        let (position, old) = (panel.outer_position()?, panel.outer_size()?);
        panel.set_size(LogicalSize::new(width, PANEL_HEIGHT))?;
        let new = panel.outer_size()?;
        let x = position.x + (old.width as i32 - new.width as i32) / 2;
        panel.set_position(PhysicalPosition::new(x, position.y))
    };
    resize().map_err(|e| e.to_string())
}

/// Re-read the settings and the library, in the background.
/// Then watch the .bib files, to reload when one changes.
fn reload(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let reload = state.reloads.fetch_add(1, Ordering::SeqCst) + 1;
        set_status(&app, Status::Loading);
        let loaded = Settings::load(&state.settings_path).and_then(|settings| {
            *state.matching.write().unwrap() = settings.matching;
            *state.citation_style.write().unwrap() = settings.citation_style.clone();
            let bibs = settings.bibs();
            if bibs.is_empty() {
                return Err("No library: choose its .bib files (menu-bar icon → Settings…)".into());
            }
            let library = Library::load(&bibs).map_err(|e| e.to_string())?;
            Ok((bibs, library))
        });
        if state.reloads.load(Ordering::SeqCst) != reload {
            return;
        }
        let status = match loaded {
            Ok((bibs, library)) => {
                let count = library.len();
                eprintln!(
                    "papers-desktop: loaded {count} entries from {} .bib files",
                    bibs.len()
                );
                *state.library.write().unwrap() = library;
                watch(&app, &bibs);
                Status::Ready { count }
            }
            Err(error) => {
                eprintln!("papers-desktop: {error}");
                Status::Failed { error }
            }
        };
        set_status(&app, status);
    });
}

/// Watch the .bib files of the library, unless they are watched already.
fn watch(app: &AppHandle, bibs: &[PathBuf]) {
    let state = app.state::<AppState>();
    let mut watches = state.watches.lock().unwrap();
    if watches
        .iter()
        .map(BibWatch::path)
        .eq(bibs.iter().map(PathBuf::as_path))
    {
        return;
    }
    watches.clear();
    for bib in bibs {
        let handle = app.clone();
        match BibWatch::new(bib.clone(), move || reload(&handle)) {
            Ok(w) => watches.push(w),
            Err(e) => eprintln!("papers-desktop: cannot watch {}: {e}", bib.display()),
        }
    }
}

/// The menu item of the panel, with its hotkey.
pub(crate) fn show_hotkey(app: &AppHandle, hotkey: &str) {
    if let Some(item) = app.state::<AppState>().search_item.lock().unwrap().as_ref() {
        let _ = item.set_accelerator(Some(hotkey));
    }
}

/// The menu item of updates: "Check for Updates…", or the version found.
pub(crate) fn show_update(app: &AppHandle, available: Option<&Available>) {
    let text = match available {
        Some(update) => format!("Update to {}…", update.version),
        None => "Check for Updates…".to_string(),
    };
    if let Some(item) = app.state::<AppState>().update_item.lock().unwrap().as_ref() {
        let _ = item.set_text(text);
    }
}

/// Show the settings window, or a section of it ("updates"). The window is
/// made hidden the first time: its page shows it once drawn, at the section.
fn settings_window(app: &AppHandle, section: Option<&str>) {
    hide_window(app);
    if let Some(window) = app.get_webview_window(SETTINGS) {
        let _ = window.show();
        let _ = window.set_focus();
        if let Some(section) = section {
            let _ = app.emit_to(SETTINGS, "section", section);
        }
        return;
    }
    let url = match section {
        Some(section) => format!("settings#{section}"),
        None => "settings".to_string(),
    };
    let window = WebviewWindowBuilder::new(app, SETTINGS, WebviewUrl::App(url.into()))
        .title("Papers Desktop Settings")
        .inner_size(560.0, 760.0)
        .resizable(false)
        .minimizable(false)
        .center()
        .visible(false)
        .build();
    if let Err(e) = window {
        eprintln!("papers-desktop: cannot open the settings: {e}");
    }
}

/// Open GitHub's form for a new issue, with the version and the system
/// below the text to write.
fn feedback(app: &AppHandle) -> Result<(), String> {
    let body = format!(
        "\n\n\nPapers Desktop {}, {} {}",
        app.package_info().version,
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let form = tauri::Url::parse_with_params(&format!("{REPO}/issues/new"), [("body", body)])
        .map_err(|e| e.to_string())?;
    app.opener()
        .open_url(form.as_str(), None::<&str>)
        .map_err(|e| e.to_string())
}

fn set_status(app: &AppHandle, status: Status) {
    *app.state::<AppState>().status.write().unwrap() = status.clone();
    let _ = app.emit("status", status);
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

/// Show the panel finding a text selected in another app: as a reference,
/// or as a query when it is short.
fn find(app: &AppHandle, text: String) {
    show(app);
    let _ = app.emit_to(PANEL, "find", text);
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
    #[cfg(target_os = "macos")]
    focus::remember();
    let _ = panel.show();
    let _ = panel.set_focus();
    let _ = panel.emit("shown", ());
}

/// Hide the panel and hand the focus back to the app that had it; when
/// Papers had it, its other window takes it.
fn hide(app: &AppHandle) {
    hide_window(app);
    #[cfg(target_os = "macos")]
    if !focus::give_back()
        && !app
            .webview_windows()
            .values()
            .any(|w| w.is_visible().unwrap_or(false))
    {
        let _ = app.hide();
    }
}

/// Hide the panel only: the app just opened takes the focus.
fn hide_window(app: &AppHandle) {
    if let Some(panel) = app.get_webview_window(PANEL) {
        let _ = panel.hide();
    }
}

/// A black template image in the menu bar of macOS, a white icon on the dark
/// panels of the other desktops.
#[cfg(target_os = "macos")]
const TRAY_ICON: &[u8] = include_bytes!("../icons/tray.png");
#[cfg(not(target_os = "macos"))]
const TRAY_ICON: &[u8] = include_bytes!("../icons/tray-light.png");

fn tray(app: &AppHandle, hotkey: &str) -> tauri::Result<()> {
    let search = MenuItem::with_id(app, "search", "Search Papers", true, Some(hotkey))?;
    let updates = MenuItem::with_id(app, "updates", "Check for Updates…", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &search,
            &MenuItem::with_id(app, "library", "Open Library", true, None::<&str>)?,
            &MenuItem::with_id(app, "settings", "Settings…", true, Some("CmdOrCtrl+,"))?,
            &MenuItem::with_id(app, "reload", "Reload Library", true, None::<&str>)?,
            &updates,
            &MenuItem::with_id(app, "feedback", "Send Feedback…", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "quit", "Quit Papers", true, None::<&str>)?,
        ],
    )?;
    *app.state::<AppState>().search_item.lock().unwrap() = Some(search);
    *app.state::<AppState>().update_item.lock().unwrap() = Some(updates);
    TrayIconBuilder::with_id("tray")
        .icon(Image::from_bytes(TRAY_ICON)?)
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip("Papers")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "search" => show(app),
            "library" => main_window::show(app),
            "settings" => settings_window(app, None),
            "updates" => settings_window(app, Some("updates")),
            "feedback" => {
                if let Err(e) = feedback(app) {
                    eprintln!("papers-desktop: cannot open the feedback form: {e}");
                }
            }
            "reload" => reload(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // a second launch toggles the panel of the running app: under Wayland
        // a desktop shortcut that launches the app stands in for the hotkey
        .plugin(tauri_plugin_single_instance::init(|app, args, _| {
            if !selection::from_args(app, &args) {
                toggle(app);
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_window_state::Builder::new()
                .with_denylist(&[PANEL, SETTINGS])
                .with_state_flags(StateFlags::SIZE | StateFlags::POSITION | StateFlags::MAXIMIZED)
                .build(),
        )
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_drag::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let settings_path = app.path().app_config_dir()?.join("settings.json");
            let settings = Settings::load(&settings_path).unwrap_or_default();
            app.manage(AppState {
                settings_path,
                matching: RwLock::new(settings.matching),
                citation_style: RwLock::new(settings.citation_style.clone()),
                library: RwLock::new(Library::default()),
                status: RwLock::new(Status::Loading),
                watches: Mutex::new(Vec::new()),
                reloads: AtomicU64::new(0),
                search_item: Mutex::new(None),
                update_item: Mutex::new(None),
                recent: Recent::load(app.path().app_data_dir()?.join("recent.json")),
                layout: Layout::new(app.path().app_data_dir()?.join("window.json")),
            });
            app.manage(Updates::new(app.handle()));
            hotkey::setup(app.handle(), &settings);
            tray(app.handle(), &settings.hotkey)?;
            #[cfg(target_os = "macos")]
            menu::setup(app.handle())?;
            selection::setup(app.handle());
            reload(app.handle());
            update::watch(app.handle());
            // after an update, the settings tell of it
            if app.state::<Updates>().installed().is_some() {
                settings_window(app.handle(), Some("updates"));
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == PANEL && matches!(event, WindowEvent::Focused(false)) {
                let _ = window.hide();
            }
            if window.label() == MAIN && matches!(event, WindowEvent::Destroyed) {
                main_window::closed(window.app_handle());
            }
        })
        .invoke_handler(tauri::generate_handler![
            search,
            closest,
            rows,
            matching,
            tags,
            duplicates,
            recent,
            opened,
            item,
            status,
            hide_panel,
            open,
            open_link,
            reveal,
            copy,
            copy_text,
            clear_recent,
            bibtex,
            pdf,
            set_preview,
            csl,
            citation_style,
            get_settings,
            save_settings,
            set_start_at_login,
            open_settings,
            open_library,
            open_paper,
            layout,
            save_layout,
            send_feedback,
            check_update,
            found_update,
            installed_update,
            release_notes,
            install_update
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
