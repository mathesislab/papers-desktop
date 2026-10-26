//! The papers command line, the only reader of the bibtex file.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::{Error, Library, Result};

#[derive(Debug, Clone)]
pub struct Papers {
    exe: PathBuf,
}

impl Papers {
    pub fn new(exe: impl Into<PathBuf>) -> Papers {
        Papers { exe: exe.into() }
    }

    /// The papers executable on the PATH of the user's shell: apps started
    /// from the Finder or at login do not get the PATH the shell sets up.
    pub fn locate() -> Option<PathBuf> {
        if cfg!(windows) {
            return None; // Windows apps get the user PATH: "papers" is enough
        }
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
        let out = Command::new(shell)
            .args(["-ilc", "command -v papers"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::trim)
            .rfind(|l| l.starts_with('/'))
            .map(PathBuf::from)
    }

    pub fn exe(&self) -> &Path {
        &self.exe
    }

    /// The default library, as `papers list --format csl-json` writes it.
    pub fn load(&self) -> Result<Library> {
        Library::from_csl_json(&self.run(&["list", "--format", "csl-json"])?)
    }

    /// The bibtex file of the default library.
    pub fn bibtex_path(&self) -> Result<PathBuf> {
        Ok(PathBuf::from(self.run(&["config", "bibtex"])?.trim()))
    }

    /// The bibtex entry of one key, as papers writes it.
    pub fn bibtex(&self, key: &str) -> Result<String> {
        let bibtex = self.run(&["list", "--strict", "--format", "bibtex", "--key", key])?;
        Ok(bibtex.trim().to_string())
    }

    fn run(&self, args: &[&str]) -> Result<String> {
        let cmd = format!("papers {}", args.join(" "));
        let mut command = Command::new(&self.exe);
        command.args(args).env("NO_COLOR", "1").stdin(Stdio::null());
        // a library in the working directory would take over the default one
        if let Some(home) = std::env::home_dir() {
            command.current_dir(home);
        }
        let out = command.output().map_err(|source| Error::Spawn {
            cmd: cmd.clone(),
            source,
        })?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            let stderr = if stderr.is_empty() {
                out.status.to_string()
            } else {
                stderr
            };
            return Err(Error::Command { cmd, stderr });
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }
}
