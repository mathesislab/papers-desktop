//! The menu bar of the app (macOS), shown while the main window is open. Its
//! items are the commands of the main window, which reach its page also
//! while the PDF view has the keys; ⌘W closes the paper in front, as the page
//! decides, not the window. Undo and Redo are the library's in the main window (the page decides:
//! in a field they are the field's); in another window, the field's. The zoom items are the text size of the other
//! windows; in the main window, the page decides: the PDF in front zooms,
//! else the text size.

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSEventModifierFlags};
use objc2_foundation::NSString;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Emitter, Manager, Webview, Wry};

use crate::PANEL;
use crate::main_window::MAIN;

/// the commands that lead to the fields and lists of the page, which take
/// the keys back from the PDF view
const ZOOM_IN: &str = "Zoom In";
const TO_PAGE: &[&str] = &[
    "add",
    "find",
    "library",
    "feeds",
    "next-tab",
    "previous-tab",
    "show-next-tab",
    "show-previous-tab",
];

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
                    &item("undo", "Undo", "CmdOrCtrl+Z")?,
                    &item("redo", "Redo", "CmdOrCtrl+Shift+Z")?,
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
                    &item("zoom-in", ZOOM_IN, "CmdOrCtrl+=")?,
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
                    &item("feeds", "Feeds", "CmdOrCtrl+2")?,
                    &item("next-tab", "Next Tab", "Ctrl+Tab")?,
                    &item("previous-tab", "Previous Tab", "Ctrl+Shift+Tab")?,
                    &separator()?,
                    &item("show-next-tab", "Show Next Tab", "CmdOrCtrl+Shift+]")?,
                    &item("show-previous-tab", "Show Previous Tab", "CmdOrCtrl+Shift+[")?,
                ],
            )?,
        ],
    )?;
    app.set_menu(menu)?;
    show_plus(app);
    app.on_menu_event(|app, event| match event.id().as_ref() {
        "close" => close(app),
        "settings" => crate::settings_window(app, None),
        "install-cli" => crate::cli_tool::install_from_menu(app),
        id => command(app, id),
    });
    Ok(())
}

/// View → Zoom In shows ⌘+, as Safari's does. The accelerator of a menu item here is a key, which
/// cannot be `+` (it is `=`: the item shows ⌘=), so the key equivalent of the item is set to `+`
/// once the menu is made. AppKit does not match ⌘= to it: that key reaches the page, which zooms
/// in as well (`onkeydown` of the library, `zoomStep`).
fn show_plus(app: &AppHandle) {
    let _ = app.run_on_main_thread(|| {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let Some(menu) = NSApplication::sharedApplication(mtm).mainMenu() else {
            return;
        };
        for top in menu.itemArray() {
            let Some(submenu) = top.submenu() else {
                continue;
            };
            for item in submenu.itemArray() {
                if item.title().to_string() == ZOOM_IN {
                    item.setKeyEquivalent(&NSString::from_str("+"));
                    item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
                }
            }
        }
    });
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

/// Undo and Redo: the main window's page decides (the last change of the library, or the field's
/// own); in another window the field in focus does it.
fn undo_redo(app: &AppHandle, name: &str) {
    let windows = app.webview_windows();
    let Some(window) = windows.values().find(|w| w.is_focused().unwrap_or(false)) else {
        return;
    };
    if window.label() == MAIN {
        let _ = window.emit("command", name);
    } else {
        let _ = window.eval(format!("document.execCommand('{name}')"));
    }
}

/// A command for the main window, when it is in front.
fn command(app: &AppHandle, name: &str) {
    if matches!(name, "undo" | "redo") {
        return undo_redo(app, name);
    }
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
