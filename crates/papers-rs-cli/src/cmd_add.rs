//! `papers add`: the command line of `papers_rs::command::add`.

use std::io::IsTerminal;

use papers_rs::command::add::{AddRequest, add};
use papers_rs::command::files::files_mode;
use papers_rs::command::report::{Notes, print_changes};
use papers_rs::config::paths::isfile;
use papers_rs::error::Error;
use papers_rs::log;
use papers_rs::merge::{OnDuplicate, Resolve, Resolver};

use crate::cmd_write::{Done, check_install, finish, open_session};
use crate::dialogue::Dialogue;
use crate::parse::Leaf;
use crate::util::file_uri_to_path;

/// The `--on-duplicate` word of a deprecated `-m/--mode` letter of `add`.
fn mode_word(letter: &str) -> &'static str {
    match letter {
        "i" => "ask",
        "u" | "m" => "keep",
        "U" | "o" => "update",
        "s" => "skip",
        _ => "error",
    }
}

/// What a letter of `-m/--mode` was, as a hint (`ADD_MODES`).
fn mode_note(letter: &str) -> &'static str {
    match letter {
        "o" => " (the key and the fields only in the library are now kept)",
        "s" => " (the file of the added entry is no longer attached; keep does that)",
        _ => "",
    }
}

/// `-a NAME` is `--author NAME`; the other values of `-a` are files to add
/// (`-a NAME FILE`). The v2 form `-a FILE...` (attach): when every value is an
/// existing file or a URL, they are attached as with `--attach`, with a warning.
fn author_short(
    values: &[String],
    attachments: &mut Vec<String>,
    author: &mut Option<String>,
    files: &mut Vec<String>,
) -> Result<(), Error> {
    let converted: Vec<String> = values.iter().map(|v| file_uri_to_path(v)).collect();
    if converted
        .iter()
        .all(|v| isfile(v) || v.starts_with("http://") || v.starts_with("https://"))
    {
        let one = converted.len() == 1;
        log::warning(&format!(
            "-a now means --author; {} {}, so {} attached as with --attach (this fallback is removed in v4)",
            converted.join(" "),
            if one { "exists" } else { "exist" },
            if one { "it is" } else { "they are" }
        ));
        attachments.extend(converted);
        return Ok(());
    }
    let name = values[0].clone();
    if converted[0] != name || papers_rs::config::paths::exists(&name) {
        return Err(Error::exit(format!(
            "-a {name}: a file, but -a now means --author: use --attach FILE to attach it, or --author NAME"
        )));
    }
    if author.is_some() {
        return Err(Error::exit("-a is the same as --author: give one of them"));
    }
    *author = Some(name);
    files.extend(converted[1..].iter().cloned());
    Ok(())
}

/// `--field=value` pairs of `--metadata`/`--set`: the name stripped, the value as given.
pub fn field_values(texts: &[String]) -> Vec<(String, String)> {
    texts
        .iter()
        .filter_map(|t| t.split_once('='))
        .map(|(name, value)| (name.trim().to_string(), value.to_string()))
        .collect()
}

/// The resolver of the duplicates: `--on-duplicate`, else the deprecated
/// `-m/--mode` letter, else ask in a terminal and error otherwise.
fn duplicate_resolver(
    leaf: &Leaf,
    config: &papers_rs::config::Config,
    no_check_duplicate: &mut bool,
    color: bool,
) -> Box<dyn Resolve> {
    let mut mode = leaf.string("on_duplicate");
    if let Some(letter) = leaf.string("mode") {
        if letter == "a" {
            log::warning("-m/--mode a is deprecated: use --no-check-duplicate");
            *no_check_duplicate = true;
        } else {
            let new = mode_word(&letter);
            log::warning(&format!(
                "-m/--mode {letter} is deprecated: use --on-duplicate {new}{}",
                mode_note(&letter)
            ));
            if mode.is_none() {
                mode = Some(new.to_string());
            }
        }
    }
    let mode = mode.unwrap_or_else(|| {
        if std::io::stdin().is_terminal() {
            "ask"
        } else {
            "error"
        }
        .to_string()
    });
    let mode = OnDuplicate::parse(&mode).unwrap_or(OnDuplicate::Error);
    if mode == OnDuplicate::Ask {
        Box::new(Dialogue::new(
            color,
            papers_rs::command::edit::Editor::of(config),
        ))
    } else {
        Box::new(Resolver::new(mode))
    }
}

pub fn add_cmd(leaf: &Leaf, config: papers_rs::config::Config) -> Done {
    let config = check_install(leaf, config, true)?;
    let mut request = AddRequest {
        files: leaf.strings("file"),
        format: leaf.string("format"),
        attachments: leaf.strings("attachment"),
        metadata: field_values(&leaf.strings("metadata")),
        doi: leaf.string("doi"),
        key: leaf.string("key"),
        title: leaf.string("title"),
        author: leaf.string("author"),
        journal: leaf.string("journal"),
        year: leaf.string("year"),
        entry_type: leaf.string("type"),
        update_key: leaf.flag("update_key"),
        check_duplicate: !leaf.flag("no_check_duplicate"),
        edit: leaf.flag("edit"),
        ignore_errors: leaf.flag("ignore_errors"),
        recursive: leaf.flag("recursive"),
        no_query_doi: leaf.flag("no_query_doi"),
        no_query_fulltext: leaf.flag("no_query_fulltext"),
        scholar: leaf.flag("scholar"),
        no_pool: leaf.flag("no_pool"),
        pool: leaf.string("pool"),
        ..AddRequest::default()
    };
    let deprecated = leaf.strings("attachment_deprecated");
    if !deprecated.is_empty() {
        log::warning("--attachment is deprecated: use --attach");
        request.attachments.extend(deprecated);
    }
    let short = leaf.strings("author_short");
    if !short.is_empty() {
        author_short(
            &short,
            &mut request.attachments,
            &mut request.author,
            &mut request.files,
        )?;
    }
    request.mode = files_mode(
        leaf.flag("link"),
        leaf.flag("move") || leaf.flag("rename"),
        leaf.flag("copy"),
        &config,
    )?;
    if leaf.flag("no_merge_files") {
        log::warning(
            "--no-merge-files is deprecated and ignored: the file links of duplicates are always merged",
        );
    }
    let color = crate::cmd_write::color_enabled(leaf);
    let mut no_check = leaf.flag("no_check_duplicate");
    let resolver = duplicate_resolver(leaf, &config, &mut no_check, color);
    request.check_duplicate = !no_check;
    run(leaf, config, request, resolver)
}

fn run(
    leaf: &Leaf,
    config: papers_rs::config::Config,
    request: AddRequest,
    mut resolver: Box<dyn Resolve>,
) -> Done {
    let session = open_session(leaf, config, false)?;
    let open = leaf.flag("open");
    let porcelain = leaf.flag("porcelain");
    let mut session = session;
    let result = (|| -> Done {
        let touched = add(&mut session, &request, resolver.as_mut())?;
        session.commit()?;
        let notes = Notes::new();
        let color = session.color();
        print_changes(
            &session.library,
            &touched,
            true,
            &notes,
            porcelain,
            color,
            &session.say,
        );
        if open {
            open_entries(&session, &touched);
        }
        Ok(())
    })();
    finish(session);
    result
}

/// The files of the entries with the application of the system (`view_entry_files`), the
/// entries in the order of the library.
fn open_entries(session: &papers_rs::command::Session, keys: &[String]) {
    let wanted: std::collections::HashSet<String> = keys.iter().map(|k| k.to_lowercase()).collect();
    let entries: Vec<String> = session
        .library
        .entries()
        .filter(|e| wanted.contains(&e.key.to_lowercase()))
        .map(|e| e.key.clone())
        .collect();
    for key in entries {
        let files = session.library.files(&key);
        if files.len() > 1 {
            let parent = commonpath_of(&files);
            let hidden = papers_rs::config::paths::join(
                &parent,
                &format!(".{}.bib", papers_rs::config::paths::basename(&parent)),
            );
            if isfile(&hidden) {
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

/// Open a file or a folder with the application that the system associates with it.
pub fn open_with_system(path: &str) {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(program).arg(path).status();
}

/// `os.path.commonpath` of absolute paths.
fn commonpath_of(paths: &[String]) -> String {
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
