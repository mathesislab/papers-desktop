//! The `papers-rs` command line tool that comes with the app (Tauri's sidecar,
//! `papers-rs-cli` next to the executable), installed as a link in `~/.local/bin` (macOS and
//! Linux alike: no administrator prompt). If that folder is not in the PATH of
//! the user's shell, the settings say so and offer to add it to the startup
//! file of the shell, on a click. Only a link of ours is ever replaced or
//! removed.

use std::fs;
use std::io::{self, ErrorKind, Read};
use std::path::{Path, PathBuf};

use serde::Serialize;

/// the name of the command, and of the link
pub const NAME: &str = "papers-rs";
/// the name of the sidecar: not that of the command, which tauri-build would
/// copy over cargo's own `target/<profile>/papers-rs`
const SIDECAR: &str = "papers-rs-cli";
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
    /// not allowed to write there
    Denied,
    Failed,
}

/// Why an install or an uninstall did not happen, as the page shows it.
#[derive(Debug, Clone, Serialize)]
pub struct Failure {
    pub kind: FailureKind,
    pub message: String,
}

impl Failure {
    fn new(kind: FailureKind, message: impl Into<String>) -> Failure {
        Failure {
            kind,
            message: message.into(),
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
/// or to the sidecar of another copy of the app (named as the app's folder
/// on macOS, or next to the app's executable; `papers-rs` up to 0.6).
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
        || (target.file_name().is_some_and(|n| n == SIDECAR || n == NAME)
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

/// The folder of the link, in the home folder.
pub fn local_bin(home: &Path) -> PathBuf {
    home.join(".local/bin")
}

fn default_dir() -> Option<PathBuf> {
    cfg!(unix).then(std::env::home_dir).flatten().map(|home| local_bin(&home))
}

/// The sidecar of this app: next to its executable, once built by
/// scripts/sidecar.sh (a development build has the placeholder: none).
pub fn bundled_binary() -> Option<PathBuf> {
    let path = std::env::current_exe().ok()?.parent()?.join(SIDECAR);
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

/// The change, off the thread of the page.
async fn run(change: fn() -> Result<Done, Failure>) -> Result<Status, Failure> {
    tauri::async_runtime::spawn_blocking(change)
        .await
        .map_err(|e| Failure::new(FailureKind::Failed, e.to_string()))??;
    Ok(current_status())
}

fn install_here() -> Result<Done, Failure> {
    let (dir, binary) = target()?;
    install(&dir, &binary)
}

fn uninstall_here() -> Result<Done, Failure> {
    let (dir, binary) = target()?;
    uninstall(&dir, &binary)
}

/// The shells whose startup file we can add the folder to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    Zsh,
    Bash,
    Fish,
    Other,
}

impl Shell {
    /// From the `SHELL` variable: `/bin/zsh`, `/opt/homebrew/bin/fish`, …
    pub fn of(shell: &str) -> Shell {
        match Path::new(shell).file_name().and_then(|n| n.to_str()) {
            Some("zsh") => Shell::Zsh,
            Some("bash") => Shell::Bash,
            Some("fish") => Shell::Fish,
            _ => Shell::Other,
        }
    }

    /// The line that puts `~/.local/bin` in the PATH.
    pub fn line(self) -> &'static str {
        match self {
            Shell::Fish => "fish_add_path ~/.local/bin",
            _ => r#"export PATH="$HOME/.local/bin:$PATH""#,
        }
    }

    /// The startup file for a shell in a terminal: on macOS a terminal runs
    /// a login shell, which bash starts from the first of three profiles
    /// that exists; elsewhere bash reads `.bashrc` in an interactive shell.
    pub fn rc_file(self, home: &Path, macos: bool) -> Option<PathBuf> {
        match self {
            Shell::Zsh => Some(home.join(".zshrc")),
            Shell::Fish => Some(home.join(".config/fish/config.fish")),
            Shell::Bash if macos => [".bash_profile", ".bash_login", ".profile"]
                .iter()
                .map(|name| home.join(name))
                .find(|file| file.exists())
                .or_else(|| Some(home.join(".bash_profile"))),
            Shell::Bash => Some(home.join(".bashrc")),
            Shell::Other => None,
        }
    }
}

/// the comment that starts the block of ours in a startup file
const MARK: &str = "# added by Papers Desktop: the papers-rs command line tool";

/// What the settings show of the PATH.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PathAdvice {
    /// `~/.local/bin` is in the PATH of the user's shell
    pub on_path: bool,
    /// what to add, for the shell
    pub line: &'static str,
    /// the startup file the button adds it to: none for a shell we do not know
    pub file: Option<PathBuf>,
    /// the block of ours is in that file already
    pub added: bool,
}

/// Whether `dir` is one of the folders of a PATH.
pub fn path_has(path: &str, dir: &Path) -> bool {
    std::env::split_paths(path).any(|folder| folder == dir)
}

/// The advice for the PATH of a shell (None: it could not be asked).
pub fn advice(home: &Path, shell: Shell, path: Option<&str>, macos: bool) -> PathAdvice {
    let file = shell.rc_file(home, macos);
    PathAdvice {
        on_path: path.is_some_and(|p| path_has(p, &local_bin(home))),
        line: shell.line(),
        added: file
            .as_deref()
            .and_then(|f| fs::read_to_string(f).ok())
            .is_some_and(|text| text.contains(MARK)),
        file,
    }
}

/// Append the block of ours to the startup file of the shell, once; the file
/// and its folder are made if missing.
pub fn add_to_rc(home: &Path, shell: Shell, macos: bool) -> Result<Done, Failure> {
    let file = shell.rc_file(home, macos).ok_or_else(|| {
        Failure::new(FailureKind::Failed, "Not a shell that Papers Desktop knows: add the line yourself")
    })?;
    let text = match fs::read_to_string(&file) {
        Ok(text) => text,
        Err(e) if e.kind() == ErrorKind::NotFound => String::new(),
        Err(e) => return Err(Failure::io(&file, &e)),
    };
    if text.contains(MARK) {
        return Ok(Done::Already);
    }
    let mut block = String::new();
    if !text.is_empty() {
        if !text.ends_with('\n') {
            block.push('\n');
        }
        block.push('\n');
    }
    block.push_str(&format!("{MARK}\n{}\n", shell.line()));
    let append = || -> io::Result<()> {
        use std::io::Write;
        if let Some(folder) = file.parent() {
            fs::create_dir_all(folder)?;
        }
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file)?
            .write_all(block.as_bytes())
    };
    append().map_err(|e| Failure::io(&file, &e))?;
    Ok(Done::Installed)
}

fn shell_of_user() -> Shell {
    Shell::of(&std::env::var("SHELL").unwrap_or_default())
}

fn home() -> Result<PathBuf, Failure> {
    std::env::home_dir().ok_or_else(|| Failure::new(FailureKind::Failed, "No home folder"))
}

fn current_advice() -> Result<PathAdvice, Failure> {
    // as a terminal starts the shell: login and interactive, which reads the startup files
    let path = crate::writer::interactive_login_path();
    Ok(advice(&home()?, shell_of_user(), path.as_deref(), cfg!(target_os = "macos")))
}

/// Whether `~/.local/bin` is in the PATH of the user's shell: it starts the
/// shell, so off the thread of the page.
#[tauri::command]
pub async fn cli_path() -> Result<PathAdvice, Failure> {
    tauri::async_runtime::spawn_blocking(current_advice)
        .await
        .map_err(|e| Failure::new(FailureKind::Failed, e.to_string()))?
}

/// The button of the settings: the block into the startup file, then the
/// advice again.
#[tauri::command]
pub async fn add_cli_path() -> Result<PathAdvice, Failure> {
    tauri::async_runtime::spawn_blocking(|| {
        add_to_rc(&home()?, shell_of_user(), cfg!(target_os = "macos"))?;
        current_advice()
    })
    .await
    .map_err(|e| Failure::new(FailureKind::Failed, e.to_string()))?
}

/// `~/…` for a path in the home folder.
#[cfg(target_os = "macos")]
fn tilde(path: &Path) -> String {
    match std::env::home_dir().as_deref().and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// "Install Command Line Tool…" of the app menu (macOS): installs, then says
/// so, with a note if `~/.local/bin` is not in the PATH of the shell (the
/// settings add it).
#[cfg(target_os = "macos")]
pub fn install_from_menu(app: &tauri::AppHandle) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
    let app = app.clone();
    std::thread::spawn(move || {
        let (kind, text) = match install_here() {
            Ok(done) => {
                let dir = tilde(&default_dir().unwrap_or_default());
                let mut text = match done {
                    Done::Already => format!("papers-rs is already installed in {dir}."),
                    _ => format!("papers-rs is installed in {dir}."),
                };
                if current_advice().is_ok_and(|advice| !advice.on_path) {
                    text.push_str(&format!(
                        "\n\n{dir} is not in the PATH of your shell. Settings (General, Command line tool) add it to your startup file, or add this line:\n\n{}",
                        shell_of_user().line()
                    ));
                }
                (MessageDialogKind::Info, text)
            }
            Err(e) => (MessageDialogKind::Error, e.message),
        };
        app.dialog()
            .message(text)
            .title("Command Line Tool")
            .kind(kind)
            .show(|_| {});
    });
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
        let binary = app.join(SIDECAR);
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
        link(old.join(SIDECAR), bin.join(NAME)).unwrap();
        assert_eq!(link_state(&bin, &binary), Link::Ours(old.join(SIDECAR)));
        assert_eq!(install(&bin, &binary).unwrap(), Done::Installed);
        assert_eq!(fs::read_link(bin.join(NAME)).unwrap(), binary);
    }

    #[test]
    fn a_link_to_the_sidecar_of_0_6_is_pointed_again() {
        let (tmp, binary, bin) = setup();
        // named as the command then; gone from the app updated since
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
    fn installs_in_local_bin_of_the_home() {
        let (tmp, binary, _) = setup();
        let home = tmp.path().join("home");
        let dir = local_bin(&home);
        assert_eq!(dir, home.join(".local/bin"));
        // the home has no .local/bin yet
        assert_eq!(install(&dir, &binary).unwrap(), Done::Installed);
        assert_eq!(fs::read_link(dir.join(NAME)).unwrap(), binary);
        assert_eq!(uninstall(&dir, &binary).unwrap(), Done::Removed);
        assert!(dir.is_dir() && fs::symlink_metadata(dir.join(NAME)).is_err());
    }

    #[test]
    fn local_bin_in_a_path() {
        let dir = Path::new("/home/me/.local/bin");
        assert!(path_has("/usr/bin:/home/me/.local/bin:/bin", dir));
        assert!(path_has("/home/me/.local/bin/", dir));
        assert!(!path_has("/usr/bin:/home/me/.local/bin2:/home/me/.local", dir));
        assert!(!path_has("", dir));
    }

    #[test]
    fn shells_and_their_files() {
        let home = Path::new("/h");
        assert_eq!(Shell::of("/bin/zsh"), Shell::Zsh);
        assert_eq!(Shell::of("/usr/local/bin/bash"), Shell::Bash);
        assert_eq!(Shell::of("/opt/homebrew/bin/fish"), Shell::Fish);
        assert_eq!(Shell::of("/bin/tcsh"), Shell::Other);
        assert_eq!(Shell::of(""), Shell::Other);
        assert_eq!(Shell::Zsh.rc_file(home, true), Some(home.join(".zshrc")));
        assert_eq!(Shell::Bash.rc_file(home, false), Some(home.join(".bashrc")));
        assert_eq!(Shell::Bash.rc_file(home, true), Some(home.join(".bash_profile")));
        assert_eq!(Shell::Fish.rc_file(home, false), Some(home.join(".config/fish/config.fish")));
        assert_eq!(Shell::Other.rc_file(home, false), None);
        assert_eq!(Shell::Zsh.line(), r#"export PATH="$HOME/.local/bin:$PATH""#);
        assert_eq!(Shell::Fish.line(), "fish_add_path ~/.local/bin");
    }

    #[test]
    fn bash_on_macos_reads_the_first_profile_there_is() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        fs::write(home.join(".profile"), "").unwrap();
        assert_eq!(Shell::Bash.rc_file(home, true), Some(home.join(".profile")));
        fs::write(home.join(".bash_profile"), "").unwrap();
        assert_eq!(Shell::Bash.rc_file(home, true), Some(home.join(".bash_profile")));
    }

    #[test]
    fn the_block_is_added_once_per_shell() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        for (shell, name, line) in [
            (Shell::Zsh, ".zshrc", Shell::Zsh.line()),
            (Shell::Bash, ".bashrc", Shell::Bash.line()),
            (Shell::Fish, ".config/fish/config.fish", "fish_add_path ~/.local/bin"),
        ] {
            let file = home.join(name);
            assert!(!advice(home, shell, None, false).added);
            assert_eq!(add_to_rc(home, shell, false).unwrap(), Done::Installed);
            assert_eq!(fs::read_to_string(&file).unwrap(), format!("{MARK}\n{line}\n"));
            assert!(advice(home, shell, None, false).added);
            assert_eq!(add_to_rc(home, shell, false).unwrap(), Done::Already);
            assert_eq!(fs::read_to_string(&file).unwrap(), format!("{MARK}\n{line}\n"));
        }
        // an unknown shell: nothing is written
        assert!(add_to_rc(home, Shell::Other, false).is_err());
    }

    #[test]
    fn the_block_goes_after_what_the_file_has() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join(".zshrc");
        fs::write(&file, "alias a=b").unwrap();
        add_to_rc(tmp.path(), Shell::Zsh, false).unwrap();
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            format!("alias a=b\n\n{MARK}\n{}\n", Shell::Zsh.line())
        );
    }

    #[test]
    fn advice_from_a_path() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let path = format!("/usr/bin:{}", local_bin(home).display());
        let zsh = |path: Option<&str>| advice(home, Shell::Zsh, path, true);
        assert!(zsh(Some(&path)).on_path);
        assert!(!zsh(Some("/usr/bin")).on_path);
        assert!(!zsh(None).on_path);
        assert_eq!(zsh(None).file, Some(home.join(".zshrc")));
        assert_eq!(advice(home, Shell::Other, None, true).file, None);
    }
}
