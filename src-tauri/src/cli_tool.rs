//! The `papers-rs` command line tool that comes with the app (Tauri's sidecar,
//! next to the executable), installed as a link in a folder of the PATH:
//! `/usr/local/bin` on macOS, with the system's administrator prompt, as VS
//! Code installs `code`; `~/.local/bin` on Linux. Only a link of ours is ever
//! replaced or removed.

use std::fs;
use std::io::{self, ErrorKind, Read};
use std::path::{Path, PathBuf};

use serde::Serialize;

/// the name of the command, and of the link
pub const NAME: &str = "papers-rs";
/// the executable of the app, next to the sidecar
const APP: &str = "papers-desktop";
/// what the placeholder of the sidecar has in its first line (build.rs)
const PLACEHOLDER: &[u8] = b"# papers-rs-placeholder";

/// What is at the path of the link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    Missing,
    /// a link of ours, to this target (not always the binary of this app: a
    /// copy of the app moved or updated since)
    Ours(PathBuf),
    /// anything else: a program, a link to one
    Foreign,
}

/// What an install or an uninstall did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Done {
    Installed,
    Already,
    Removed,
    /// an uninstall with nothing to remove
    Absent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FailureKind {
    /// a file that is not our link is at the path: left as it is
    Foreign,
    /// the user cancelled the administrator prompt
    Cancelled,
    /// not allowed to write there (and no prompt to ask)
    Denied,
    Failed,
}

/// Why an install or an uninstall did not happen, as the page shows it.
#[derive(Debug, Clone, Serialize)]
pub struct Failure {
    pub kind: FailureKind,
    pub message: String,
    /// for `Cancelled`: what to run in a terminal instead
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}

impl Failure {
    fn new(kind: FailureKind, message: impl Into<String>) -> Failure {
        Failure {
            kind,
            message: message.into(),
            command: None,
        }
    }

    fn io(path: &Path, e: &io::Error) -> Failure {
        let kind = match e.kind() {
            ErrorKind::PermissionDenied => FailureKind::Denied,
            _ => FailureKind::Failed,
        };
        Failure::new(kind, format!("{}: {e}", path.display()))
    }
}

/// Whether the link in `dir` is of ours: a link to the binary of this app,
/// or to a `papers-rs` of another copy of the app (named as the app's folder
/// on macOS, or next to the app's executable).
pub fn link_state(dir: &Path, binary: &Path) -> Link {
    let path = dir.join(NAME);
    let Ok(meta) = fs::symlink_metadata(&path) else {
        return Link::Missing;
    };
    if !meta.is_symlink() {
        return Link::Foreign;
    }
    match fs::read_link(&path) {
        Ok(target) if is_ours(&target, binary) => Link::Ours(target),
        _ => Link::Foreign,
    }
}

fn is_ours(target: &Path, binary: &Path) -> bool {
    target == binary
        || (target.file_name().is_some_and(|n| n == NAME)
            && (target.components().any(|c| c.as_os_str() == "Papers Desktop.app")
                || target.parent().is_some_and(|d| d.join(APP).exists())))
}

#[cfg(unix)]
fn symlink(binary: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(binary, link)
}

#[cfg(not(unix))]
fn symlink(_binary: &Path, _link: &Path) -> io::Result<()> {
    Err(io::Error::new(ErrorKind::Unsupported, "links are for macOS and Linux"))
}

/// Link `papers-rs` in `dir` to `binary`: created, or pointed again if it is
/// a link of ours to another copy of the app. A file or a link that is not
/// ours is never replaced.
pub fn install(dir: &Path, binary: &Path) -> Result<Done, Failure> {
    let path = dir.join(NAME);
    match link_state(dir, binary) {
        Link::Foreign => return Err(foreign(&path)),
        Link::Ours(target) if target == binary => return Ok(Done::Already),
        Link::Ours(_) => fs::remove_file(&path).map_err(|e| Failure::io(&path, &e))?,
        Link::Missing => fs::create_dir_all(dir).map_err(|e| Failure::io(dir, &e))?,
    }
    symlink(binary, &path).map_err(|e| Failure::io(&path, &e))?;
    Ok(Done::Installed)
}

/// Remove the link in `dir`, if it is ours.
pub fn uninstall(dir: &Path, binary: &Path) -> Result<Done, Failure> {
    let path = dir.join(NAME);
    match link_state(dir, binary) {
        Link::Missing => Ok(Done::Absent),
        Link::Foreign => Err(foreign(&path)),
        Link::Ours(_) => {
            fs::remove_file(&path).map_err(|e| Failure::io(&path, &e))?;
            Ok(Done::Removed)
        }
    }
}

fn foreign(path: &Path) -> Failure {
    Failure::new(
        FailureKind::Foreign,
        format!("{} is not a link to Papers Desktop's: left as it is", path.display()),
    )
}

/// The folder of the link: in the PATH of the system, or of the user.
pub fn default_dir() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        Some(PathBuf::from("/usr/local/bin"))
    } else if cfg!(unix) {
        std::env::home_dir().map(|home| home.join(".local/bin"))
    } else {
        None
    }
}

/// The `papers-rs` of this app: next to its executable, once built by
/// scripts/sidecar.sh (a development build has the placeholder: none).
pub fn bundled_binary() -> Option<PathBuf> {
    let path = std::env::current_exe().ok()?.parent()?.join(NAME);
    (path.is_file() && !is_placeholder(&path)).then_some(path)
}

fn is_placeholder(path: &Path) -> bool {
    let mut head = Vec::new();
    let read = fs::File::open(path).and_then(|file| file.take(64).read_to_end(&mut head));
    read.is_ok() && head.windows(PLACEHOLDER.len()).any(|w| w == PLACEHOLDER)
}

/// What the settings window shows of the tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Status {
    /// false: nothing to install (a development build, an AppImage, Windows)
    pub available: bool,
    /// the link
    pub link: Option<PathBuf>,
    /// "missing", "ours" or "foreign" (not ours: never touched)
    pub state: &'static str,
    /// where our link points
    pub points_to: Option<PathBuf>,
    /// our link points to the copy of the app that runs
    pub current: bool,
}

pub fn status(dir: Option<&Path>, binary: Option<&Path>) -> Status {
    let (Some(dir), Some(binary)) = (dir, binary) else {
        return Status {
            available: false,
            link: dir.map(|d| d.join(NAME)),
            state: "missing",
            points_to: None,
            current: false,
        };
    };
    let (state, points_to) = match link_state(dir, binary) {
        Link::Missing => ("missing", None),
        Link::Foreign => ("foreign", None),
        Link::Ours(target) => ("ours", Some(target)),
    };
    Status {
        available: true,
        link: Some(dir.join(NAME)),
        state,
        current: points_to.as_deref() == Some(binary),
        points_to,
    }
}

/// The binary and the folder to install, or why it cannot be.
fn target() -> Result<(PathBuf, PathBuf), Failure> {
    let unavailable = |why: &str| Failure::new(FailureKind::Failed, why);
    // an AppImage is mounted while it runs: a link to it would break
    if std::env::var_os("APPIMAGE").is_some() {
        return Err(unavailable(
            "Not from an AppImage: it is mounted only while the app runs. Install the .deb, or copy papers-rs out of it",
        ));
    }
    let binary = bundled_binary().ok_or_else(|| unavailable("This build of the app has no papers-rs"))?;
    let dir = default_dir().ok_or_else(|| unavailable("The command line tool is for macOS and Linux"))?;
    Ok((dir, binary))
}

fn current_status() -> Status {
    let binary = bundled_binary().filter(|_| std::env::var_os("APPIMAGE").is_none());
    status(default_dir().as_deref(), binary.as_deref())
}

#[tauri::command]
pub fn cli_status() -> Status {
    current_status()
}

#[tauri::command]
pub async fn install_cli() -> Result<Status, Failure> {
    run(install_here).await
}

#[tauri::command]
pub async fn uninstall_cli() -> Result<Status, Failure> {
    run(uninstall_here).await
}

/// The change, off the thread of the page: the administrator prompt waits for
/// the user.
async fn run(change: fn() -> Result<Done, Failure>) -> Result<Status, Failure> {
    tauri::async_runtime::spawn_blocking(change)
        .await
        .map_err(|e| Failure::new(FailureKind::Failed, e.to_string()))??;
    Ok(current_status())
}

fn install_here() -> Result<Done, Failure> {
    let (dir, binary) = target()?;
    match install(&dir, &binary) {
        Err(Failure {
            kind: FailureKind::Denied,
            ..
        }) if cfg!(target_os = "macos") => privileged(&dir, &binary, false),
        done => done,
    }
}

fn uninstall_here() -> Result<Done, Failure> {
    let (dir, binary) = target()?;
    match uninstall(&dir, &binary) {
        Err(Failure {
            kind: FailureKind::Denied,
            ..
        }) if cfg!(target_os = "macos") => privileged(&dir, &binary, true),
        done => done,
    }
}

/// The install or the uninstall again, by the shell with the administrator
/// prompt of the system, after the checks of ours (a file that is not our
/// link is not touched). If the user cancels, the failure has the command to
/// run in a terminal.
fn privileged(dir: &Path, binary: &Path, remove: bool) -> Result<Done, Failure> {
    let state = link_state(dir, binary);
    match (&state, remove) {
        (Link::Foreign, _) => return Err(foreign(&dir.join(NAME))),
        (Link::Missing, true) => return Ok(Done::Absent),
        (Link::Ours(target), false) if target == binary => return Ok(Done::Already),
        _ => {}
    }
    let script = script(dir, binary, remove);
    match run_as_administrator(&script, remove) {
        Ok(()) => Ok(if remove { Done::Removed } else { Done::Installed }),
        Err(Some(message)) => Err(Failure::new(FailureKind::Failed, message)),
        Err(None) => Err(Failure {
            kind: FailureKind::Cancelled,
            message: "Cancelled: nothing was changed. In a terminal, run:".into(),
            command: Some(terminal_command(dir, binary, remove)),
        }),
    }
}

/// `'text'` for a POSIX shell.
fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// The commands that make or remove the link, for `sh`; `-n` so that a
/// link to a folder of ours is replaced, not entered.
fn script(dir: &Path, binary: &Path, remove: bool) -> String {
    let link = quoted(&dir.join(NAME).to_string_lossy());
    if remove {
        format!("rm {link}")
    } else {
        let dir = quoted(&dir.to_string_lossy());
        let binary = quoted(&binary.to_string_lossy());
        format!("mkdir -p {dir} && ln -sfn {binary} {link}")
    }
}

/// What to run in a terminal instead: the script, with `sudo` for each command.
fn terminal_command(dir: &Path, binary: &Path, remove: bool) -> String {
    script(dir, binary, remove)
        .split(" && ")
        .map(|command| format!("sudo {command}"))
        .collect::<Vec<_>>()
        .join(" && ")
}

/// The string of an AppleScript for `text`.
#[cfg(any(target_os = "macos", test))]
fn applescript_string(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// `do shell script … with administrator privileges`, for the script.
#[cfg(any(target_os = "macos", test))]
fn applescript(script: &str, remove: bool) -> String {
    let prompt = if remove {
        "Papers Desktop wants to remove the papers-rs command."
    } else {
        "Papers Desktop wants to install the papers-rs command."
    };
    format!(
        "do shell script {} with prompt {} with administrator privileges",
        applescript_string(script),
        applescript_string(prompt)
    )
}

/// Run a script with the administrator prompt of macOS. Err(None): the user
/// cancelled it.
#[cfg(target_os = "macos")]
fn run_as_administrator(script: &str, remove: bool) -> Result<(), Option<String>> {
    let output = std::process::Command::new("osascript")
        .arg("-e")
        .arg(applescript(script, remove))
        .output()
        .map_err(|e| Some(format!("osascript: {e}")))?;
    if output.status.success() {
        return Ok(());
    }
    let error = String::from_utf8_lossy(&output.stderr);
    // AppleScript's error -128: "User canceled."
    if error.contains("(-128)") {
        Err(None)
    } else {
        Err(Some(error.trim().to_string()))
    }
}

#[cfg(not(target_os = "macos"))]
fn run_as_administrator(_script: &str, _remove: bool) -> Result<(), Option<String>> {
    Err(Some("no administrator prompt on this system".into()))
}

/// "Install Command Line Tool…" of the app menu (macOS): installs, then says
/// what happened (and the command to run in a terminal if the user cancelled
/// the prompt).
#[cfg(target_os = "macos")]
pub fn install_from_menu(app: &tauri::AppHandle) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
    let app = app.clone();
    std::thread::spawn(move || {
        let (kind, text) = match install_here() {
            Ok(Done::Already) => (
                MessageDialogKind::Info,
                format!("The papers-rs command is already installed at {}.", default_link()),
            ),
            Ok(_) => (
                MessageDialogKind::Info,
                format!("The papers-rs command is installed at {}.", default_link()),
            ),
            Err(Failure {
                kind: FailureKind::Cancelled,
                message,
                command,
            }) => (
                MessageDialogKind::Warning,
                format!("{message}\n\n{}", command.unwrap_or_default()),
            ),
            Err(e) => (MessageDialogKind::Error, e.message),
        };
        app.dialog()
            .message(text)
            .title("Command Line Tool")
            .kind(kind)
            .show(|_| {});
    });
}

#[cfg(target_os = "macos")]
fn default_link() -> String {
    default_dir().unwrap_or_default().join(NAME).display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink as link;

    /// a temp folder with the binary of an app and an empty folder for links
    fn setup() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let app = tmp.path().join("app");
        fs::create_dir(&app).unwrap();
        let binary = app.join(NAME);
        fs::write(&binary, "binary").unwrap();
        fs::write(app.join(APP), "app").unwrap();
        let bin = tmp.path().join("bin");
        fs::create_dir(&bin).unwrap();
        (tmp, binary, bin)
    }

    #[test]
    fn installs_a_link_creating_the_folder() {
        let (tmp, binary, _) = setup();
        let dir = tmp.path().join("new/bin");
        assert_eq!(install(&dir, &binary).unwrap(), Done::Installed);
        assert_eq!(fs::read_link(dir.join(NAME)).unwrap(), binary);
        assert_eq!(link_state(&dir, &binary), Link::Ours(binary.clone()));
    }

    #[test]
    fn already_installed() {
        let (_tmp, binary, bin) = setup();
        install(&bin, &binary).unwrap();
        assert_eq!(install(&bin, &binary).unwrap(), Done::Already);
        assert_eq!(fs::read_link(bin.join(NAME)).unwrap(), binary);
    }

    #[test]
    fn a_link_to_another_copy_of_the_app_is_pointed_again() {
        let (tmp, binary, bin) = setup();
        // the app was moved: the old link is dangling, in a folder of the app's name
        let old = tmp.path().join("Applications/Papers Desktop.app/Contents/MacOS");
        link(old.join(NAME), bin.join(NAME)).unwrap();
        assert_eq!(link_state(&bin, &binary), Link::Ours(old.join(NAME)));
        assert_eq!(install(&bin, &binary).unwrap(), Done::Installed);
        assert_eq!(fs::read_link(bin.join(NAME)).unwrap(), binary);
    }

    #[test]
    fn a_foreign_file_is_never_replaced() {
        let (tmp, binary, bin) = setup();
        let path = bin.join(NAME);
        fs::write(&path, "someone else's").unwrap();
        let e = install(&bin, &binary).unwrap_err();
        assert_eq!(e.kind, FailureKind::Foreign);
        assert_eq!(fs::read_to_string(&path).unwrap(), "someone else's");
        let e = uninstall(&bin, &binary).unwrap_err();
        assert_eq!(e.kind, FailureKind::Foreign);
        assert!(path.is_file());

        // a link to a papers-rs that is not the app's: foreign too
        fs::remove_file(&path).unwrap();
        let other = tmp.path().join("cargo/papers-rs");
        fs::create_dir(other.parent().unwrap()).unwrap();
        fs::write(&other, "cargo's").unwrap();
        link(&other, &path).unwrap();
        assert_eq!(install(&bin, &binary).unwrap_err().kind, FailureKind::Foreign);
        assert_eq!(uninstall(&bin, &binary).unwrap_err().kind, FailureKind::Foreign);
        assert_eq!(fs::read_link(&path).unwrap(), other);
        assert_eq!(status(Some(&bin), Some(&binary)).state, "foreign");
    }

    #[test]
    fn uninstall_only_our_link() {
        let (_tmp, binary, bin) = setup();
        assert_eq!(uninstall(&bin, &binary).unwrap(), Done::Absent);
        install(&bin, &binary).unwrap();
        assert_eq!(uninstall(&bin, &binary).unwrap(), Done::Removed);
        assert!(fs::symlink_metadata(bin.join(NAME)).is_err());
        assert!(binary.is_file());
    }

    #[test]
    fn denied_in_a_folder_that_cannot_be_written() {
        use std::os::unix::fs::PermissionsExt;
        let (_tmp, binary, bin) = setup();
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o555)).unwrap();
        let result = install(&bin, &binary);
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
        // root can write anywhere
        if let Err(e) = result {
            assert_eq!(e.kind, FailureKind::Denied);
        }
    }

    #[test]
    fn status_tells_where_the_link_points() {
        let (_tmp, binary, bin) = setup();
        let state = |binary: Option<&Path>| status(Some(&bin), binary);
        assert_eq!(state(Some(&binary)).state, "missing");
        assert!(!state(None).available);
        install(&bin, &binary).unwrap();
        let status = state(Some(&binary));
        assert!(status.available && status.current);
        assert_eq!(status.state, "ours");
        assert_eq!(status.points_to, Some(binary));
    }

    #[test]
    fn placeholder_is_not_a_binary() {
        let (_tmp, binary, _) = setup();
        assert!(!is_placeholder(&binary));
        fs::write(&binary, "#!/bin/sh\n# papers-rs-placeholder\nexit 1\n").unwrap();
        assert!(is_placeholder(&binary));
    }

    #[test]
    fn scripts_quote_the_paths() {
        let dir = Path::new("/usr/local/bin");
        let binary = Path::new("/Applications/Papers Desktop.app/Contents/MacOS/papers-rs");
        assert_eq!(
            script(dir, binary, false),
            "mkdir -p '/usr/local/bin' && ln -sfn '/Applications/Papers Desktop.app/Contents/MacOS/papers-rs' '/usr/local/bin/papers-rs'"
        );
        assert_eq!(script(dir, binary, true), "rm '/usr/local/bin/papers-rs'");
        assert_eq!(
            terminal_command(dir, binary, false),
            "sudo mkdir -p '/usr/local/bin' && sudo ln -sfn '/Applications/Papers Desktop.app/Contents/MacOS/papers-rs' '/usr/local/bin/papers-rs'"
        );
        assert_eq!(quoted("it's"), r"'it'\''s'");
    }

    #[test]
    fn applescript_escapes_its_strings() {
        let text = applescript(r#"ln -s 'a "b"' c"#, false);
        assert_eq!(
            text,
            r#"do shell script "ln -s 'a \"b\"' c" with prompt "Papers Desktop wants to install the papers-rs command." with administrator privileges"#
        );
    }
}
