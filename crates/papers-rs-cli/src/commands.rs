//! The commands of papers, their options as papers' parser (papers/cli/parser.py)
//! defines them.

use papers_rs::config::settings::SETTINGS;
use papers_rs::config::settings::Kind;

use crate::spec::{Builder, Nargs, Spec};

/// The commands in the help of papers.
pub const COMMANDS: [&str; 22] = [
    "status", "init", "config", "migrate", "add", "check", "file", "list", "edit", "rm", "tag", "pull", "push", "sync",
    "export", "open", "fetch", "extract", "undo", "redo", "restore", "history",
];

/// The deprecated commands (removed in v4): they work, with a warning, and the help does not show them.
pub const DEPRECATED_COMMANDS: [&str; 7] = ["install", "uninstall", "filecheck", "doi", "restore-backup", "backup", "git"];

pub const FILE_COMMANDS: [&str; 9] =
    ["rename", "mv", "attach", "detach", "rm", "check", "fix", "clean", "checksum"];

/// All the commands that papers parses.
pub fn is_command(name: &str) -> bool {
    COMMANDS.contains(&name) || DEPRECATED_COMMANDS.contains(&name)
}

const COLORS: &[&str] = &["auto", "always", "never"];
const FORMATS_ALL: &[&str] = &["bibtex", "ris", "csl-json", "bibjson"];
const ON_DUPLICATE_ALL: &[&str] = &["ask", "keep", "update", "skip", "error"];
const ON_DUPLICATE_CHECK: &[&str] = &["ask", "keep", "skip", "error"];
const FILES_MODE: &[&str] = &["copy", "link", "none"];

// the options shared by the commands

fn logging(b: &mut Builder) {
    b.flag("verbose", &["-v", "--verbose"]);
    b.flag("debug", &["--debug"]);
    b.flag("info", &["--info"]);
    b.flag("warn", &["--warn"]);
    b.flag("error", &["--error"]);
    b.choice("color", &["--color"], COLORS);
    b.flag("porcelain", &["--porcelain"]);
    b.hide(&["info", "warn", "error"]);
}

fn config_options(b: &mut Builder) {
    logging(b);
    b.opt("filesdir", &["--filesdir"], Nargs::One);
    b.opt("bibtex", &["--bibtex"], Nargs::One);
    b.flag("dry_run", &["--dry-run"]);
    b.flag("relative_paths", &["--relative-paths"]);
    b.flag("absolute_paths", &["--absolute-paths"]);
    b.flag("no_git", &["--no-git"]);
    b.hide(&["relative_paths", "absolute_paths", "no_git"]);
}

/// The name or key format options (`--name-template`, `--key-author-num`...).
fn format_options(b: &mut Builder, kind: &str) {
    for (name, setting_kind) in SETTINGS.iter() {
        let Some(field) = name.strip_prefix(&format!("{kind}.")) else { continue };
        let option = format!("--{kind}-{field}");
        let id = format!("{kind}-{field}");
        match setting_kind {
            Kind::Bool => {
                b.boolean(&id, &format!("{kind}-{field}"));
            }
            Kind::Int => {
                b.int(&id, &[&option]);
            }
            _ => {
                b.opt(&id, &[&option], Nargs::One);
            }
        }
    }
}

fn files_mode(b: &mut Builder, add: bool) {
    b.flag("move", &["--move"]);
    if add {
        b.flag("rename", &["-r", "--rename"]);
        b.flag("copy", &["-c", "--copy"]);
    } else {
        b.flag("copy", &["--copy"]);
    }
    b.flag("link", &["--link"]);
}

/// The definition of a command (`file rename` for a subcommand of `file`).
pub fn spec(path: &str) -> Option<Spec> {
    let prog = format!("papers {path}");
    let mut b = Builder::new(&prog);
    match path {
        "status" => {
            config_options(&mut b);
            b.flag("no_check_files", &["--no-check-files"]);
        }
        "init" => {
            logging(&mut b);
            format_options(&mut b, "name");
            format_options(&mut b, "key");
            b.pos("dir", "DIR", Nargs::Opt);
            b.opt("bibtex", &["--bibtex"], Nargs::One);
            b.opt("filesdir", &["--filesdir"], Nargs::One);
            b.opt("pool", &["--pool"], Nargs::One);
            b.flag("shallow", &["--shallow"]);
            b.flag("is_global", &["--global"]);
            b.choice("paths", &["--paths"], &["relative", "absolute"]);
            b.flag("git", &["--git"]);
            b.flag("no_git", &["--no-git"]);
            b.flag("git_lfs", &["--git-lfs"]);
            b.opt("editor", &["--editor"], Nargs::One);
            b.hide(&["info", "warn", "error", "git", "no_git", "git_lfs"]);
        }
        "config" => {
            logging(&mut b);
            // `KEY [VALUE ...]`: clap does not take an optional positional before a repeated one
            b.pos("key_values", "KEY", Nargs::Star);
            b.opt("unset", &["--unset"], Nargs::One);
            b.flag("prune", &["--prune"]);
            b.flag("is_global", &["--global"]);
            b.flag("yes", &["-y", "--yes"]);
            b.flag("dry_run", &["--dry-run"]);
        }
        "migrate" => {
            config_options(&mut b);
            b.flag("relative", &["--relative"]);
            b.flag("folders", &["--folders"]);
        }
        "install" => {
            config_options(&mut b);
            format_options(&mut b, "name");
            format_options(&mut b, "key");
            b.flag("edit", &["--edit"]);
            b.flag("reset", &["--reset"]);
            b.flag("yes", &["-y", "--yes"]);
            b.flag("no_prompt", &["--no-prompt", "--force"]);
            b.flag("local", &["--local"]);
            b.flag("global_install", &["--global"]);
            b.flag("git", &["--git", "--backup"]);
            b.flag("git_lfs", &["--git-lfs", "--backup-files"]);
            b.opt("editor", &["--editor"], Nargs::One);
            b.opt("pool", &["--pool"], Nargs::One);
            b.flag("no_check_files", &["--no-check-files"]);
            b.hide(&["no_prompt", "local", "git", "git_lfs", "pool"]);
        }
        "uninstall" => {
            logging(&mut b);
            b.flag("recursive", &["--recursive"]);
            b.flag("remove_history", &["--remove-backup", "--remove-history"]);
            b.hide(&["recursive"]);
        }
        "add" => {
            config_options(&mut b);
            format_options(&mut b, "name");
            format_options(&mut b, "key");
            b.pos("file", "file", Nargs::Star);
            b.choice("format", &["--format"], FORMATS_ALL);
            b.choice("on_duplicate", &["--on-duplicate"], ON_DUPLICATE_ALL);
            b.flag("no_check_duplicate", &["--no-check-duplicate"]);
            b.flag("no_pool", &["--no-pool"]);
            b.opt("pool", &["--pool"], Nargs::One);
            b.flag("no_merge_files", &["--no-merge-files"]);
            b.flag("update_key", &["-u", "--update-key"]);
            b.choice("mode", &["-m", "--mode"], &["u", "U", "o", "s", "r", "i", "a"]);
            b.flag("recursive", &["-R", "--recursive"]);
            b.flag("ignore_errors", &["--ignore-errors"]);
            b.opt("doi", &["--doi"], Nargs::One);
            b.opt("key", &["--key"], Nargs::One);
            b.opt("type", &["--type"], Nargs::One);
            b.opt("title", &["--title"], Nargs::One);
            b.opt("author", &["--author"], Nargs::One);
            b.opt("author_short", &["-a"], Nargs::Plus);
            b.opt("journal", &["--journal"], Nargs::One);
            b.opt("year", &["--year"], Nargs::One);
            b.fields("metadata", &["--metadata"]);
            b.flag("no_query_doi", &["--no-query-doi"]);
            b.flag("no_query_fulltext", &["--no-query-fulltext"]);
            b.flag("scholar", &["--scholar"]);
            b.flag("openalex", &["--openalex"]);
            b.exclusive(&["scholar", "openalex"]);
            b.opt("attachment", &["--attach"], Nargs::Plus);
            b.opt("attachment_deprecated", &["--attachment"], Nargs::Plus);
            files_mode(&mut b, true);
            b.flag("edit", &["-e", "--edit"]);
            b.flag("open", &["-o", "--open"]);
            b.hide(&["no_merge_files", "mode", "attachment_deprecated"]);
        }
        "check" => {
            config_options(&mut b);
            format_options(&mut b, "key");
            b.pos("entries", "KEY", Nargs::Star);
            b.opt("keys", &["-k", "--keys"], Nargs::Plus);
            b.flag("yes", &["-y", "--yes"]);
            b.flag("force", &["-f", "--force"]);
            b.flag("fix_key", &["--fix-key"]);
            b.flag("key_ascii", &["--key-ascii"]);
            b.flag("auto_key", &["--auto-key"]);
            b.flag("fix_doi", &["--fix-doi"]);
            b.flag("fetch", &["--fetch"]);
            b.flag("fetch_all", &["--fetch-all"]);
            b.flag("format_name", &["--format-name"]);
            b.choice("encoding", &["--encoding"], &["latex", "unicode"]);
            b.flag("duplicates", &["--duplicates"]);
            b.choice("on_duplicate", &["--on-duplicate"], ON_DUPLICATE_CHECK);
            b.choice("mode", &["-m", "--mode"], &["i", "m", "s"]);
            b.hide(&["force", "mode"]);
        }
        "file" => {}
        "file rename" => {
            config_options(&mut b);
            format_options(&mut b, "name");
            b.pos("entries", "KEY", Nargs::Star);
            files_mode(&mut b, false);
            b.flag("keep_source", &["--keep-source"]);
        }
        "file mv" => {
            config_options(&mut b);
            b.pos("paths", "SRC... DEST", Nargs::Plus);
            b.flag("force", &["-f", "--force"]);
            b.flag("n", &["-n"]);
        }
        "file attach" => {
            config_options(&mut b);
            format_options(&mut b, "name");
            b.pos("key", "KEY", Nargs::One);
            b.pos("files", "FILE", Nargs::Plus);
            files_mode(&mut b, false);
        }
        "file detach" => {
            config_options(&mut b);
            b.pos("files", "FILE", Nargs::Star);
            b.append("key", &["-k", "--key"], Nargs::Plus);
            b.flag("yes", &["-y", "--yes"]);
            b.flag("recursive", &["-r"]);
            b.flag("broken", &["--broken"]);
        }
        "file rm" => {
            config_options(&mut b);
            b.pos("files", "FILE", Nargs::Star);
            b.append("key", &["-k", "--key"], Nargs::Plus);
            b.flag("recursive", &["-r"]);
        }
        "file check" => {
            config_options(&mut b);
            b.pos("entries", "KEY", Nargs::Star);
            b.flag("metadata", &["--metadata"]);
            b.append("pool", &["--pool"], Nargs::One);
            b.append("search", &["--search"], Nargs::One);
        }
        "file fix" => {
            config_options(&mut b);
            b.pos("entries", "KEY", Nargs::Star);
            b.append("pool", &["--pool"], Nargs::One);
            b.append("search", &["--search"], Nargs::One);
            b.flag("move", &["--move"]);
        }
        "file clean" => {
            config_options(&mut b);
            b.flag("yes", &["-y", "--yes"]);
        }
        "file checksum" => {
            config_options(&mut b);
            b.pos("entries", "KEY", Nargs::Star);
            b.flag("check", &["--check"]);
        }
        "filecheck" => {
            config_options(&mut b);
            format_options(&mut b, "name");
            b.flag("rename", &["-r", "--rename"]);
            b.flag("copy", &["-c", "--copy"]);
            b.flag("metadata_check", &["--metadata-check"]);
            b.flag("hash_check", &["--hash-check"]);
            b.flag("delete_broken", &["-d", "--delete-broken"]);
            b.flag("clean_filesdir", &["--clean-filesdir"]);
            b.flag("fix_mendeley", &["--fix-mendeley"]);
            b.flag("force", &["--force"]);
        }
        "list" => {
            b = b.numbers_are_options();
            config_options(&mut b);
            b.pos("fullsearch", "fullsearch", Nargs::Star);
            b.flag("strict", &["--strict"]);
            b.flag("fuzzy", &["--fuzzy"]);
            b.exclusive(&["strict", "fuzzy"]);
            b.int("fuzzy_ratio", &["--fuzzy-ratio"]);
            b.opt("similarity", &["--similarity"], Nargs::One);
            b.flag("invert", &["--invert"]);
            b.flag("any", &["--any"]);
            b.opt("author", &["-a", "--author"], Nargs::Plus);
            b.opt("first_author", &["--first-author"], Nargs::Plus);
            b.opt("year", &["-y", "--year"], Nargs::Plus);
            b.opt("title", &["-t", "--title"], Nargs::Plus);
            b.opt("abstract", &["--abstract"], Nargs::Plus);
            b.opt("key", &["-k", "--key", "--id"], Nargs::Plus);
            b.opt("doi", &["--doi"], Nargs::Plus);
            b.opt("keywords", &["--keywords", "--tag"], Nargs::Plus);
            b.flag("duplicates_key", &["--duplicates-key"]);
            b.flag("duplicates_doi", &["--duplicates-doi"]);
            b.flag("duplicates_tit", &["--duplicates-tit"]);
            b.choice("by", &["--by"], &["key", "doi", "title"]);
            b.flag("duplicates", &["--duplicates"]);
            b.flag("has_file", &["--has-file"]);
            b.flag("no_file", &["--no-file"]);
            b.flag("broken_file", &["--broken-file"]);
            b.flag("review_required", &["--review-required"]);
            b.choice("format", &["--format"], LIST_FORMATS);
            b.flag("plain", &["--plain"]);
            b.flag("one_liner", &["-l", "-1", "--one-liner"]);
            b.flag("key_only", &["-K", "--key-only"]);
            b.opt("field", &["--field"], Nargs::Plus);
            b.opt("field_f", &["-f"], Nargs::Plus);
            b.exclusive(&["format", "plain", "one_liner", "key_only", "field", "field_f"]);
            b.flag("no_key", &["--no-key"]);
            b.flag("delete", &["--delete"]);
            b.flag("delete_files", &["--delete-files"]);
            b.flag("keep_files", &["--keep-files"]);
            b.flag("edit", &["-e", "--edit"]);
            b.opt("add_keywords", &["--add-keywords", "--add-tag"], Nargs::Plus);
            b.flag("rename", &["--rename"]);
            b.flag("keep_source", &["--keep-source"]);
            b.flag("open", &["-o", "--open"]);
            b.opt("attach", &["--attach"], Nargs::Plus);
            b.opt("rm_tag", &["--rm-tag"], Nargs::Plus);
            b.opt("detach", &["--detach"], Nargs::Star);
            b.flag("detach_broken", &["--detach-broken"]);
            b.opt("fix_files", &["--fix-files"], Nargs::Star);
            b.flag("move", &["--move"]);
            b.flag("push", &["--push"]);
            b.flag("pull", &["--pull"]);
            b.flag("sync", &["--sync"]);
            b.append("pool", &["--pool"], Nargs::One);
            b.choice("on_duplicate", &["--on-duplicate"], ON_DUPLICATE_ALL);
            b.opt("export", &["--export"], Nargs::One);
            b.choice("export_files", &["--export-files"], FILES_MODE);
            b.append("export_format", &["--export-format"], Nargs::One);
            b.flag("fetch", &["--fetch"]);
            b.opt("add_files", &["--add-files"], Nargs::Plus);
            b.flag("copy", &["--copy"]);
            b.hide(&[
                "similarity", "duplicates_key", "duplicates_doi", "duplicates_tit", "field_f", "fetch", "add_files",
                "copy",
            ]);
        }
        "edit" => {
            config_options(&mut b);
            b.pos("entries", "KEY", Nargs::Plus);
            b.opt("from_file", &["--from"], Nargs::One);
            b.choice("format", &["--format"], FORMATS_ALL);
            b.fields("set", &["--set"]);
            b.opt("unset", &["--unset"], Nargs::Plus);
            b.opt("new_key", &["--key"], Nargs::One);
            b.flag("keep_old_key", &["--keep-old-key"]);
        }
        "rm" => {
            config_options(&mut b);
            b.pos("entries", "KEY", Nargs::Plus);
            b.flag("delete_files", &["--delete-files"]);
            b.flag("keep_files", &["--keep-files"]);
            b.flag("yes", &["-y", "--yes"]);
        }
        "tag" => {
            config_options(&mut b);
            b.pos("entries", "KEY", Nargs::Star);
            b.opt("add", &["--add"], Nargs::Plus);
            b.opt("rm", &["--rm"], Nargs::Plus);
            b.opt("rename", &["--rename"], Nargs::Exact(2));
        }
        "pull" => {
            config_options(&mut b);
            b.pos("entries", "KEY", Nargs::Star);
            b.opt("pool", &["--pool"], Nargs::One);
            b.flag("all", &["--all"]);
            b.choice("on_duplicate", &["--on-duplicate"], ON_DUPLICATE_ALL);
            b.flag("check", &["--check"]);
        }
        "push" => {
            config_options(&mut b);
            b.pos("entries", "KEY", Nargs::Star);
            b.flag("all", &["--all"]);
            b.opt("pool", &["--pool"], Nargs::One);
            b.choice("on_duplicate", &["--on-duplicate"], ON_DUPLICATE_ALL);
        }
        "sync" => {
            config_options(&mut b);
            b.pos("entries", "KEY", Nargs::Star);
            b.flag("all", &["--all"]);
            b.opt("pool", &["--pool"], Nargs::One);
            b.choice("on_duplicate", &["--on-duplicate"], ON_DUPLICATE_ALL);
        }
        "export" => {
            config_options(&mut b);
            b.pos("dir", "DIR", Nargs::One);
            b.pos("entries", "KEY", Nargs::Star);
            b.choice("files", &["--files"], FILES_MODE);
            b.append("format", &["--format"], Nargs::One);
        }
        "open" => {
            config_options(&mut b);
            b.pos("key", "key", Nargs::Plus);
        }
        "doi" => {
            logging(&mut b);
            b.pos("pdf", "pdf", Nargs::One);
            b.flag("image", &["--image"]);
        }
        "fetch" => {
            logging(&mut b);
            b.pos("doi_or_text", "doi_or_text", Nargs::Plus);
            b.flag("scholar", &["--scholar"]);
            b.flag("openalex", &["--openalex"]);
            b.exclusive(&["scholar", "openalex"]);
        }
        "extract" => {
            logging(&mut b);
            b.pos("pdf", "pdf", Nargs::Plus);
            b.flag("doi_only", &["--doi-only"]);
            b.flag("recursive", &["-R", "--recursive"]);
            b.int("word_count", &["-n", "--word-count"]);
            b.flag("fulltext", &["--fulltext"]);
            b.flag("scholar", &["--scholar"]);
            b.flag("openalex", &["--openalex"]);
            b.exclusive(&["scholar", "openalex"]);
            b.flag("image", &["--image"]);
        }
        "undo" | "redo" => {
            config_options(&mut b);
            b.flag("restore_files", &["--restore-files"]);
            b.steps("steps", &["-n", "--steps"]);
            b.flag("library_only", &["--library-only"]);
            b.hide(&["restore_files"]);
        }
        "restore" | "restore-backup" => {
            config_options(&mut b);
            b.flag("restore_files", &["--restore-files"]);
            b.pos("state", "STEP", Nargs::Opt);
            b.opt("reference", &["--ref"], Nargs::One);
            b.hide(&["restore_files"]);
        }
        "git" => {
            b.remainder("gitargs", "gitargs");
        }
        "history" | "backup" => {
            config_options(&mut b);
            b.flag("all", &["--all"]);
            b.opt("remove", &["--remove"], Nargs::Opt);
            b.opt("drop_files", &["--drop-files"], Nargs::One);
            b.opt("revert", &["--revert"], Nargs::One);
            b.choice("on_duplicate", &["--on-duplicate"], ON_DUPLICATE_ALL);
            b.int("n", &["-n"]);
            b.flag("yes", &["-y", "--yes"]);
            if path == "backup" {
                // `[action] [patterns ...]`
                b.pos("action_patterns", "action", Nargs::Star);
                b.hide(&["action_patterns"]);
            }
        }
        _ => return None,
    }
    Some(b.finish())
}

const LIST_FORMATS: &[&str] = &["line", "bibtex", "ris", "csl-json", "bibjson", "key"];
