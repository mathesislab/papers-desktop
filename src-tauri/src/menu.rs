//! The menu bar of the app (macOS), shown while the main window is open. Its
//! ⌘W closes the paper in front of the main window, as the page of the
//! window decides, not the window; find and zoom reach the page also while
//! the PDF view has the keys.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Emitter, Manager, Webview};

use crate::PANEL;
use crate::main_window::MAIN;

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let separator = || PredefinedMenuItem::separator(app);
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
                &[&MenuItem::with_id(
                    app,
                    "close",
                    "Close",
                    true,
                    Some("CmdOrCtrl+W"),
                )?],
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
                    &MenuItem::with_id(app, "find", "Find", true, Some("CmdOrCtrl+F"))?,
                ],
            )?,
            &Submenu::with_items(
                app,
                "View",
                true,
                &[
                    &MenuItem::with_id(app, "zoom-in", "Zoom In", true, Some("CmdOrCtrl+="))?,
                    &MenuItem::with_id(app, "zoom-out", "Zoom Out", true, Some("CmdOrCtrl+-"))?,
                    &MenuItem::with_id(app, "zoom-fit", "Fit Width", true, Some("CmdOrCtrl+0"))?,
                ],
            )?,
            &Submenu::with_items(
                app,
                "Window",
                true,
                &[
                    &PredefinedMenuItem::minimize(app, None)?,
                    &PredefinedMenuItem::maximize(app, None)?,
                ],
            )?,
        ],
    )?;
    app.set_menu(menu)?;
    app.on_menu_event(|app, event| match event.id().as_ref() {
        "close" => close(app),
        "find" => find(app),
        "zoom-in" => zoom(app, 1),
        "zoom-out" => zoom(app, -1),
        "zoom-fit" => zoom(app, 0),
        _ => {}
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
            let _ = window.emit("close", ());
        }
        PANEL => crate::hide(app),
        _ => {
            let _ = window.close();
        }
    }
}

/// ⌘F in the main window: its find field takes the keys, from the PDF view too.
fn find(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN)
        && window.is_focused().unwrap_or(false)
    {
        let webview: &Webview = window.as_ref();
        let _ = webview.set_focus();
        let _ = window.emit("find", ());
    }
}

fn zoom(app: &AppHandle, step: i32) {
    if let Some(window) = app.get_webview_window(MAIN)
        && window.is_focused().unwrap_or(false)
    {
        let _ = window.emit("zoom", step);
    }
}
