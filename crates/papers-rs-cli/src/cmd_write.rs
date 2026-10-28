//! What the commands that write a library share on the command line: the
//! library of the command (`--bibtex`, `--filesdir`), the options of the
//! write path, the questions put to the terminal, and what is printed at the
//! end (the lines of `papers_rs::command`).

use std::io::{BufRead, IsTerminal, Write};
use std::sync::OnceLock;

use papers_rs::command::{Options, Outcome, Prompt, Session};
use papers_rs::config::Config;
use papers_rs::error::Error;
use papers_rs::log;

use crate::cmd_init::format_options;
use crate::parse::Leaf;

/// What ends a command other than its result.
pub enum Failure {
    /// An error of the command line (`papers edit: error: ...`, status 2).
    Usage(String),
    Error(Error),
}

impl From<Error> for Failure {
    fn from(error: Error) -> Failure {
        Failure::Error(error)
    }
}

pub type Done = std::result::Result<(), Failure>;

static COMMAND_LINE: OnceLock<String> = OnceLock::new();

/// The command line as given (before `-C DIR` is taken): the message of the history.
pub fn set_command_line(argv: &[String]) {
    let _ = COMMAND_LINE.set(format!("papers {}", argv.join(" ")).trim_end().to_string());
}

pub fn command_line() -> String {
    COMMAND_LINE
        .get()
        .cloned()
        .unwrap_or_else(|| "papers".to_string())
}

/// The configuration of a command that needs a library: as `list` has it (`--bibtex`, `--filesdir`),
/// and the name and key format options applied for this run (`set_format_config_from_cmd`).
pub fn check_install(
    leaf: &Leaf,
    config: Config,
    bibtex_must_exist: bool,
) -> Result<Config, Error> {
    let mut config = crate::cmd_list::check_install(leaf, config, bibtex_must_exist)?;
    for (name, value) in format_options(leaf) {
        // for this run only: the library configuration is not written
        config.set(&name, value)?;
    }
    Ok(config)
}

/// `--color` and the terminal: colors and links in the lines (`color_enabled`).
pub fn color_enabled(leaf: &Leaf) -> bool {
    match leaf.string("color").as_deref() {
        Some("always") => true,
        Some("never") => false,
        _ => {
            std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty())
                && std::io::stdout().is_terminal()
        }
    }
}

/// The questions of a command, put to the terminal: the question on the
/// standard output (the standard error with `--porcelain`), the answer from
/// the standard input.
pub struct Terminal;

impl Prompt for Terminal {
    fn interactive(&self) -> bool {
        std::io::stdin().is_terminal()
    }

    fn input(&mut self, prompt: &str) -> Option<String> {
        log::print(prompt);
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        match std::io::stdin().lock().read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(line.trim_end_matches(['\n', '\r']).to_string()),
        }
    }
}

/// A session for the library of `config`: lines of the standard output go
/// to it as they come, questions to the terminal.
pub fn open_session(leaf: &Leaf, config: Config, assume_yes: bool) -> Result<Session, Error> {
    let options = Options {
        message: command_line(),
        dry_run: leaf.flag("dry_run"),
        assume_yes,
        color: color_enabled(leaf),
        prompt: Box::new(Terminal),
        sink: Some(Box::new(|line: &str| log::println(line))),
    };
    Session::open(config, options)
}

/// The end of a command: the lines that remain, and the porcelain lines on the
/// standard output.
pub fn finish(session: Session) {
    let Outcome { out, porcelain } = session.finish();
    for line in out {
        log::println(&line);
    }
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    for line in porcelain {
        let _ = writeln!(stdout, "{line}");
    }
    let _ = stdout.flush();
}

/// Run a command on a session, and finish the session whatever the result
/// (the file operations done are recorded, the lock is released).
pub fn run_session<T>(
    session: Session,
    command: impl FnOnce(&mut Session) -> Result<T, Failure>,
) -> Result<T, Failure> {
    let mut session = session;
    let result = command(&mut session);
    finish(session);
    result
}

/// Write the file links of the bibtex as the `paths` setting says, relative or
/// absolute (`papers.library.convert_file_links`): the library is loaded with the links as
/// the setting says, and saved if that changed them.
pub fn convert_file_links(config: Config) -> Result<(), Error> {
    let options = Options {
        message: command_line(),
        sink: Some(Box::new(|line: &str| log::println(line))),
        ..Options::default()
    };
    let mut session = Session::open(config, options)?;
    session.no_auto_push();
    // links to the files of the directory of the bibtex: written relative to it, as the setting says
    let relative_to = session.library.relative_to.clone();
    session.library.set_relative_to(&relative_to);
    let result = session.commit().map(|_| ());
    finish(session);
    result
}
