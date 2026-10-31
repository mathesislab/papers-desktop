//! The `feeds` program: the commands of the feeds crate on a folder, as paper-feeder's job
//! and more (subscribe, fetch, list, show, mark, the interests, rules and saved searches, the
//! import of a paper-feeder setup, the RSS digest). With `--library`, the .bib files of a library
//! tell how like its papers an item is (`like:`).
//!
//! [`run`] is the whole program, on a [`Host`] (the environment, the clock, where requests
//! go) and two writers, so that the tests run it with recorded replies.

mod cli;
mod digest;
mod items;
mod paperfeeder;
mod searches;
mod settings;
mod subs;
mod text;

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use clap::Parser;
use papers_core::{Library, Likeness};
use papers_feeds::http::{Paced, Pacing};
use papers_feeds::{Access, Feeds, Http, Network};

use crate::cli::{AccessArgs, Cli, Command};

/// The environment: the value of a variable.
pub type Env<'a> = Box<dyn Fn(&str) -> Option<String> + 'a>;

/// What the program runs on: the environment, the clock, and the network.
pub struct Host<'a> {
    /// an environment variable
    pub env: Env<'a>,
    pub now: Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>,
    /// where the requests go; none: the network, paced
    pub http: Option<&'a dyn Http>,
}

impl Host<'_> {
    /// The process's environment and clock, and the network.
    pub fn system() -> Host<'static> {
        Host {
            env: Box::new(|name| std::env::var(name).ok()),
            now: Arc::new(Utc::now),
            http: None,
        }
    }
}

/// What ends a command without its result.
#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error(transparent)]
    Feeds(#[from] papers_feeds::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Library(#[from] papers_core::Error),
    #[error("{}: {source}", path.display())]
    File {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{0}")]
    Message(String),
}

pub(crate) type Result<T> = std::result::Result<T, Error>;

/// An error of a command with its message.
pub(crate) fn fail<T>(message: impl Into<String>) -> Result<T> {
    Err(Error::Message(message.into()))
}

/// What a command works with: the host, the folder, the library, the writers.
pub(crate) struct Ctx<'a> {
    pub host: &'a Host<'a>,
    pub dir: PathBuf,
    /// the library of `--library`, and the index of its papers
    pub library: Option<(Library, Arc<Likeness>)>,
    pub out: &'a mut dyn Write,
    pub err: &'a mut dyn Write,
}

impl Ctx<'_> {
    /// The feeds of the folder, on the host's clock, with the index of the library if given.
    pub fn open(&self) -> Result<Feeds> {
        let now = self.host.now.clone();
        let mut feeds = Feeds::open_with(&self.dir, Box::new(move || now()))?;
        feeds.set_likeness(self.library.as_ref().map(|(_, index)| index.clone()));
        Ok(feeds)
    }

    /// An environment variable, if it is set and not empty.
    pub fn env(&self, name: &str) -> Option<String> {
        (self.host.env)(name).filter(|v| !v.trim().is_empty())
    }

    /// What the APIs are asked with: the option, else the environment.
    pub fn access(&self, args: &AccessArgs) -> Access {
        let option = |value: &Option<String>, name: &str| {
            value
                .clone()
                .filter(|v| !v.is_empty())
                .or_else(|| self.env(name))
        };
        Access {
            mailto: option(&args.mailto, "FEEDS_MAILTO"),
            openalex_key: option(&args.openalex_key, "OPENALEX_KEY"),
        }
    }

    /// Runs `f` on the host's recorded replies, or on the network paced for `access`.
    pub fn with_http<T>(&self, access: &Access, f: impl FnOnce(&dyn Http) -> T) -> Result<T> {
        match self.host.http {
            Some(http) => Ok(f(http)),
            None => {
                let network = Network::new().map_err(Error::Message)?;
                Ok(f(&Paced::new(network, Pacing::polite(access))))
            }
        }
    }
}

/// The folder: `--dir`, else `FEEDS_DIR`, else `papers-feeds/` in the data folder of the platform.
fn folder(host: &Host, option: Option<PathBuf>) -> Result<PathBuf> {
    let env = (host.env)("FEEDS_DIR").filter(|v| !v.is_empty());
    option
        .or_else(|| env.map(PathBuf::from))
        .or_else(|| dirs::data_dir().map(|d| d.join("papers-feeds")))
        .map_or_else(|| fail("no data folder on this system: use --dir"), Ok)
}

/// The library of these .bib files and the index of its papers; none without a file.
fn library(bibs: &[PathBuf]) -> Result<Option<(Library, Arc<Likeness>)>> {
    if bibs.is_empty() {
        return Ok(None);
    }
    let library = Library::load(bibs)?;
    let index = Arc::new(Likeness::of(&library));
    Ok(Some((library, index)))
}

/// Runs the program on its arguments (the first is its name); the exit status. What a
/// command prints goes to `out`, warnings and errors to `err`.
pub fn run(args: Vec<String>, host: &Host, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(e) => {
            let code = e.exit_code();
            let _ = match code {
                0 => write!(out, "{}", e.render()),
                _ => write!(err, "{}", e.render()),
            };
            return code;
        }
    };
    let result = folder(host, cli.dir).and_then(|dir| {
        let mut ctx = Ctx {
            host,
            dir,
            library: library(&cli.library)?,
            out,
            err,
        };
        dispatch(&mut ctx, cli.command)
    });
    match result {
        Ok(()) => 0,
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::BrokenPipe => 0,
        Err(e) => {
            let _ = writeln!(err, "feeds: error: {e}");
            1
        }
    }
}

/// The command of the command line.
fn dispatch(ctx: &mut Ctx, command: Command) -> Result<()> {
    match command {
        Command::Subscribe(a) => subs::subscribe(ctx, a),
        Command::Edit(a) => subs::edit(ctx, a),
        Command::Unsubscribe(a) => subs::unsubscribe(ctx, a),
        Command::Subscriptions(a) => subs::subscriptions(ctx, a),
        Command::Fetch(a) => subs::fetch(ctx, a),
        Command::List(a) => items::list(ctx, a),
        Command::Show(a) => items::show(ctx, a),
        Command::Find(a) => items::find(ctx, a),
        Command::Mark(a) => items::mark(ctx, a),
        Command::Prune => items::prune(ctx),
        Command::Interests(a) => settings::interests(ctx, a),
        Command::Rules(a) => settings::rules(ctx, a),
        Command::Searches(a) => searches::searches(ctx, a),
        Command::Config(a) => settings::config(ctx, a),
        Command::Opml(a) => subs::opml(ctx, a),
        Command::ImportPaperFeeder(a) => paperfeeder::import(ctx, a),
        Command::Digest(a) => digest::digest(ctx, a),
    }
}
