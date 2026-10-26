//! A text selected in another app, found in the library: the Services menu
//! sends it on macOS ("Search Papers", in Info.plist); on Linux,
//! `papers-desktop --selection` (for a desktop shortcut) reads the primary
//! selection.

use tauri::AppHandle;

/// The flag of a launch that finds the selection.
#[cfg(target_os = "linux")]
const FLAG: &str = "--selection";

/// Provide the service of the Services menu.
#[cfg(target_os = "macos")]
pub fn setup(app: &AppHandle) {
    macos::provide(app);
}

#[cfg(not(target_os = "macos"))]
pub fn setup(_: &AppHandle) {}

/// Find the selection if a launch asks for it; whether it did.
#[cfg(target_os = "linux")]
pub fn from_args(app: &AppHandle, args: &[String]) -> bool {
    if !args.iter().any(|a| a == FLAG) {
        return false;
    }
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let primary = gtk::Clipboard::get(&gtk::gdk::SELECTION_PRIMARY);
        primary.request_text(move |_, text| crate::find(&handle, text.unwrap_or_default().into()));
    });
    true
}

#[cfg(not(target_os = "linux"))]
pub fn from_args(_: &AppHandle, _: &[String]) -> bool {
    false
}

#[cfg(target_os = "macos")]
mod macos {
    use objc2::rc::Retained;
    use objc2::runtime::NSObject;
    use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
    use objc2_app_kit::{NSApplication, NSPasteboard, NSPasteboardTypeString};
    use objc2_foundation::NSString;
    use tauri::AppHandle;

    struct Ivars {
        app: AppHandle,
    }

    define_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "PapersDesktopServices"]
        #[ivars = Ivars]
        struct Services;

        impl Services {
            /// The NSMessage of the service.
            #[unsafe(method(searchPapers:userData:error:))]
            fn search_papers(
                &self,
                pasteboard: &NSPasteboard,
                _data: Option<&NSString>,
                _error: *mut *mut NSString,
            ) {
                // SAFETY: a constant string of AppKit
                let kind = unsafe { NSPasteboardTypeString };
                if let Some(text) = pasteboard.stringForType(kind) {
                    crate::find(&self.ivars().app, text.to_string());
                }
            }
        }
    );

    pub fn provide(app: &AppHandle) {
        let mtm = MainThreadMarker::new().expect("the app is set up on the main thread");
        let services = Services::alloc(mtm).set_ivars(Ivars { app: app.clone() });
        let services: Retained<Services> = unsafe { msg_send![super(services), init] };
        // SAFETY: the provider responds to the message of the service
        unsafe { NSApplication::sharedApplication(mtm).setServicesProvider(Some(&services)) };
        // the application does not keep its provider, which lives as long as the app
        std::mem::forget(services);
    }
}
