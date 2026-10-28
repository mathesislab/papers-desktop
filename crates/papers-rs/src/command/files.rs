//! What a command does with the files of the entries it adds to the library
//! (`papers.files.FilesPolicy`): move them into the files directory, copy them
//! (a hard link when possible), or leave them where they are and link them.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use crate::config::paths::{abspath, dirname, is_inside, realpath, short_home};
use crate::config::{Config, MachineConfig, config_root, find_library_config};
use crate::error::{Error, Result};
use crate::fetch::download::is_download;
use crate::library::{Action, Policy};
use crate::log;

use super::session::{Say, SharedPrompt, no_input_error};

/// The files mode: `--move`, `--copy`, `--link`, else the `files` setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// ask for each file (the `files` setting `ask`)
    Ask,
    Move,
    Copy,
    Link,
}

impl Mode {
    pub fn parse(word: &str) -> Option<Mode> {
        Some(match word {
            "ask" => Mode::Ask,
            "move" => Mode::Move,
            "copy" => Mode::Copy,
            "link" => Mode::Link,
            _ => return None,
        })
    }

    pub fn word(self) -> &'static str {
        match self {
            Mode::Ask => "ask",
            Mode::Move => "move",
            Mode::Copy => "copy",
            Mode::Link => "link",
        }
    }
}

/// The files mode of a command (`files_mode`): the options (`--link` excludes
/// the others), else the `files` setting; a library without files directory
/// can only link files where they are.
pub fn files_mode(link: bool, move_: bool, copy: bool, config: &Config) -> Result<Mode> {
    if link && (move_ || copy) {
        return Err(Error::exit(
            "--link excludes --move, -r/--rename and --copy",
        ));
    }
    let mode = if copy {
        Mode::Copy
    } else if link {
        Mode::Link
    } else if move_ {
        Mode::Move
    } else {
        let setting = config.get("files");
        let mode = setting
            .as_str()
            .filter(|s| !s.is_empty())
            .and_then(Mode::parse)
            .unwrap_or(Mode::Move);
        if config.filesdir.is_none() {
            return Ok(Mode::Link);
        }
        mode
    };
    if config.filesdir.is_none() && mode != Mode::Link {
        return Err(Error::exit(format!(
            "--{}: the library has no files directory (pass --filesdir, or set it with `papers config filesdir DIR`)",
            mode.word()
        )));
    }
    Ok(mode)
}

/// Finds the library a file belongs to: a library whose configuration is found
/// in the file's directory or above, and whose files directory contains the
/// file (`OtherLibraries`).
struct OtherLibraries {
    current: Option<String>,
    machine: MachineConfig,
}

impl OtherLibraries {
    /// The directory of the configuration of the other library whose files directory holds `file`.
    fn owner(&self, file: &str) -> Option<String> {
        let mut start = dirname(&abspath(file));
        loop {
            let configfile = find_library_config(&start).ok().flatten()?;
            let libdir = config_root(&configfile);
            if self.current.as_deref() != Some(realpath(&libdir).as_str()) {
                let other = Config::load(&configfile, self.machine.clone(), None).ok();
                if let Some(other) = other
                    && other
                        .filesdir
                        .as_deref()
                        .is_some_and(|d| !d.is_empty() && is_inside(file, d))
                {
                    return Some(other.configdir());
                }
            }
            let parent = dirname(&libdir);
            if parent == libdir {
                return None;
            }
            start = parent;
        }
    }
}

struct State {
    said: HashSet<String>,
    for_all: Option<Action>,
}

/// What to do with each file placed in the files directory: the [`Policy`] of
/// the library, and the failure of a question that nobody answered, which the
/// policy cannot raise (`check` it after).
pub struct Placement {
    pub policy: Policy,
    failure: Arc<Mutex<Option<Error>>>,
}

impl Placement {
    /// An error of the policy: a question with no input.
    pub fn check(&self) -> Result<()> {
        match self.failure.lock().expect("the failure").take() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

/// What a files policy knows besides the mode (`FilesPolicy` of papers): the files that stay where
/// they are, those that are never moved, and the pools.
#[derive(Debug, Clone, Default)]
pub struct PolicyOptions {
    /// Files to leave where they are, whatever the mode (the files an entry had already).
    pub keep: Vec<String>,
    /// Files that are copied, never moved, even from the files directory (the files that `file fix` found).
    pub copied: Vec<String>,
    /// Files of pool entries (outside `pool_dirs` too): never moved.
    pub pool_files: Vec<String>,
    /// The files directories of pools: a file in one of them is never moved.
    pub pool_dirs: Vec<String>,
    /// `filesdir` is the files directory of the pool of a shallow library: the files already there
    /// are linked where they are, never renamed (other libraries may link to them).
    pub in_pool: bool,
}

/// The policy of a library for a files mode (`files_policy`): a file that papers
/// downloaded is always moved; a file that
/// belongs to another library is never moved (`move` copies it); a file
/// already in the files directory is moved (renamed), also in copy mode;
/// with `ask`, each other file is asked about.
pub fn files_policy(config: &Config, mode: Mode, say: Say, prompt: SharedPrompt) -> Placement {
    files_policy_with(config, mode, say, prompt, &PolicyOptions::default())
}

/// [`files_policy`] with the options of `FilesPolicy`: in order, a file that papers downloaded is
/// moved; one of `keep` is linked; in link mode every file is linked; one of `copied` is copied
/// (linked, in a shallow library, if it is in the files directory); one in the files directory is
/// moved (linked, in a shallow library); one of `pool_files` is copied (linked, in a shallow
/// library); one in a files directory of `pool_dirs` is copied; for the others the mode decides.
pub fn files_policy_with(
    config: &Config,
    mode: Mode,
    say: Say,
    prompt: SharedPrompt,
    options: &PolicyOptions,
) -> Placement {
    let filesdir = config.filesdir.clone();
    let others = OtherLibraries {
        current: config.file.as_deref().map(|f| realpath(&config_root(f))),
        machine: config.machine.clone(),
    };
    let state = Mutex::new(State {
        said: HashSet::new(),
        for_all: None,
    });
    let failure = Arc::new(Mutex::new(None));
    let keep: HashSet<String> = options.keep.iter().map(|f| realpath(f)).collect();
    let copied: HashSet<String> = options.copied.iter().map(|f| realpath(f)).collect();
    let pool_files: HashSet<String> = options.pool_files.iter().map(|f| realpath(f)).collect();
    let pool_dirs: Vec<String> = options.pool_dirs.iter().filter(|d| !d.is_empty()).cloned().collect();
    let in_pool = options.in_pool;
    let policy = {
        let failure = Arc::clone(&failure);
        Policy::by_file(move |file: &str| -> Action {
            // a file that papers downloaded is always moved into the files directory
            if is_download(file) {
                return Action::Move;
            }
            let real = realpath(file);
            if keep.contains(&real) || mode == Mode::Link {
                return Action::Link;
            }
            let inside = filesdir.as_deref().is_some_and(|d| is_inside(file, d));
            if copied.contains(&real) {
                return if in_pool && inside { Action::Link } else { Action::Copy };
            }
            if inside {
                return if in_pool { Action::Link } else { Action::Move };
            }
            if pool_files.contains(&real) {
                return if in_pool { Action::Link } else { Action::Copy };
            }
            if pool_dirs.iter().any(|d| is_inside(file, d)) {
                return Action::Copy;
            }
            let mut state = state.lock().expect("the policy");
            let action = match mode {
                Mode::Ask => ask(file, filesdir.as_deref(), &mut state, &prompt, &failure),
                Mode::Move => Action::Move,
                Mode::Copy => Action::Copy,
                Mode::Link => Action::Link,
            };
            if action == Action::Move
                && let Some(configdir) = others.owner(file)
            {
                if state.said.insert(file.to_string()) {
                    say.line(format!(
                        "{file} belongs to the library in {configdir}: copied, not moved"
                    ));
                }
                return Action::Copy;
            }
            action
        })
    };
    Placement { policy, failure }
}

/// Files mode `ask`: move, copy or link this file, as the user answers (M, C
/// or L: also for the next files; Enter: link). Without a terminal, the file
/// is linked where it is.
fn ask(
    file: &str,
    filesdir: Option<&str>,
    state: &mut State,
    prompt: &SharedPrompt,
    failure: &Arc<Mutex<Option<Error>>>,
) -> Action {
    if let Some(action) = state.for_all {
        return action;
    }
    let mut prompt = prompt.lock().expect("the prompt");
    if !prompt.interactive() {
        log::warning(&format!(
            "{file}: left where it is (files mode ask, no terminal to ask): --move, --copy or --link chooses, or `papers config files move|copy|link`"
        ));
        state.for_all = Some(Action::Link);
        return Action::Link;
    }
    let place = filesdir.map_or_else(|| "the files directory".to_string(), short_home);
    loop {
        let question = format!(
            "{}: [m]ove it to {place}, [c]opy it there, or [l]eave it where it is (Enter)? (M, C, L: the same for all the next files) ",
            short_home(file)
        );
        let Some(answer) = prompt.input(&question) else {
            *failure.lock().expect("the failure") = Some(no_input_error());
            return Action::Link;
        };
        let answer = answer.trim().to_string();
        if answer.is_empty() {
            return Action::Link;
        }
        let action = match answer.to_lowercase().as_str() {
            "m" => Action::Move,
            "c" => Action::Copy,
            "l" => Action::Link,
            _ => continue,
        };
        if answer.chars().count() == 1 {
            if answer.chars().all(char::is_uppercase) {
                state.for_all = Some(action);
            }
            return action;
        }
    }
}
