//! The menu bar of the app (macOS), shown while the main window is open. Its
//! items are the commands of the main window, which reach its page also
//! while the PDF view has the keys; ⌘W closes the paper in front, as the page
//! decides, not the window. The zoom items are the text size of the other
//! windows; in the main window, the page decides: the PDF in front zooms,
//! else the text size.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Emitter, Manager, Webview, Wry};

use crate::PANEL;
use crate::main_window::MAIN;

/// the commands that lead to the fields and lists of the page, which take
/// the keys back from the PDF view
const TO_PAGE: &[&str] = &["add", "find", "library", "next-tab", "previous-tab"];

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let separator = || PredefinedMenuItem::separator(app);
    // a command of the main window: its id is the name the page knows
    let item = |id: &str, text: &str, keys: &str| -> tauri::Result<MenuItem<Wry>> {
        MenuItem::with_id(app, id, text, true, Some(keys))
    };
    let menu = Menu::with_items(
        app,
        &[
            &Submenu::with_items(
                app,
                "Papers",
                true,
                &[
                    &PredefinedMenuItem::about(app, None, None)?,
                    &separator()?,
                    &MenuItem::with_id(app, "settings", "Settings…", true, Some("CmdOrCtrl+,"))?,
                    &MenuItem::with_id(app, "install-cli", "Install Command Line Tool…", true, None::<&str>)?,
                    &separator()?,
                    &PredefinedMenuItem::services(app, None)?,
                    &separator()?,
                    &PredefinedMenuItem::hide(app, None)?,
                    &PredefinedMenuItem::hide_others(app, None)?,
                    &PredefinedMenuItem::show_all(app, None)?,
                    &separator()?,
                    &PredefinedMenuItem::quit(app, None)?,
                ],
            )?,
            &Submenu::with_items(
                app,
                "File",
                true,
                &[
                    &item("add", "Add…", "CmdOrCtrl+N")?,
                    &separator()?,
                    &MenuItem::with_id(app, "close", "Close", true, Some("CmdOrCtrl+W"))?,
                ],
            )?,
            &Submenu::with_items(
                app,
                "Edit",
                true,
                &[
                    &PredefinedMenuItem::undo(app, None)?,
                    &PredefinedMenuItem::redo(app, None)?,
                    &separator()?,
                    &PredefinedMenuItem::cut(app, None)?,
                    &PredefinedMenuItem::copy(app, None)?,
                    &PredefinedMenuItem::paste(app, None)?,
                    &PredefinedMenuItem::select_all(app, None)?,
                    &separator()?,
                    &item("find", "Find", "CmdOrCtrl+F")?,
                ],
            )?,
            &Submenu::with_items(
                app,
                "View",
                true,
                &[
                    &item("sidebar", "Show or Hide the Sidebar", "CmdOrCtrl+Alt+S")?,
                    &item("details", "Show or Hide the Details", "CmdOrCtrl+Alt+I")?,
                    &separator()?,
                    &item("zoom-in", "Zoom In", "CmdOrCtrl+=")?,
                    &item("zoom-out", "Zoom Out", "CmdOrCtrl+-")?,
                    &item("zoom-fit", "Actual Size", "CmdOrCtrl+0")?,
                ],
            )?,
            &Submenu::with_items(
                app,
                "Window",
                true,
                &[
                    &PredefinedMenuItem::minimize(app, None)?,
                    &PredefinedMenuItem::maximize(app, None)?,
                    &separator()?,
                    &item("library", "Library", "CmdOrCtrl+1")?,
                    &item("next-tab", "Next Tab", "Ctrl+Tab")?,
                    &item("previous-tab", "Previous Tab", "Ctrl+Shift+Tab")?,
                ],
            )?,
        ],
    )?;
    app.set_menu(menu)?;
    app.on_menu_event(|app, event| match event.id().as_ref() {
        "close" => close(app),
        "settings" => crate::settings_window(app, None),
        "install-cli" => crate::cli_tool::install_from_menu(app),
        id => command(app, id),
    });
    Ok(())
}

/// ⌘W: the paper in front of the main window, else the window in front;
/// the panel hides.
fn close(app: &AppHandle) {
    let windows = app.webview_windows();
    let Some(window) = windows.values().find(|w| w.is_focused().unwrap_or(false)) else {
        return;
    };
    match window.label() {
        MAIN => {
            let _ = window.emit("command", "close");
        }
        PANEL => crate::hide(app),
        _ => {
            let _ = window.close();
        }
    }
}

/// A command for the main window, when it is in front.
fn command(app: &AppHandle, name: &str) {
    if let Some(window) = app.get_webview_window(MAIN)
        && window.is_focused().unwrap_or(false)
    {
        if TO_PAGE.contains(&name) {
            let webview: &Webview = window.as_ref();
            let _ = webview.set_focus();
        }
        let _ = window.emit("command", name);
    } else if let Some(steps) = match name {
        "zoom-in" => Some(1),
        "zoom-out" => Some(-1),
        "zoom-fit" => Some(0),
        _ => None,
    } {
        let _ = crate::text_size::step(app, steps);
    }
}
