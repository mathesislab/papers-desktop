//! A papers command as the writer of the papers libraries: any command line
//! of the papers standard (papers-cli, the `papers-rs` program, ...), run in
//! the folder of the library with `--porcelain`.

use std::io::{ErrorKind as IoErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::{AddOptions, Changes, Kind, PapersWriter, Pasted, WriteError};

/// The first version of papers with the options used here (`edit --from`,
/// `--porcelain`), as `release` orders it.
pub const PAPERS_MIN: [u32; 5] = [3, 0, 0, BETA, 4];
pub const PAPERS_MIN_NAME: &str = "3.0.0b4";

/// The papers command, its version, and the PATH of the login shell to run
/// it with: papers runs git (and git-lfs) from it.
#[derive(Debug, Clone)]
pub struct PapersCommand {
    pub path: PathBuf,
    pub version: String,
    pub shell_path: Option<String>,
}

/// The papers command looked for.
pub enum Found {
    Usable(PapersCommand),
    /// not found, or too old: its version
    Missing(Option<String>),
}

impl PapersCommand {
    /// The papers command of the setting, else the one the login shell finds.
    pub fn find(setting: &Path) -> Found {
        let shell_path = login_path();
        let path = if setting.as_os_str().is_empty() {
            shell_path.as_deref().and_then(in_path)
        } else {
            Some(setting.to_path_buf())
        };
        let Some(path) = path else {
            return Found::Missing(None);
        };
        let Some(version) = version(&path) else {
            return Found::Missing(None);
        };
        if release(&version).is_none_or(|release| release < PAPERS_MIN) {
            return Found::Missing(Some(version));
        }
        Found::Usable(PapersCommand {
            path,
            version,
            shell_path,
        })
    }

    /// Run a papers command in the folder of its library, with `input` on
    /// its stdin; its report of the entries changed. A failure tells what
    /// papers wrote to stderr, without the prefix of its log.
    fn run(
        &self,
        folder: &Path,
        command: &str,
        args: &[String],
        input: Option<&str>,
    ) -> Result<Changes, WriteError> {
        let mut papers = Command::new(&self.path);
        papers
            .arg("-C")
            .arg(folder)
            .args([command, "--porcelain"])
            .args(args)
            .env("NO_COLOR", "1")
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(path) = &self.shell_path {
            papers.env("PATH", path);
        }
        let output = papers.spawn().and_then(|mut child| {
            if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
                stdin.write_all(input.as_bytes())?;
            }
            child.wait_with_output()
        });
        let output = match output {
            Ok(output) => output,
            Err(e) if e.kind() == IoErrorKind::NotFound => return Err(WriteError::missing(None)),
            Err(e) => return Err(WriteError::failed(format!("papers: {e}"))),
        };
        if output.status.success() {
            return Ok(Changes::from_porcelain(&String::from_utf8_lossy(
                &output.stdout,
            )));
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        let lines: Vec<&str> = stderr
            .lines()
            .map(|l| l.strip_prefix("ERROR:papers:").unwrap_or(l))
            .collect();
        let message = lines.join("\n").trim().to_string();
        Err(WriteError::failed(if message.is_empty() {
            format!("papers {command} {}: {}", args.join(" "), output.status)
        } else {
            message
        }))
    }
}

impl PapersWriter for PapersCommand {
    fn kind(&self) -> Kind {
        Kind::Papers
    }

    fn version(&self) -> Option<String> {
        Some(self.version.clone())
    }

    fn tag(
        &self,
        folder: &Path,
        keys: &[String],
        add: &[String],
        remove: &[String],
    ) -> Result<Changes, WriteError> {
        let mut args = keys.to_vec();
        for (option, tags) in [("--add", add), ("--rm", remove)] {
            if !tags.is_empty() {
                args.push(option.to_string());
                args.extend(tags.iter().cloned());
            }
        }
        self.run(folder, "tag", &args, None)
    }

    fn rename_tag(&self, folder: &Path, old: &str, new: &str) -> Result<Changes, WriteError> {
        let args = ["--rename", old, new].map(String::from);
        self.run(folder, "tag", &args, None)
    }

    fn edit(&self, folder: &Path, key: &str, text: &str) -> Result<Changes, WriteError> {
        let args = [key, "--from", "-"].map(String::from);
        self.run(folder, "edit", &args, Some(text))
    }

    /// papers adds a .bib file: the text in a temporary one
    fn add(&self, folder: &Path, text: &str, options: AddOptions) -> Result<Changes, WriteError> {
        let pasted = Pasted::new(text)?;
        let mut args = vec![
            pasted.path().to_string_lossy().into(),
            "--on-duplicate".into(),
            options.on_duplicate.arg().into(),
        ];
        if options.update_key {
            args.push("--update-key".into());
        }
        self.run(folder, "add", &args, None)
    }

    fn remove(&self, folder: &Path, keys: &[String]) -> Result<Changes, WriteError> {
        let mut args = keys.to_vec();
        args.push("--keep-files".to_string());
        self.run(folder, "rm", &args, None)
    }

    fn step(&self, folder: &Path, back: bool) -> Result<(), WriteError> {
        let command = if back { "undo" } else { "redo" };
        self.run(folder, command, &[], None).map(|_| ())
    }
}

/// The PATH of the login shell: an app started from the desktop does not
/// have it.
fn login_path() -> Option<String> {
    let shell = std::env::var_os("SHELL").unwrap_or_else(|| "/bin/sh".into());
    let output = Command::new(shell)
        .args(["-lc", "printf '%s' \"$PATH\""])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (output.status.success() && !path.is_empty()).then_some(path)
}

/// The papers command in the folders of a PATH.
fn in_path(path: &str) -> Option<PathBuf> {
    std::env::split_paths(path)
        .map(|folder| folder.join("papers"))
        .find(|papers| papers.is_file())
}

/// The version of a papers command, e.g. "3.0.0b1".
fn version(papers: &Path) -> Option<String> {
    let output = Command::new(papers)
        .arg("--version")
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let version = stdout
        .split_whitespace()
        .find(|w| w.starts_with(|c: char| c.is_ascii_digit()))?;
    Some(version.to_string())
}

const BETA: u32 = 1;

/// A version of papers as ordered: its three release numbers, its stage
/// (alpha 0, beta 1, release candidate 2, final 3) and the number of the
/// stage. A development build is the version it leads to: 3.0.0b4.dev18 is
/// 3.0.0b4.
pub fn release(version: &str) -> Option<[u32; 5]> {
    let end = version
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(version.len());
    let (numbers, rest) = version.split_at(end);
    let mut numbers = numbers.trim_end_matches('.').split('.');
    let mut number = || numbers.next().map_or(Some(0), |n| n.parse().ok());
    let (major, minor, patch) = (number()?, number()?, number()?);
    let (stage, rest) = [("rc", 2), ("a", 0), ("b", BETA)]
        .into_iter()
        .find_map(|(name, stage)| Some((stage, rest.strip_prefix(name)?)))
        .unwrap_or((3, ""));
    let digits = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    let n = rest[..digits].parse().unwrap_or(0);
    Some([major, minor, patch, stage, n])
}
