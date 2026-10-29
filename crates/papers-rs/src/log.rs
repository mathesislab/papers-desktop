//! What papers writes while it works: the messages of its logger (standard
//! error, `papers: warning: MESSAGE`) and its standard output, which
//! `--porcelain` sends to the standard error.

use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

pub const DEBUG: u8 = 10;
pub const INFO: u8 = 20;
pub const WARNING: u8 = 30;
pub const ERROR: u8 = 40;

static LEVEL: AtomicU8 = AtomicU8::new(WARNING);
static PORCELAIN: AtomicBool = AtomicBool::new(false);

/// The level of the logger: messages below it are not written.
pub fn set_level(level: u8) {
    LEVEL.store(level, Ordering::Relaxed);
}

pub fn level() -> u8 {
    LEVEL.load(Ordering::Relaxed)
}

pub fn is_debug() -> bool {
    level() <= DEBUG
}

/// With `--porcelain`, the standard output is the standard error.
pub fn set_porcelain(on: bool) {
    PORCELAIN.store(on, Ordering::Relaxed);
}

pub fn porcelain() -> bool {
    PORCELAIN.load(Ordering::Relaxed)
}

fn log(level_value: u8, name: &str, message: &str) {
    if level_value >= level() {
        let stderr = std::io::stderr();
        let _ = writeln!(stderr.lock(), "papers: {name}: {message}");
    }
}

pub fn debug(message: &str) {
    log(DEBUG, "debug", message);
}

pub fn info(message: &str) {
    log(INFO, "info", message);
}

pub fn warning(message: &str) {
    log(WARNING, "warning", message);
}

pub fn error(message: &str) {
    log(ERROR, "error", message);
}

fn write_out(text: &str, to_stderr: bool) {
    let result = if to_stderr {
        std::io::stderr().lock().write_all(text.as_bytes())
    } else {
        std::io::stdout().lock().write_all(text.as_bytes())
    };
    if let Err(error) = result
        && error.kind() == std::io::ErrorKind::BrokenPipe
    {
        std::process::exit(1);
    }
}

/// Text on the standard output as it is (the standard error with `--porcelain`).
pub fn print(text: &str) {
    write_out(text, porcelain());
}

/// `print()`: a line on the standard output (the standard error with
/// `--porcelain`). A closed pipe ends the process with status 1, as papers
/// does.
pub fn println(message: &str) {
    write_out(&format!("{message}\n"), porcelain());
}

/// Text that is the data of the command (the listing of `papers list`): on the standard output,
/// also with `--porcelain`.
pub fn print_data(text: &str) {
    write_out(text, false);
}

/// A line of the data of the command, as [`print_data`].
pub fn println_data(message: &str) {
    write_out(&format!("{message}\n"), false);
}
