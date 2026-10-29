//! The actions of `papers list` on the files of the entries listed: `--attach`, `--detach`,
//! `--detach-broken`, `--rename` (`--keep-source`), `--fix-files`, and `-o/--open`. They are the
//! commands of `papers file` (`cmd_file`) on the selection of `list`.

use papers_rs::command::file_cmd::{
    Attach, CheckOptions, attach_files, check_entries_files, detach_entries, rename_entries,
};
use papers_rs::command::files::{Mode, files_mode};
use papers_rs::command::queries::Queries;
use papers_rs::command::report::{Notes, print_changes};
use papers_rs::config::Config;
use papers_rs::config::paths::{basename, isfile, join};
use papers_rs::error::Error;
use papers_rs::library::Library;
use papers_rs::log;
use papers_rs::search::Search;

use crate::cmd_add::open_with_system;
use crate::cmd_config::Failure;
use crate::cmd_file::open;
use crate::parse::Leaf;
use crate::util::file_uri_to_path;

/// What `list` does to the files of the entries it lists.
pub enum FileAction {
    /// `--attach FILE...` (or the deprecated `--add-files`, with its files mode).
    Attach {
        files: Vec<String>,
        mode: Option<Mode>,
    },
    /// `--detach [FILE...]`, `--detach-broken`.
    Detach { files: Vec<String>, broken: bool },
    /// `--rename`, `--keep-source`, `--move`.
    Rename { keep_source: bool, move_: bool },
    /// `--fix-files [DIR...]`, `--move`.
    FixFiles { search: Vec<String>, move_: bool },
    /// `-o/--open`.
    Open,
}

fn given(leaf: &Leaf, id: &str) -> bool {
    leaf.matches.try_contains_id(id).unwrap_or(false)
}

impl FileAction {
    /// The file action that the options of `list` ask for, if any (the deprecated forms
    /// `--add-files` and `--copy` as `list` reads them; the options have been checked).
    pub fn from_leaf(leaf: &Leaf) -> Option<FileAction> {
        let add_files = leaf.strings("add_files");
        let attach: Vec<String> = leaf
            .strings("attach")
            .into_iter()
            .chain(add_files.iter().cloned())
            .map(|f| file_uri_to_path(&f))
            .collect();
        if !attach.is_empty() {
            // --add-files: the files mode of --rename and --copy; --attach: the files setting
            let mode = (!add_files.is_empty()).then(|| {
                if leaf.flag("copy") {
                    Mode::Copy
                } else if leaf.flag("rename") {
                    Mode::Move
                } else {
                    Mode::Link
                }
            });
            return Some(FileAction::Attach {
                files: attach,
                mode,
            });
        }
        if given(leaf, "detach") || leaf.flag("detach_broken") {
            return Some(FileAction::Detach {
                files: leaf
                    .strings("detach")
                    .iter()
                    .map(|f| file_uri_to_path(f))
                    .collect(),
                broken: leaf.flag("detach_broken"),
            });
        }
        if given(leaf, "fix_files") {
            return Some(FileAction::FixFiles {
                search: leaf.strings("fix_files"),
                move_: leaf.flag("move"),
            });
        }
        if leaf.flag("rename") || leaf.flag("copy") {
            return Some(FileAction::Rename {
                keep_source: leaf.flag("keep_source") || leaf.flag("copy"),
                move_: leaf.flag("move"),
            });
        }
        leaf.flag("open").then_some(FileAction::Open)
    }
}

/// The keys of the entries of `library` that the search selects.
fn selected(library: &Library, search: &Search) -> Result<Vec<String>, Failure> {
    let all: Vec<&papers_rs::bib::Entry> = library.entries().collect();
    let entries = search
        .select(&all, Some(&library.distinct), &library.relative_to)
        .map_err(Failure::Usage)?;
    Ok(entries.iter().map(|e| e.key.clone()).collect())
}

/// Do the action on the entries that `search` selects.
pub fn run(
    leaf: &Leaf,
    config: Config,
    search: &Search,
    action: FileAction,
) -> Result<(), Failure> {
    if let FileAction::Open = action {
        let library = papers_rs::command::pool::load_library(&config)?;
        let keys = selected(&library, search)?;
        open_entries(&library, &keys);
        return Ok(());
    }
    let mode = match &action {
        FileAction::Attach {
            mode: Some(mode), ..
        } => Some(*mode),
        FileAction::Attach { mode: None, .. } => Some(files_mode(false, false, false, &config)?),
        _ => None,
    };
    let mut session = open(leaf, config, false, false)?;
    let result = (|| -> Result<(), Failure> {
        // in a shallow library the files that `attach`, `rename` and `fix-files` place go to the pool
        if matches!(
            action,
            FileAction::Attach { .. } | FileAction::Rename { .. } | FileAction::FixFiles { .. }
        ) && !(matches!(action, FileAction::FixFiles { .. }) && session.dry_run())
        {
            session.use_pool_files()?;
        }
        let keys = selected(&session.library, search)?;
        match action {
            FileAction::Attach { files, .. } => {
                if keys.len() != 1 {
                    return Err(Error::exit(format!(
                        "list only one entry to use --attach ({} listed): nothing was changed",
                        keys.len()
                    ))
                    .into());
                }
                let attach = Attach::new(mode.unwrap_or(Mode::Move));
                let placed = attach_files(&mut session, &keys[0], &files, &attach)?;
                let committed = session.commit()?;
                report(&session, leaf, &keys, &committed.notes);
                crate::cmd_file::refused(placed)?;
            }
            FileAction::Detach { files, broken } => {
                detach_entries(&mut session, Some(&keys), &files, broken, false)?;
            }
            FileAction::FixFiles { search, move_ } => {
                let options = CheckOptions {
                    pools: leaf.strings("pool"),
                    search,
                    move_,
                    ..CheckOptions::default()
                };
                check_entries_files(&mut session, &options, true, &keys, &Queries::default())?;
            }
            FileAction::Rename { keep_source, move_ } => {
                let failed = rename_entries(
                    &mut session,
                    &keys,
                    keep_source,
                    move_.then_some(Mode::Move),
                )?;
                let committed = session.commit()?;
                report(&session, leaf, &keys, &committed.notes);
                crate::cmd_file::refused(failed == 0)?;
            }
            FileAction::Open => {}
        }
        Ok(())
    })();
    crate::cmd_write::finish(session);
    result
}

fn report(session: &papers_rs::command::Session, leaf: &Leaf, keys: &[String], notes: &Notes) {
    print_changes(
        &session.library,
        keys,
        false,
        notes,
        leaf.flag("porcelain"),
        session.color(),
        &session.say,
    );
}

/// `os.path.commonpath` of absolute paths.
fn commonpath(paths: &[String]) -> String {
    let split: Vec<Vec<&str>> = paths
        .iter()
        .map(|p| p.split('/').filter(|c| !c.is_empty()).collect())
        .collect();
    let mut common: Vec<&str> = split.first().cloned().unwrap_or_default();
    for other in &split[1..] {
        let n = common.iter().zip(other).take_while(|(a, b)| a == b).count();
        common.truncate(n);
    }
    format!("/{}", common.join("/"))
}

/// Open the files of the entries with the application of the system (`open_entries`,
/// `view_entry_files`): an entry without file is a warning; the files of an entry of the layout
/// before papers 2.19 (`<name>/` with a hidden `.<name>.bib`) are opened as a folder.
fn open_entries(library: &Library, keys: &[String]) {
    for key in keys {
        let files = library.files(key);
        if files.is_empty() {
            log::warning(&format!("{key}: no attached file"));
            continue;
        }
        if files.len() > 1 {
            let parent = commonpath(&files);
            if isfile(&join(&parent, &format!(".{}.bib", basename(&parent)))) {
                open_with_system(&parent);
                continue;
            }
        }
        for f in files {
            log::info(&format!("opening {f} ..."));
            open_with_system(&f);
        }
    }
}
