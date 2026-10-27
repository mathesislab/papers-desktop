//! The menu bar of the app (macOS), shown while the main window is open. Its
//! ⌘W closes the paper in front of the main window, as the page of the
//! window decides, not the window.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Emitter, Manager};

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
    app.on_menu_event(|app, event| {
        if event.id() == "close" {
            close(app);
        }
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
