//! The write path: what every command that changes a library does around its
//! change, in the order papers does it (`papers.cli._run`, `papers.library.savebib`,
//! `papers.library.finish_journal`).
//!
//! ```text
//! let mut session = Session::open(config, options)?;   // the history before the command, the lock, the library
//! ... change session.library ...                        // the operations of this module: add, edit, rm, tag
//! session.commit()?;                                    // history head, checksums, atomic save, history step
//! session.delete_files(files)?;                         // (rm) after the save, kept for undo
//! let outcome = session.finish();                       // file operations not yet recorded; the lock is released
//! ```
//!
//! **Opening.** The library as it is is kept in memory for the first state of
//! its history (nothing is written: a command that changes nothing leaves no
//! trace), the lock of the library is taken (a dry run takes none), the bibtex
//! is loaded. A library with a pool (a shallow library) is not supported yet:
//! `open` fails with a clear error.
//!
//! **Committing** ([`Session::commit`]). A library that did not change is not
//! saved and has no new state. Otherwise: the lock is checked (a command that
//! lost it stops), the bibtex on disk that no state of the history has is
//! recorded as a state of its own (changed outside papers), the checksums of
//! the files that the command placed or linked go to `.papers/files.json`, the
//! bibtex is written in one step, and the step of the command (its message,
//! the bibtex, the files of `.papers/`, the file operations) is recorded. A
//! bibtex that another program wrote meanwhile is not overwritten: the files
//! that the command moved or copied are put back, and the command fails. A
//! failure of the history is reported (a line of the logger) and does not
//! undo the save.
//!
//! **Finishing** ([`Session::finish`], also after an error). The file
//! operations that the state of the command does not have yet join it (the
//! files deleted by `rm`), or, when the command did not save, make a state of
//! their own, so that undo brings the files back. The lock is released.
//!
//! **Output.** Nothing is printed here. What a command prints is given to a
//! [`Say`]: lines of the standard output, and the `--porcelain` lines. With a
//! sink (the command line) the lines go to it as they come, else they are
//! collected and returned by [`Session::finish`] (the app). Messages of the
//! logger (`papers: warning: ...`) go through [`crate::log`], as everywhere in
//! this crate. Questions are put to a [`Prompt`]; the default has no terminal.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use crate::bib::Entry;
use crate::config::Config;
use crate::config::paths::{abspath, basename, dirname, getcwd, isfile, normpath};
use crate::entry;
use crate::error::{Error, Result};
use crate::history::restore::{Deleter, file_digest, move_path, show};
use crate::history::{FileOp as HistoryOp, Snapshot};
use crate::history::{FilesJson, History, OpKind, Step};
use crate::library::{FileOp, Library, OpKind as LibraryOpKind, SaveError, SaveStatus};
use crate::lock::{self, Lock};
use crate::log;

/// Where the text of a command goes, as it comes.
pub type Sink = Box<dyn FnMut(&str) + Send>;

#[derive(Default)]
struct SayInner {
    sink: Option<Sink>,
    lines: Vec<String>,
    porcelain: Vec<String>,
}

/// What a command prints: lines of the standard output (`print()`; the
/// standard error with `--porcelain`), and the porcelain lines (the standard
/// output with `--porcelain`). Cheap to clone: the clones share the output.
#[derive(Clone, Default)]
pub struct Say(Arc<Mutex<SayInner>>);

impl Say {
    /// With a sink, the lines of the standard output go to it as they come;
    /// without, they are collected.
    pub fn new(sink: Option<Sink>) -> Say {
        Say(Arc::new(Mutex::new(SayInner {
            sink,
            ..SayInner::default()
        })))
    }

    /// A line of the standard output.
    pub fn line(&self, text: impl AsRef<str>) {
        let mut inner = self.0.lock().expect("the output");
        match &mut inner.sink {
            Some(sink) => sink(text.as_ref()),
            None => inner.lines.push(text.as_ref().to_string()),
        }
    }

    /// A porcelain line (`--porcelain`: for programs, on the standard output).
    pub fn porcelain(&self, text: impl AsRef<str>) {
        self.0
            .lock()
            .expect("the output")
            .porcelain
            .push(text.as_ref().to_string());
    }

    /// What was not sent to the sink: the lines, and the porcelain lines.
    pub fn take(&self) -> (Vec<String>, Vec<String>) {
        let mut inner = self.0.lock().expect("the output");
        (
            std::mem::take(&mut inner.lines),
            std::mem::take(&mut inner.porcelain),
        )
    }
}

/// Whoever answers the questions of a command: a terminal, a dialog of the app.
pub trait Prompt: Send {
    /// Papers can ask (`stdin_is_terminal`).
    fn interactive(&self) -> bool;

    /// `input(prompt)`: the answer, `None` when there is no more input.
    fn input(&mut self, prompt: &str) -> Option<String>;
}

/// No terminal: nothing can be asked.
pub struct NoPrompt;

impl Prompt for NoPrompt {
    fn interactive(&self) -> bool {
        false
    }

    fn input(&mut self, _prompt: &str) -> Option<String> {
        None
    }
}

pub type SharedPrompt = Arc<Mutex<Box<dyn Prompt>>>;

/// The error of a question that has no one to answer it (`EOFError` in papers).
pub fn no_input_error() -> Error {
    Error::exit(
        "papers asked a question, but there is no input to answer it: run the command in a terminal, or answer with its options (e.g. --on-duplicate for duplicates, -y/--yes)",
    )
}

/// The options of a command that writes a library.
pub struct Options {
    /// The command as the history records it: `papers add refs.bib`, `papers`
    /// and the arguments joined by spaces (`-C DIR` included).
    pub message: String,
    /// `--dry-run`: nothing is written, moved or deleted.
    pub dry_run: bool,
    /// `-y/--yes`: the questions of the guard before deleting files are answered yes.
    pub assume_yes: bool,
    /// The lines have colors and links (`--color`, a terminal).
    pub color: bool,
    pub prompt: Box<dyn Prompt>,
    /// Where the lines of the standard output go (the command line); none: collected.
    pub sink: Option<Sink>,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            message: "papers".to_string(),
            dry_run: false,
            assume_yes: false,
            color: false,
            prompt: Box::new(NoPrompt),
            sink: None,
        }
    }
}

/// What a finished command leaves for whoever shows it.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// The lines of the standard output not sent to a sink.
    pub out: Vec<String>,
    /// The `--porcelain` lines.
    pub porcelain: Vec<String>,
}

/// What [`Session::commit`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Committed {
    /// The bibtex is as the command made it (written, or already so); false for a library that did not change, and a dry run.
    pub saved: bool,
    /// The state of the library after the command in its history (none if nothing was recorded).
    pub step: Option<u64>,
}

/// A command that writes a library, from the lock to the history.
pub struct Session {
    pub config: Config,
    pub library: Library,
    pub say: Say,
    options: Options,
    prompt: SharedPrompt,
    lock: Option<Lock>,
    before: Option<Snapshot>,
    history: Option<History>,
    /// The step of the history that has the command's save: `Some(number)` after `record`.
    recorded: Option<Option<u64>>,
    /// The library's file operations were put back after a refused save.
    rolled_back: bool,
    /// The operations done after the save (files deleted), oldest first.
    ops_after: Vec<HistoryOp>,
}

/// The error of what is not ported yet.
pub fn not_implemented(what: &str) -> Error {
    Error::exit(format!("{what}: not implemented in papers-rs yet"))
}

impl Session {
    /// Start a command on the library of `config` (its bibtex must exist).
    pub fn open(mut config: Config, mut options: Options) -> Result<Session> {
        if config.shallow() {
            return Err(not_implemented(
                "a library with a pool (a shallow library; `papers config pool none` makes it self-contained)",
            ));
        }
        let before = if options.dry_run {
            None
        } else {
            History::before_command(&mut config)
        };
        let lock = if options.dry_run {
            None
        } else {
            lock::acquire(&config, &options.message, None)?
        };
        let mut library = Library::open(&config)?;
        library.dry_run = options.dry_run;
        let prompt: SharedPrompt = Arc::new(Mutex::new(std::mem::replace(
            &mut options.prompt,
            Box::new(NoPrompt),
        )));
        let say = Say::new(options.sink.take());
        {
            let (prompt, say) = (Arc::clone(&prompt), say.clone());
            library.confirm_replace = Box::new(move |dst: &str| {
                let _ = &say;
                let mut prompt = prompt.lock().expect("the prompt");
                match prompt.input(&format!("dest file already exists: {dst}. Replace? (y/n) ")) {
                    Some(answer) => answer.to_lowercase() == "y",
                    None => false,
                }
            });
        }
        Ok(Session {
            config,
            library,
            say,
            options,
            prompt,
            lock,
            before,
            history: None,
            recorded: None,
            rolled_back: false,
            ops_after: Vec::new(),
        })
    }

    pub fn dry_run(&self) -> bool {
        self.options.dry_run
    }

    pub fn color(&self) -> bool {
        self.options.color
    }

    pub fn message(&self) -> &str {
        &self.options.message
    }

    /// The prompt of the command, to share with whoever asks questions.
    pub fn prompt(&self) -> SharedPrompt {
        Arc::clone(&self.prompt)
    }

    pub fn interactive(&self) -> bool {
        self.prompt.lock().expect("the prompt").interactive()
    }

    /// `input(prompt)`: the answer, an error when there is no more input.
    pub fn input(&self, prompt: &str) -> Result<String> {
        self.prompt
            .lock()
            .expect("the prompt")
            .input(prompt)
            .ok_or_else(no_input_error)
    }

    /// Whether the files that a command deletes are kept in the history (the `keep-deleted-files` setting).
    pub fn keeps_deleted(&self) -> bool {
        !matches!(
            self.config.get("keep-deleted-files"),
            serde_json::Value::Bool(false)
        )
    }

    fn history(&mut self) -> Result<&mut History> {
        if self.history.is_none() {
            let opened = History::open(&mut self.config, true, self.options.dry_run)?;
            self.history = Some(opened.ok_or_else(|| Error::other("the library has no history"))?);
        }
        Ok(self.history.as_mut().expect("the history"))
    }

    // saving
    // ------

    /// Save the library, and record the step of the command in its history
    /// (see the module). With `--dry-run`, only the line that says so.
    pub fn commit(&mut self) -> Result<Committed> {
        let bibtex = self.library.bibtex.clone();
        if self.options.dry_run {
            self.say.line(format!("DRYRUN: {bibtex} not saved"));
            return Ok(Committed {
                saved: false,
                step: None,
            });
        }
        if self.library.unchanged() {
            // logs that nothing changed; nothing is written, and the history has no new state
            self.library.save(false).map_err(Error::from)?;
            return Ok(Committed {
                saved: false,
                step: None,
            });
        }
        if let Some(lock) = &self.lock {
            lock.check()?;
        }
        // the bibtex as it was before the command, if papers did not write it
        let before = self.before.take();
        match self.history().and_then(|h| h.sync_head(before.as_ref())) {
            Ok(_) => {}
            Err(error) => log::error(&format!(
                "the history of the library could not record its state before the command: {error}"
            )),
        }
        self.update_files_json()?;
        let saved = match self.library.save(false) {
            Ok(saved) => saved,
            Err(SaveError::BibtexChanged(message)) => {
                let lines = rollback(self.library.file_ops());
                self.rolled_back = true;
                if !lines.is_empty() {
                    self.say.line("the files of the command are put back:");
                    self.say.line(lines.join("\n"));
                }
                return Err(Error::exit(message));
            }
            Err(SaveError::Error(error)) => return Err(error),
        };
        let ops: Vec<HistoryOp> = saved.file_ops.iter().map(history_op).collect();
        let message = self.options.message.clone();
        // the history comes last: a failure must not undo the work done above
        let step = match self.history().and_then(|h| {
            h.record(Step {
                message,
                ops,
                ..Step::default()
            })
        }) {
            Ok(recorded) => {
                self.recorded = Some(recorded.number);
                recorded.number
            }
            Err(error) => {
                log::error(&format!(
                    "the bibliography was saved, but its history failed: {error}"
                ));
                None
            }
        };
        Ok(Committed {
            saved: saved.status != SaveStatus::Unchanged,
            step,
        })
    }

    /// `.papers/files.json` before the bibtex is written: the checksums of the
    /// files that the command moved, copied, added or linked; the files that
    /// no entry links to any more are dropped (`papers.checksums.update`).
    fn update_files_json(&self) -> Result<()> {
        let Some(libdir) = self.config.bibtex.as_deref().map(|b| dirname(&abspath(b))) else {
            return Ok(());
        };
        let Some(files_json) = FilesJson::of(&self.config, &libdir) else {
            return Ok(());
        };
        let relative_to = self.library.relative_to.clone();
        let links_of =
            |entries: &mut dyn Iterator<Item = &Entry>, relative_to: &str| -> BTreeSet<String> {
                entries
                    .filter(|e| e.get("file").is_some())
                    .flat_map(|e| entry::files(e, Some(relative_to)))
                    .map(|p| normpath(&p))
                    .collect()
            };
        let links = links_of(&mut self.library.entries(), &relative_to);
        let mut touched: BTreeSet<String> = self
            .library
            .file_ops()
            .iter()
            .map(|op| normpath(&op.dst))
            .collect();
        if let Some(loaded) = self.library.loaded_entries() {
            let loaded_relative_to = self
                .library
                .loaded_relative_to()
                .unwrap_or(&relative_to)
                .to_string();
            let before = links_of(&mut loaded.into_iter(), &loaded_relative_to);
            touched.extend(links.difference(&before).cloned());
        }
        let checksums = !matches!(self.config.get("checksums"), serde_json::Value::Bool(false));
        files_json.update(&links, &touched, checksums)?;
        Ok(())
    }

    // deleting files
    // --------------

    /// Delete files after the save of the library (`papers.files.delete_files`):
    /// they are kept in the history for undo (the `keep-deleted-files`
    /// setting), unless the history cannot hold them: papers asks first, and
    /// without a terminal the files are kept where they are. `same`: for a
    /// file, an identical file that stays (undo copies it back from there: the
    /// file itself is not kept).
    pub fn delete_files(&mut self, files: &[String], same: &[(String, String)]) -> Result<()> {
        let dry = self.options.dry_run;
        let files: Vec<&String> = files.iter().filter(|f| isfile(f)).collect();
        let same: Vec<(&str, &str)> = same
            .iter()
            .filter(|(f, copy)| {
                files.iter().any(|x| x.as_str() == f.as_str())
                    && isfile(copy)
                    && file_digest(copy) == file_digest(f)
            })
            .map(|(f, c)| (f.as_str(), c.as_str()))
            .collect();
        let is_same = |f: &str| same.iter().any(|(s, _)| *s == f);
        let sizes: u64 = files
            .iter()
            .filter(|f| !is_same(f))
            .map(|f| std::fs::metadata(f.as_str()).map(|m| m.len()).unwrap_or(0))
            .sum();
        let keeps_deleted = self.keeps_deleted();
        let count = files.len() - same.len();
        let keep = if sizes > 0 && !dry {
            // the question before deleting more than the history keeps (`papers.backup._ask`)
            let (say, prompt, assume_yes) =
                (self.say.clone(), self.prompt(), self.options.assume_yes);
            let mut ask = move |question: &str| -> bool {
                if assume_yes {
                    return true;
                }
                say.line(question);
                let mut prompt = prompt.lock().expect("the prompt");
                if !prompt.interactive() {
                    log::warning("no terminal to confirm: the files are kept (-y/--yes proceeds)");
                    return false;
                }
                matches!(
                    prompt
                        .input("Proceed? [y/N] ")
                        .map(|a| a.trim().to_lowercase())
                        .as_deref(),
                    Some("y" | "yes")
                )
            };
            self.history()?.guard(sizes, count, &mut ask)
        } else {
            Some(true)
        };
        let Some(keep) = keep else {
            for file in &files {
                self.say.line(format!("kept {file}"));
            }
            return Ok(());
        };
        let mut record = Some(std::mem::take(&mut self.ops_after));
        let mut result = Ok(());
        for file in &files {
            let mut note = String::new();
            if is_hard_link(file) {
                note.push_str(" (hard link: the other links remain)");
            }
            if keep && !is_same(file) && keeps_deleted {
                note.push_str(" (kept in the history: undo restores it)");
            }
            self.say.line(format!(
                "{}delete {file}{note}",
                if dry { "DRYRUN: " } else { "" }
            ));
            if dry {
                continue;
            }
            let trash = |path: &str| (crate::history::store::to_system_trash)(path);
            let step = if let Some((_, copy)) = same.iter().find(|(s, _)| *s == file.as_str()) {
                // recorded with its copy: not kept
                let digest = file_digest(file);
                if let Some(ops) = record.as_mut() {
                    ops.push(HistoryOp::new(OpKind::Removed, file, Some(copy), digest));
                }
                std::fs::remove_file(file.as_str())
                    .map_err(|e| Error::other(format!("{file}: {e}")))
            } else {
                let history = self.history()?;
                let deleter = Deleter {
                    keep: keep && keeps_deleted,
                    store: Some(&history.store),
                    trash: &trash,
                };
                deleter.remove(file, &mut record)
            };
            if let Err(error) = step {
                result = Err(error);
                break;
            }
        }
        self.ops_after = record.unwrap_or_default();
        result
    }

    // the end
    // -------

    /// The end of the command, also after an error: the file operations that
    /// the command's state does not have yet are recorded (see the module),
    /// the lock is released, and what was not sent to a sink is returned.
    pub fn finish(mut self) -> Outcome {
        let mut ops: Vec<HistoryOp> = Vec::new();
        if self.recorded.is_none() && !self.rolled_back {
            ops.extend(self.library.file_ops().iter().map(history_op));
        }
        ops.extend(std::mem::take(&mut self.ops_after));
        if !ops.is_empty() && !self.options.dry_run {
            let count = ops.len();
            let result = match self.recorded {
                Some(Some(n)) => self.history().and_then(|h| h.add_operations(n, &ops)),
                Some(None) => Ok(()),
                None => {
                    let message = self.options.message.clone();
                    let before = self.before.take();
                    self.history().and_then(|h| {
                        h.sync_head(before.as_ref())?;
                        h.record(Step {
                            message,
                            ops,
                            ..Step::default()
                        })
                        .map(|_| ())
                    })
                }
            };
            if let Err(error) = result {
                log::error(&format!(
                    "{count} file operation(s) not recorded for undo: {error}"
                ));
            }
        }
        drop(self.lock.take());
        let (out, porcelain) = self.say.take();
        Outcome { out, porcelain }
    }
}

fn history_op(op: &FileOp) -> HistoryOp {
    let kind = match op.op {
        LibraryOpKind::Moved => OpKind::Moved,
        LibraryOpKind::Copied => OpKind::Copied,
    };
    HistoryOp::new(kind, &op.src, Some(&op.dst), op.checksum.clone())
}

fn is_hard_link(file: &str) -> bool {
    use std::os::unix::fs::MetadataExt;
    isfile(file)
        && std::fs::metadata(file)
            .map(|m| m.nlink() > 1)
            .unwrap_or(false)
}

/// Put back the files of the operations of a command whose save was refused,
/// the last one first (`papers.journal.rollback`). The safety rules of
/// `restore` hold: no file is overwritten, a file changed since stays. The
/// lines of the report.
fn rollback(ops: &[FileOp]) -> Vec<String> {
    let cwd = getcwd();
    let lexists = |p: &str| std::fs::symlink_metadata(p).is_ok();
    let mut lines = Vec::new();
    for op in ops.iter().rev() {
        let (src, dst) = (&op.src, &op.dst);
        if !lexists(dst) {
            continue;
        }
        if op.checksum.is_some() && isfile(dst) && file_digest(dst) != op.checksum {
            lines.push(format!(
                "    {}: changed since: left as it is",
                show(dst, &cwd)
            ));
        } else if op.op == LibraryOpKind::Moved && !lexists(src) {
            let parent = dirname(src);
            if !parent.is_empty() {
                let _ = std::fs::create_dir_all(&parent);
            }
            match move_path(dst, src) {
                Ok(()) => lines.push(arrow(dst, src, "moved back to", &cwd)),
                Err(error) => {
                    lines.push(format!("    {}: not moved back ({error})", show(dst, &cwd)))
                }
            }
        } else if op.op == LibraryOpKind::Copied && isfile(src) {
            let _ = std::fs::remove_file(dst);
            lines.push(format!(
                "    {}: removed (a copy of {})",
                show(dst, &cwd),
                show(src, &cwd)
            ));
        } else {
            lines.push(format!(
                "    {}: not moved back, {} exists",
                show(dst, &cwd),
                show(src, &cwd)
            ));
        }
    }
    lines
}

fn arrow(src: &str, dst: &str, verb: &str, cwd: &str) -> String {
    if dirname(src) == dirname(dst) && verb.starts_with("moved") {
        format!(
            "    {} --> {} {}",
            show(src, cwd),
            verb.replacen("moved", "renamed", 1),
            basename(dst)
        )
    } else {
        format!("    {} --> {verb} {}", show(src, cwd), show(dst, cwd))
    }
}
