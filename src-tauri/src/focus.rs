//! The focus given back, when the panel hides, to the app that had it before
//! (macOS): the other windows of Papers stay as they are.

use std::sync::Mutex;

use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSWorkspace};

/// the process of the app in front when the panel showed, unless Papers
static PREVIOUS: Mutex<Option<i32>> = Mutex::new(None);

/// Note the app in front, as the panel shows.
pub fn remember() {
    let front = NSWorkspace::sharedWorkspace().frontmostApplication();
    let pid = front.map(|app| app.processIdentifier());
    *PREVIOUS.lock().unwrap() = pid.filter(|&pid| pid != std::process::id() as i32);
}

/// Bring the app noted back to the front; false without one.
pub fn give_back() -> bool {
    let Some(pid) = PREVIOUS.lock().unwrap().take() else {
        return false;
    };
    NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
        .is_some_and(|app| app.activateWithOptions(NSApplicationActivationOptions::empty()))
}
