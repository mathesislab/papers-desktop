//! The history of the libraries: undo, redo, restore, in the store that
//! papers (Python), papers-rs and the app share (`spec/history.md` of
//! Mahé Perrette's specification; `papers.history`, `papers.backup`,
//! `papers.journal`, `papers.checksums` are the reference).
//!
//! The store is a folder of the machine (the `history-dir` setting):
//!
//! ```text
//! objects/ab/ab12…          a piece of text (zlib), named by the sha256 of its bytes
//! file-objects/ab/ab12….pdf a file deleted by a command, named by its sha256
//! projects/<id>/            the history of a library: states/NNNNNN.json, heads.json
//! separate/<id>/            a store of its own (history-separate), same layout
//! ```
//!
//! A **project** is the history of a library (named by its `id`), a
//! **place** is where a copy of it is (the real path of its bibtex), a
//! **state** is a snapshot (the bibtex, the kept `.papers/` files, the file
//! operations of the command), a **head** is the current state of a place with
//! its redo list.
//!
//! The modules:
//!
//! - [`store`]: the objects (pieces of text, cut as papers cuts them), the file
//!   objects, the size and the retention;
//! - [`project`]: the states (`State`, with the keys it does not know kept) and
//!   the heads;
//! - [`ops`], [`restore`]: the file operations of a command, and bringing the
//!   files from one state to another;
//! - [`files_json`]: `.papers/files.json`, the checksums of the linked files;
//! - [`setup`]: the settings of a library for the history (`Setup`), its project
//!   and place, the texts a state keeps (`Snapshot`);
//! - [`recorder`]: `History`, the history of one library: recording a step
//!   (`Step`, the hook of the library model), the changes made outside papers,
//!   the operations after the save, the guard before deleting files;
//! - [`travel`]: undo, redo, restore (`History::undo`, `redo`, `restore`),
//!   with the steps of a pool;
//! - [`report`]: what undo, redo and restore print;
//! - [`listing`]: the lines of `papers history`, the removal of a history.
//!
//! How a command uses it (the library model calls these):
//!
//! ```text
//! let before = History::before_command(&mut config);      // at the start (nothing written)
//! ...
//! let mut h = History::open(&mut config, true, dry_run)?.unwrap();
//! h.sync_head(before.as_ref())?;                            // before the save: the head, outside changes
//! // update .papers/files.json (FilesJson::update), write the bibtex
//! let rec = h.record(Step { message: "papers add x".into(), ops, ..Step::default() })?;
//! // operations after the save (rm --delete-files):
//! h.add_operations(rec.number.unwrap(), &later_ops)?;
//! ```
//!
//! A command that also writes another library (its pool) records that one first,
//! with `Step { from: Some(dir of the command's bibtex), .. }` and no operations;
//! `Recorded::link` goes into `Step::pools` of the command's own step.
//!
//! Where the history stops and the library model starts: the file operations
//! (`FileOp::new`, with `Deleter` for what a command deletes), the guard's
//! question (`History::guard` takes the `ask` function), the lock (the caller
//! holds it, and passes it to `undo`, `redo`, `restore`), the put back of the
//! files when a save is refused (`journal.rollback`), and the commands' output
//! (`report`, `listing`).
//!
//! The pool steps follow undo and redo as in papers 3.0.0b5 (`travel`); a pool
//! that moved on since is *reverted* there: a new step of the pool that takes
//! back the step of the command, by the three-way merge of
//! [`crate::command::revert`] (as `papers history --revert N` does for a
//! library).
//!
//! Not here: the commands (`papers undo`, `history --revert`, in `command` and the program).

pub mod files_json;
pub mod listing;
pub mod ojson;
pub mod ops;
pub mod project;
pub mod recorder;
pub mod report;
pub mod restore;
pub mod setup;
pub mod store;
pub mod travel;

pub use files_json::FilesJson;
pub use ops::{FileOp, OpKind};
pub use project::{Head, PoolLink, Project, State};
pub use recorder::{History, Recorded, Step};
pub use setup::{Setup, Snapshot};
pub use store::Store;
pub use travel::Moved;
