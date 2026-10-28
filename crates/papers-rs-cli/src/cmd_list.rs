//! `papers list`: the entries of the library that a search selects, listed as a line, as bibtex, RIS,
//! CSL-JSON, BibJSON, as keys or as fields; or `--export` to a directory.
//!
//! Its actions that change the library (`--delete`, `--edit`, `--add-tag`...) are in `cmd_list_actions`.

use std::io::{IsTerminal, Read};

use papers_rs::bib::{Bib, Block, Entry};
use papers_rs::config::paths::{abspath, dirname, exists, realpath};
use papers_rs::config::settings::py_int;
use papers_rs::config::{Config, Origin, find_library_config};
use papers_rs::error::Error;
use papers_rs::export::FilesMode;
use papers_rs::formats::line::{Style, field_value, format_entry, format_key};
use papers_rs::formats::{Format, dumps};
use papers_rs::library::Library;
use papers_rs::log;
use papers_rs::search::{By, Search};

use crate::cmd_config::{Failure, no_library_message};
use crate::cmd_export::{export_entries, formats_of};
use crate::cmd_list_actions::{self, Action};
use crate::parse::Leaf;
use crate::util::file_uri_to_path;

/// The configuration that the command acts on, with what its options say about the library
/// (`check_install`): `--bibtex` (the library of that bibtex, or this configuration for it, never
/// written back), `--filesdir`, the deprecated `--relative-paths` and `--absolute-paths`.
pub fn check_install(
    leaf: &Leaf,
    mut config: Config,
    bibtex_must_exist: bool,
) -> Result<Config, Error> {
    if let Some(bibtex) = leaf.string("bibtex").map(|b| file_uri_to_path(&b)) {
        config = config_for_bibtex(&bibtex, config)?;
        config.bibtex = Some(bibtex);
    }
    if let Some(filesdir) = leaf.string("filesdir").map(|f| file_uri_to_path(&f)) {
        config.filesdir = Some(filesdir);
    }
    // of --relative-paths and --absolute-paths, the last one
    let last = [("absolute_paths", true), ("relative_paths", false)]
        .into_iter()
        .filter_map(|(id, absolute)| leaf.index(id).map(|i| (i, absolute)))
        .max_by_key(|(i, _)| *i);
    if let Some((_, absolute)) = last {
        let option = if absolute {
            "--absolute-paths"
        } else {
            "--relative-paths"
        };
        log::warning(&format!(
            "{option} is deprecated: file paths are a setting of the library, `papers config paths {}`",
            if absolute { "absolute" } else { "relative" }
        ));
        config.absolute_paths = absolute;
    }
    let Some(bibtex) = config.bibtex.clone().filter(|b| !b.is_empty()) else {
        return Err(Error::exit(format!(
            "{}, or pass --bibtex",
            no_library_message(&config)
        )));
    };
    if bibtex_must_exist && !exists(&bibtex) {
        return Err(Error::exit(format!(
            "no bibtex file found, do `touch {bibtex}` or `papers init --bibtex {bibtex}`"
        )));
    }
    Ok(config)
}

/// The configuration to use when `--bibtex` is passed (`config_for_bibtex`): the library of that
/// bibtex if it is another one than the configured, else this configuration, without a file.
fn config_for_bibtex(bibtex: &str, config: Config) -> Result<Config, Error> {
    if config
        .bibtex
        .as_deref()
        .is_some_and(|b| !b.is_empty() && realpath(bibtex) == realpath(b))
    {
        return Ok(config);
    }
    if let Some(file) = find_library_config(&dirname(&abspath(bibtex)))? {
        let mut other = Config::load(&file, config.machine.clone(), None)?;
        if other
            .bibtex
            .as_deref()
            .is_some_and(|b| !b.is_empty() && realpath(b) == realpath(bibtex))
        {
            other.origin = Some(Origin::Library);
            return Ok(other);
        }
    }
    let mut config = config;
    config.file = None;
    config.id = None;
    Ok(config)
}

/// The identifiers of the command line, where `-` stands for the lines of the standard input (one
/// per line, blank lines ignored) (`read_selection`).
pub fn read_selection(args: &[String]) -> Vec<String> {
    let mut selection = Vec::new();
    for arg in args {
        if arg == "-" {
            let mut text = String::new();
            let _ = std::io::stdin().read_to_string(&mut text);
            selection.extend(
                text.lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map(str::to_string),
            );
        } else {
            selection.push(arg.clone());
        }
    }
    selection
}

/// Whether the output has colours and hyperlinks: `--color always` or `never`, else (auto) only when
/// the standard output is a terminal and `NO_COLOR` is not set (`color_enabled`).
pub fn color_enabled(choice: &str) -> bool {
    if choice == "always" {
        return true;
    }
    if choice == "never" || std::env::var("NO_COLOR").is_ok_and(|v| !v.is_empty()) {
        return false;
    }
    std::io::stdout().is_terminal()
}

/// The options of `papers list` that act on the entries listed, and are not ported yet.
const WRITING_ACTIONS: [(&str, &str); 12] = [
    ("--delete", "delete"),
    ("--edit", "edit"),
    ("--fetch", "fetch"),
    ("--add-tag", "add_keywords"),
    ("--rm-tag", "rm_tag"),
    ("--attach", "attach"),
    ("--detach", "detach"),
    ("--rename", "rename"),
    ("--fix-files", "fix_files"),
    ("--push", "push"),
    ("--pull", "pull"),
    ("--sync", "sync"),
];

/// Whether an option was given, with or without values (`is not None`).
fn given(leaf: &Leaf, id: &str) -> bool {
    leaf.matches.try_contains_id(id).unwrap_or(false)
}

fn usage(message: impl Into<String>) -> Failure {
    Failure::Usage(message.into())
}

/// The warnings of the deprecated options, in the order they were given.
fn deprecated_warnings(leaf: &Leaf) {
    let mut warnings: Vec<(usize, String)> = Vec::new();
    let mut add = |id: &str, text: &str| {
        if let Some(i) = leaf
            .matches
            .index_of(id)
            .filter(|_| given(leaf, id) && (leaf.flag(id) || !leaf.strings(id).is_empty()))
        {
            warnings.push((i, text.to_string()));
        }
    };
    add("field_f", "-f is deprecated: use --field");
    add(
        "duplicates_key",
        "--duplicates-key is deprecated: use --duplicates --by key",
    );
    add(
        "duplicates_doi",
        "--duplicates-doi is deprecated: use --duplicates --by doi",
    );
    add(
        "duplicates_tit",
        "--duplicates-tit is deprecated: use --duplicates --by title",
    );
    warnings.sort();
    for (_, text) in warnings {
        log::warning(&text);
    }
}

/// The search that the options say.
fn search_of(leaf: &Leaf) -> Search {
    let by = ["duplicates_key", "duplicates_doi", "duplicates_tit"]
        .into_iter()
        .zip([By::Key, By::Doi, By::Title])
        .filter_map(|(id, by)| leaf.index(id).map(|i| (i, by)))
        .chain(
            leaf.string("by")
                .as_deref()
                .and_then(By::parse)
                .map(|by| (leaf.matches.index_of("by").unwrap_or(0), by)),
        )
        .max_by_key(|(i, _)| *i)
        .map(|(_, by)| by);
    Search {
        words: leaf.strings("fullsearch"),
        strict: leaf.flag("strict"),
        fuzzy: leaf.flag("fuzzy"),
        fuzzy_ratio: leaf.string("fuzzy_ratio").as_deref().and_then(py_int),
        invert: leaf.flag("invert"),
        any: leaf.flag("any"),
        author: leaf.strings("author"),
        first_author: leaf.strings("first_author"),
        year: leaf.strings("year"),
        title: leaf.strings("title"),
        abstract_text: leaf.strings("abstract"),
        key: leaf.strings("key"),
        doi: leaf.strings("doi"),
        keywords: leaf.strings("keywords"),
        by,
        duplicates: leaf.flag("duplicates"),
        has_file: leaf.flag("has_file"),
        no_file: leaf.flag("no_file"),
        broken_file: leaf.flag("broken_file"),
        review_required: leaf.flag("review_required"),
    }
}

/// The warning of the deprecated `--similarity`, once the library is read.
pub fn warn_similarity(leaf: &Leaf) {
    if leaf.string("similarity").is_some() {
        log::warning(
            "--similarity is deprecated and ignored: entries are the same paper by identifier, or by authors and title (see the documentation of papers add)",
        );
    }
}

pub fn list_cmd(leaf: &Leaf, config: Config) -> Result<(), Failure> {
    deprecated_warnings(leaf);
    let config = check_install(leaf, config, true)?;
    // the deprecated forms: --add-files, --copy, --fetch
    let add_files = leaf.strings("add_files");
    let attach = !leaf.strings("attach").is_empty() || !add_files.is_empty();
    let (rename, keep_source);
    if !add_files.is_empty() {
        let mode = if leaf.flag("copy") {
            "--copy"
        } else if leaf.flag("rename") {
            "--move"
        } else {
            "--link"
        };
        log::warning(&format!(
            "`papers list --add-files` is deprecated: use `papers list --attach` (`papers file attach KEY FILE... {mode}`)"
        ));
        rename = false;
        keep_source = false;
    } else {
        if leaf.flag("copy") {
            log::warning(
                "`papers list --copy` is deprecated: use `papers list --rename --keep-source`",
            );
        }
        rename = leaf.flag("rename") || leaf.flag("copy");
        keep_source = leaf.flag("keep_source") || leaf.flag("copy");
    }
    if leaf.flag("fetch") {
        log::warning(
            "`papers list --fetch` is deprecated: use `papers list ... -K | papers check --fetch --fetch-all --fix-doi --fix-key --format-name -`",
        );
    }
    let detach = given(leaf, "detach") || leaf.flag("detach_broken");
    let fix_files = given(leaf, "fix_files");
    let exchange = if leaf.flag("pull") {
        Some("pull")
    } else if leaf.flag("push") {
        Some("push")
    } else if leaf.flag("sync") {
        Some("sync")
    } else {
        None
    };
    let export = leaf.string("export").filter(|e| !e.is_empty());
    let actions: Vec<&str> = [
        ("--delete", leaf.flag("delete")),
        ("--edit", leaf.flag("edit")),
        ("--fetch", leaf.flag("fetch")),
        ("--add-tag", !leaf.strings("add_keywords").is_empty()),
        ("--rm-tag", !leaf.strings("rm_tag").is_empty()),
        ("--attach", attach),
        ("--detach", detach),
        ("--rename", rename),
        ("--fix-files", fix_files),
        ("--open", leaf.flag("open")),
        ("--push", leaf.flag("push")),
        ("--pull", leaf.flag("pull")),
        ("--sync", leaf.flag("sync")),
        ("--export", export.is_some()),
    ]
    .into_iter()
    .filter(|(_, on)| *on)
    .map(|(name, _)| name)
    .collect();
    if actions.len() > 1 {
        return Err(usage(format!(
            "one action at a time: {}",
            actions.join(" and ")
        )));
    }
    let pools = leaf.strings("pool");
    for (option, on, needs, action) in [
        (
            "--keep-source",
            leaf.flag("keep_source"),
            rename,
            "--rename",
        ),
        (
            "--move",
            leaf.flag("move"),
            fix_files || rename,
            "--fix-files or --rename",
        ),
        (
            "--pool",
            !pools.is_empty(),
            fix_files || exchange.is_some(),
            "--push, --pull, --sync or --fix-files",
        ),
        (
            "--on-duplicate",
            leaf.string("on_duplicate").is_some(),
            exchange.is_some(),
            "--push, --pull or --sync",
        ),
        (
            "--export-format",
            !leaf.strings("export_format").is_empty(),
            export.is_some(),
            "--export",
        ),
    ] {
        if on && !needs {
            return Err(usage(format!("{option} goes with {action}")));
        }
    }
    let _ = keep_source;
    if leaf.flag("detach_broken") && !leaf.strings("detach").is_empty() {
        return Err(usage("--detach-broken takes no FILE"));
    }
    if let Some(exchange) = exchange
        && pools.len() > 1
    {
        return Err(usage(format!("--{exchange} takes one --pool")));
    }
    if let Some(action) = Action::of(leaf) {
        return cmd_list_actions::run(leaf, config, &search_of(leaf), action);
    }
    for (name, id) in WRITING_ACTIONS {
        let on = match id {
            "attach" => attach,
            "detach" => detach,
            "rename" => rename,
            "fix_files" => fix_files,
            "add_keywords" | "rm_tag" => !leaf.strings(id).is_empty(),
            id => leaf.flag(id),
        };
        if on {
            return Err(Error::exit(format!("{name}: not implemented in papers-rs yet")).into());
        }
    }
    if leaf.flag("open") {
        return Err(Error::exit("--open: not implemented in papers-rs yet").into());
    }
    // the formats of --export-format, before the library is read
    let export_formats = if export.is_some() {
        Some(formats_of(
            "--export-format",
            &leaf.strings("export_format"),
        )?)
    } else {
        None
    };

    let library = Library::open(&config)?;
    let all: Vec<&Entry> = library.entries().collect();
    warn_similarity(leaf);
    let search = search_of(leaf);
    let entries = search
        .select(&all, Some(&library.distinct), &library.relative_to)
        .map_err(Failure::Usage)?;

    if let Some(dir) = export {
        let files = leaf
            .string("export_files")
            .and_then(|f| FilesMode::parse(&f))
            .unwrap_or(FilesMode::Copy);
        export_entries(
            &config,
            &library,
            &entries,
            &dir,
            export_formats.unwrap_or_default(),
            files,
            leaf.flag("dry_run"),
        )?;
        return Ok(());
    }

    let style = Style {
        color: color_enabled(&leaf.string("color").unwrap_or_else(|| "auto".to_string())),
    };
    let no_key = leaf.flag("no_key");
    let mut fields = leaf.strings("field");
    if fields.is_empty() {
        fields = leaf.strings("field_f");
    }
    // the last of --format, --plain, -l, -K decides (they exclude each other)
    let format = if leaf.flag("plain") {
        "bibtex".to_string()
    } else if leaf.flag("one_liner") {
        "line".to_string()
    } else if leaf.flag("key_only") {
        "key".to_string()
    } else {
        leaf.string("format").unwrap_or_else(|| "line".to_string())
    };
    if !fields.is_empty() {
        for e in &entries {
            let mut parts = vec![format_key(e, no_key, style)];
            parts.extend(fields.iter().map(|k| field_value(e, k).to_string()));
            log::println(&parts.join(" "));
        }
    } else if format == "key" {
        for e in &entries {
            log::println(&e.key);
        }
    } else if format == "bibtex" {
        let bib = Bib {
            blocks: entries.iter().map(|e| Block::Entry((*e).clone())).collect(),
        };
        log::println(&bib.format());
    } else if let Some(f) = Format::from_id(&format) {
        log::print(&dumps(&entries, f, &|e| {
            papers_rs::entry::files(e, Some(&library.relative_to))
        }));
    } else {
        for e in &entries {
            log::println(&format_entry(e, &library.relative_to, no_key, "", style));
        }
    }
    Ok(())
}
