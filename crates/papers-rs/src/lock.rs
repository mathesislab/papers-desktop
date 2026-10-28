//! The locks of the libraries: a command that writes a library holds its
//! lock, `.papers/lock`, so that two commands (or papers and another program
//! that follows the same rules) do not write it at the same time.
//!
//! A lock is a lease: a file created only if it does not exist, with a JSON
//! object `{token, pid, host, command}`. Its holder rewrites it every
//! `REFRESH` seconds (a thread), as long as the command runs. A waiting
//! command takes the lock over when it is stale:
//!
//! - the file did not change for `LIFETIME` seconds, measured on the clock of
//!   the waiting command (the clocks of two computers are never compared);
//! - or it names this computer and a process that no longer exists.
//!
//! Before writing the library, the holder checks that the lock still has its
//! token (see `Lock::check`): a holder that could not refresh it in time (e.g.
//! the computer was asleep) stops instead of writing over the command that
//! took the lock over. The holder removes the lock at the end of the command,
//! if it still has its token (when the `Lock` is dropped).
//!
//! A command locks the library of the command line first, then its pool; a
//! command in the pool locks the pool only: two commands never wait for each
//! other. A lock does not help in a folder synchronized by a service
//! (Dropbox, Nextcloud...): the file reaches the other computers seconds or
//! minutes later.

use std::fs;
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::{Map, Value};

use crate::config::json::{loads, quote};
use crate::config::paths::{dirname, join, short_home};
use crate::config::{Config, config_root};
use crate::error::{Error, Result};
use crate::log;

/// Seconds without refresh after which a lock is stale.
pub const LIFETIME: Duration = Duration::from_secs(10);
/// Seconds between two refreshes by the holder.
pub const REFRESH: Duration = Duration::from_secs(2);
/// Seconds a command waits for a lock that is not stale.
pub const WAIT: Duration = Duration::from_secs(30);
/// Seconds between two looks at the lock of another command.
pub const POLL: Duration = Duration::from_millis(100);

pub const LOCK_FILE: &str = "lock";

/// The lock file of a library (`.papers/lock`), or `None` (a bibtex without
/// library).
pub fn lock_path(config: &Config) -> Option<String> {
    let file = config.file.as_deref()?;
    if !config.is_library {
        return None;
    }
    Some(join(&join(&config_root(file), ".papers"), LOCK_FILE))
}

#[derive(Debug, Clone)]
struct Content {
    token: String,
    pid: u32,
    host: String,
    command: String,
}

impl Content {
    fn new(command: &str) -> Content {
        Content { token: random_token(), pid: std::process::id(), host: hostname(), command: command.to_string() }
    }

    /// `json.dump(content)`: Python's default separators.
    fn to_json(&self) -> String {
        format!(
            "{{\"token\": {}, \"pid\": {}, \"host\": {}, \"command\": {}}}",
            quote(&self.token),
            self.pid,
            quote(&self.host),
            quote(&self.command)
        )
    }
}

fn random_token() -> String {
    use std::hash::{BuildHasher, Hasher, RandomState};
    let part = |salt: u64| {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u64(salt ^ std::process::id() as u64);
        hasher.finish()
    };
    format!("{:016x}{:016x}", part(1), part(2))
}

fn hostname() -> String {
    let mut buffer = [0u8; 256];
    // SAFETY: the buffer is valid for its length, and gethostname writes at most that many bytes
    let status = unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len()) };
    if status != 0 {
        return String::new();
    }
    let end = buffer.iter().position(|b| *b == 0).unwrap_or(buffer.len());
    String::from_utf8_lossy(&buffer[..end]).into_owned()
}

/// The content of a lock file: `Some({})` if it cannot be read (being
/// written), `None` if it does not exist.
fn read(path: &str) -> Option<Map<String, Value>> {
    match fs::read_to_string(path) {
        Ok(text) => match loads(&text) {
            Ok(Value::Object(map)) => Some(map),
            _ => Some(Map::new()),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => Some(Map::new()),
    }
}

/// What changes when the lock file is rewritten (`None`: no file).
fn signature(path: &str) -> Option<(i64, i64, u64, u64)> {
    let meta = fs::metadata(path).ok()?;
    Some((meta.mtime(), meta.mtime_nsec(), meta.size(), meta.ino()))
}

/// Rewrite the lock (a refresh): through a temporary file, so that a reader
/// never sees half of it.
fn write(path: &str, content: &Content) -> std::io::Result<()> {
    let tmp = format!("{path}.{}.tmp", std::process::id());
    fs::write(&tmp, content.to_json())?;
    fs::rename(&tmp, path)
}

/// Create the lock file if it does not exist; return whether it was created.
fn create(path: &str, content: &Content) -> Result<bool> {
    match fs::OpenOptions::new().write(true).create_new(true).mode(0o644).open(path) {
        Ok(mut file) => {
            file.write_all(content.to_json().as_bytes())?;
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(e) => Err(e.into()),
    }
}

/// The lock names this computer and a process that no longer exists.
fn process_gone(other: &Map<String, Value>) -> bool {
    let Some(pid) = other.get("pid").and_then(|p| p.as_i64()) else { return false };
    if other.get("host").and_then(|h| h.as_str()) != Some(hostname().as_str()) {
        return false;
    }
    // SAFETY: signal 0 only checks that the process exists
    let status = unsafe { libc::kill(pid as libc::pid_t, 0) };
    status != 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

fn describe(other: &Option<Map<String, Value>>) -> String {
    match other {
        Some(other) if !other.is_empty() => {
            let command = other.get("command").and_then(|c| c.as_str()).filter(|c| !c.is_empty()).unwrap_or("another command");
            let show = |key: &str| match other.get(key) {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Null) | None => "None".to_string(),
                Some(v) => crate::config::json::py_str(v),
            };
            format!("{command} (pid {} on {})", show("pid"), show("host"))
        }
        _ => "another command".to_string(),
    }
}

/// A lock held by this process: removed (if it still has its token) when
/// dropped.
pub struct Lock {
    path: String,
    content: Arc<Content>,
    stop: Arc<(Mutex<bool>, std::sync::Condvar)>,
    lost: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

/// Take the lock of the library of `config` (nothing for a bibtex without
/// library), waiting up to `wait` (`WAIT` by default) for the command that
/// holds it; an error after. `command` says what holds it (e.g. `papers add
/// x.pdf`).
pub fn acquire(config: &Config, command: &str, wait: Option<Duration>) -> Result<Option<Lock>> {
    let Some(path) = lock_path(config) else { return Ok(None) };
    let dir = dirname(&path);
    fs::create_dir_all(&dir)?;
    let content = Content::new(command);
    let wait = wait.unwrap_or(WAIT);
    let start = Instant::now();
    let mut seen: Option<(i64, i64, u64, u64)> = None;
    let mut since = start;
    let mut told = false;
    loop {
        if create(&path, &content)? {
            break;
        }
        let other = read(&path);
        let current = signature(&path);
        let now = Instant::now();
        if current != seen {
            seen = current;
            since = now;
        }
        let stale = current.is_some()
            && (now.duration_since(since) >= LIFETIME || other.as_ref().is_some_and(|o| !o.is_empty() && process_gone(o)));
        if stale {
            log::info(&format!("{}: stale lock of {}, taken over", short_home(&path), describe(&other)));
            let _ = fs::remove_file(&path);
            seen = None;
            continue;
        }
        let libdir = short_home(&dirname(&dirname(&path)));
        if now.duration_since(start) >= wait {
            return Err(Error::exit(format!(
                "{libdir} is locked by {}: try again when it is done (the lock is {path})",
                describe(&other)
            )));
        }
        if !told && now.duration_since(start) >= Duration::from_secs(1) {
            log::warning(&format!("waiting for {libdir}: locked by {}", describe(&other)));
            told = true;
        }
        std::thread::sleep(POLL);
    }
    let content = Arc::new(content);
    let stop = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let lost = Arc::new(AtomicBool::new(false));
    let thread = {
        let (path, content, stop) = (path.clone(), Arc::clone(&content), Arc::clone(&stop));
        std::thread::Builder::new().name("papers-lock".into()).spawn(move || refresh(&path, &content, &stop)).ok()
    };
    Ok(Some(Lock { path, content, stop, lost, thread }))
}

fn refresh(path: &str, content: &Content, stop: &(Mutex<bool>, std::sync::Condvar)) {
    let (flag, condvar) = stop;
    loop {
        let guard = flag.lock().expect("the stop flag");
        let (guard, _) = condvar.wait_timeout_while(guard, REFRESH, |stopped| !*stopped).expect("the stop flag");
        if *guard {
            return;
        }
        drop(guard);
        let ours = read(path).and_then(|o| o.get("token").and_then(|t| t.as_str().map(str::to_string)));
        if ours.as_deref() == Some(content.token.as_str())
            && let Err(e) = write(path, content)
        {
            log::debug(&format!("{path}: lock not refreshed: {e}"));
        }
    }
}

impl Lock {
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Before writing the library: an error if this process took its lock
    /// and lost it (not refreshed in time, taken over).
    pub fn check(&self) -> Result<()> {
        if self.lost.load(Ordering::Relaxed) {
            return Err(Error::exit(format!(
                "{}: the lock was taken over: nothing more is written",
                short_home(&self.path)
            )));
        }
        let other = read(&self.path);
        match &other {
            Some(o) if !o.is_empty() && o.get("token").and_then(|t| t.as_str()) != Some(self.content.token.as_str()) => {
                self.lost.store(true, Ordering::Relaxed);
                Err(Error::exit(format!(
                    "{}: the lock was taken over by {} (this command could not refresh it in time): nothing more is written",
                    short_home(&self.path),
                    describe(&other)
                )))
            }
            None => {
                // removed by a command that found it stale: taken again, if still free
                if create(&self.path, &self.content)? {
                    Ok(())
                } else {
                    self.lost.store(true, Ordering::Relaxed);
                    Err(Error::exit(format!(
                        "{}: the lock was taken over by {}: nothing more is written",
                        short_home(&self.path),
                        describe(&read(&self.path))
                    )))
                }
            }
            _ => Ok(()),
        }
    }

    /// Remove the lock now (the same as dropping it).
    pub fn release(self) {}
}

impl Drop for Lock {
    fn drop(&mut self) {
        {
            let (flag, condvar) = &*self.stop;
            *flag.lock().expect("the stop flag") = true;
            condvar.notify_all();
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let ours = read(&self.path).and_then(|o| o.get("token").and_then(|t| t.as_str().map(str::to_string)));
        if ours.as_deref() == Some(self.content.token.as_str()) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::MachineConfig;

    fn library(dir: &std::path::Path) -> Config {
        let mut config = Config::new(MachineConfig::empty());
        config.file = Some(format!("{}/.papers/config.json", dir.display()));
        config.is_library = true;
        config
    }

    #[test]
    fn takes_and_releases_the_lock() {
        let dir = tempfile::tempdir().unwrap();
        let config = library(dir.path());
        let lock = acquire(&config, "papers add x", None).unwrap().unwrap();
        let path = lock.path().to_string();
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.starts_with("{\"token\": \""), "{content}");
        assert!(content.contains("\"command\": \"papers add x\""));
        lock.check().unwrap();
        // a second command waits, then gives up
        let error = acquire(&config, "papers rm y", Some(Duration::from_millis(300))).err().unwrap();
        assert!(error.message().contains("is locked by papers add x (pid "), "{error}");
        drop(lock);
        assert!(!std::path::Path::new(&path).exists());
    }

    #[test]
    fn takes_over_the_lock_of_a_dead_process() {
        let dir = tempfile::tempdir().unwrap();
        let config = library(dir.path());
        let path = lock_path(&config).unwrap();
        fs::create_dir_all(dirname(&path)).unwrap();
        // a process that is gone: a child that has ended
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        let dead = Content { token: "dead".into(), pid, host: hostname(), command: "papers old".into() };
        fs::write(&path, dead.to_json()).unwrap();
        let lock = acquire(&config, "papers new", Some(Duration::from_secs(2))).unwrap().unwrap();
        lock.check().unwrap();
    }

    #[test]
    fn a_lock_taken_over_is_detected() {
        let dir = tempfile::tempdir().unwrap();
        let config = library(dir.path());
        let lock = acquire(&config, "papers add", None).unwrap().unwrap();
        let other = Content::new("papers other");
        fs::write(lock.path(), other.to_json()).unwrap();
        let error = lock.check().unwrap_err();
        assert!(error.message().contains("the lock was taken over by papers other"), "{error}");
    }

    #[test]
    fn no_lock_without_library() {
        let config = Config::new(MachineConfig::empty());
        assert!(acquire(&config, "papers x", None).unwrap().is_none());
    }
}
