//! papers-rs, built in, as the writer of the papers libraries: the commands
//! of papers run in the process, with no subprocess and no Python. It shares
//! the lock, the history and the files with any other papers (a command, a
//! terminal), so that either can undo a step the other wrote.

use std::path::Path;

use papers_rs::command::add::{AddRequest, add};
use papers_rs::command::edit::replace_from_text;
use papers_rs::command::files::{Mode, files_mode};
use papers_rs::command::pool::push_after_travel;
use papers_rs::command::report::Changes;
use papers_rs::command::rm::{Files, remove_entries};
use papers_rs::command::tag::{add_remove, rename};
use papers_rs::command::{Options, Session};
use papers_rs::config::paths::exists;
use papers_rs::config::{Config, MachineConfig, find_config};
use papers_rs::error::Error;
use papers_rs::formats::Format;
use papers_rs::history::History;
use papers_rs::lock;
use papers_rs::names::{Format as KeyFormat, FormatKind};
use papers_rs::merge::{OnDuplicate as Merging, Resolver};

use super::{AddOptions, FilesMode, Kind, OnDuplicate, PapersWriter, Pasted, WriteError};

impl From<Error> for WriteError {
    fn from(e: Error) -> WriteError {
        WriteError::failed(e.message())
    }
}

/// papers-rs, in the process.
#[derive(Debug, Default)]
pub struct Builtin {
    /// the global configuration of papers to use instead of the user's: the
    /// tests give the history a folder of its own
    machine: Option<MachineConfig>,
}

impl Builtin {
    pub fn new(machine: Option<MachineConfig>) -> Builtin {
        Builtin { machine }
    }

    /// The configuration of the library of a folder.
    fn config(&self, folder: &Path) -> Result<Config, WriteError> {
        let machine = match &self.machine {
            Some(machine) => machine.clone(),
            None => MachineConfig::load(None)?,
        };
        let config = find_config(&folder.to_string_lossy(), machine)?;
        let Some(bibtex) = config.bibtex.as_deref().filter(|_| config.is_library) else {
            return Err(WriteError::failed(format!(
                "{}: not a papers library",
                folder.display()
            )));
        };
        if !exists(bibtex) {
            return Err(WriteError::failed(format!("{bibtex}: not found")));
        }
        Ok(config)
    }

    /// A command of papers on the library of a folder: `change` makes it on
    /// the session, which is then saved and recorded in the history, and
    /// the lock released whatever the result. `change` returns the keys of
    /// the entries it considered: what changed of them (and what was added,
    /// removed, renamed) is the result. `command` and `args` are what the
    /// command line would be, for the history.
    fn transact(
        &self,
        folder: &Path,
        (command, args): (&str, &[String]),
        change: impl FnOnce(&mut Session) -> Result<Vec<String>, Error>,
    ) -> Result<Changes, WriteError> {
        let config = self.config(folder)?;
        let options = Options {
            message: command_line(folder, command, args),
            ..Options::default()
        };
        let mut session = Session::open(config, options)?;
        let done = change(&mut session).and_then(|touched| {
            session.commit()?;
            Ok(Changes::of(&session.library, &touched).unwrap_or_default())
        });
        session.finish();
        Ok(done?)
    }
}

/// The files mode of an addition: the one chosen, else the `files` setting of the library.
fn mode(chosen: Option<FilesMode>, config: &Config) -> Result<Mode, Error> {
    files_mode(
        chosen == Some(FilesMode::Link),
        chosen == Some(FilesMode::Move),
        chosen == Some(FilesMode::Copy),
        config,
    )
}

impl Builtin {
    /// The key template of the library of a folder (`--update-key` makes a new entry's key by it).
    pub fn key_format(&self, folder: &Path) -> Result<KeyFormat, WriteError> {
        Ok(KeyFormat::from_config(&self.config(folder)?, FormatKind::Key))
    }

    /// What a file added to the library of a folder is done with when the user does not say:
    /// its `files` setting (`ask`, which needs a terminal, as move), else move; link when the
    /// library has no files directory.
    pub fn files_mode(&self, folder: &Path) -> Result<FilesMode, WriteError> {
        Ok(match mode(None, &self.config(folder)?)? {
            Mode::Ask | Mode::Move => FilesMode::Move,
            Mode::Copy => FilesMode::Copy,
            Mode::Link => FilesMode::Link,
        })
    }
}

/// The command line of papers that the history records for a command.
fn command_line(folder: &Path, command: &str, args: &[String]) -> String {
    let words = [command, "--porcelain"]
        .into_iter()
        .chain(args.iter().map(String::as_str));
    format!(
        "papers -C {} {}",
        folder.display(),
        words.collect::<Vec<_>>().join(" ")
    )
}

impl PapersWriter for Builtin {
    fn kind(&self) -> Kind {
        Kind::PapersRs
    }

    fn version(&self) -> Option<String> {
        None
    }

    fn refuses(&self, folder: &Path) -> Option<String> {
        self.config(folder).err().map(|e| e.message)
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
        self.transact(folder, ("tag", &args), |session| {
            let keys = session.library.select(keys)?;
            add_remove(&mut session.library, &keys, add, remove);
            Ok(keys)
        })
    }

    fn rename_tag(&self, folder: &Path, old: &str, new: &str) -> Result<Changes, WriteError> {
        let args = ["--rename", old, new].map(String::from);
        self.transact(folder, ("tag", &args), |session| {
            rename(&mut session.library, old, new);
            Ok(Vec::new())
        })
    }

    fn edit(&self, folder: &Path, key: &str, text: &str) -> Result<Changes, WriteError> {
        let args = [key, "--from", "-"].map(String::from);
        self.transact(folder, ("edit", &args), |session| {
            let keys = session.library.select(&[key.to_string()])?;
            replace_from_text(
                &mut session.library,
                &keys[0],
                text,
                Format::Bibtex,
                "--from -",
            )?;
            Ok(session.library.entries().map(|e| e.key.clone()).collect())
        })
    }

    fn add(&self, folder: &Path, text: &str, options: &AddOptions) -> Result<Changes, WriteError> {
        let pasted = Pasted::new(text)?;
        let file = pasted.path().to_string_lossy().into_owned();
        let mut args = vec![
            file.clone(),
            "--on-duplicate".into(),
            options.on_duplicate.arg().into(),
        ];
        if options.update_key {
            args.push("--update-key".into());
        }
        let pdf = options
            .pdf
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned());
        if let Some(pdf) = &pdf {
            args.extend(["--attach".to_string(), pdf.clone()]);
            args.extend(options.files.map(|m| m.flag().to_string()));
        }
        self.transact(folder, ("add", &args), |session| {
            let request = AddRequest {
                files: vec![file],
                update_key: options.update_key,
                attachments: pdf.into_iter().collect(),
                mode: mode(options.files, &session.config)?,
                ..AddRequest::default()
            };
            let mut resolver = Resolver::new(match options.on_duplicate {
                OnDuplicate::Skip => Merging::Skip,
                OnDuplicate::Keep => Merging::Keep,
                OnDuplicate::Update => Merging::Update,
            });
            add(session, &request, &mut resolver)
        })
    }

    fn attach(
        &self,
        folder: &Path,
        key: &str,
        pdf: &Path,
        files: Option<FilesMode>,
    ) -> Result<Changes, WriteError> {
        let pdf = pdf.to_string_lossy().into_owned();
        let mut args = vec![
            "--key".to_string(),
            key.to_string(),
            "--attach".into(),
            pdf.clone(),
        ];
        args.extend(files.map(|m| m.flag().to_string()));
        self.transact(folder, ("add", &args), |session| {
            let request = AddRequest {
                key: Some(key.to_string()),
                attachments: vec![pdf],
                mode: mode(files, &session.config)?,
                ..AddRequest::default()
            };
            add(session, &request, &mut Resolver::new(Merging::Skip))
        })
    }

    fn remove(&self, folder: &Path, keys: &[String]) -> Result<Changes, WriteError> {
        let mut args = keys.to_vec();
        args.push("--keep-files".to_string());
        self.transact(folder, ("rm", &args), |session| {
            let keys = session.library.select(keys)?;
            remove_entries(session, &keys, Files::Keep, false)?;
            Ok(keys)
        })
    }

    fn step(&self, folder: &Path, back: bool) -> Result<(), WriteError> {
        let mut config = self.config(folder)?;
        let command = command_line(folder, if back { "undo" } else { "redo" }, &[]);
        let lock = lock::acquire(&config, &command, None)?;
        let mut history = History::require(&mut config, false)?;
        // a pool's step reverted with conflicts: skipped, as papers does
        // without a terminal (the app asks nothing)
        let mut skip = Resolver::new(Merging::Skip);
        let moved = if back {
            history.undo(1, false, lock.as_ref(), &command, &mut skip)?
        } else {
            history.redo(1, false, lock.as_ref(), &command, &mut skip)?
        };
        // a shallow library with auto-push: what differs from its pool is pushed, as papers does
        let options = Options {
            message: command,
            ..Options::default()
        };
        push_after_travel(config, options, lock, moved.links)?;
        Ok(())
    }
}
