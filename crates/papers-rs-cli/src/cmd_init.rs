//! `papers init`: create a library.

use std::fs;

use papers_rs::config::paths::{abspath, exists, expanduser, getcwd, is_inside, isdir, join, realpath, relpath, short_home};
use papers_rs::config::{Config, DEFAULT_LIBRARY, NO_POOL, Origin, find_config, library_config_file, library_without_pool};
use papers_rs::error::{Error, Result};
use papers_rs::install::{InstallOptions, apply_install, resolve_install};
use papers_rs::{config::CONFIG_FILE_LOCAL, log};
use serde_json::Value;

use crate::parse::Leaf;
use crate::util::file_uri_to_path;

const NO_DEFAULT: &str = "no default library: `papers init PATH --global` or `papers config --global default PATH`";

pub struct InitArgs {
    pub dir: Option<String>,
    pub bibtex: Option<String>,
    pub filesdir: Option<String>,
    pub pool: Option<String>,
    pub shallow: bool,
    pub is_global: bool,
    pub paths: Option<String>,
    pub editor: Option<String>,
    pub formats: Vec<(String, Value)>,
}

/// The name and key format settings given on the command line, by setting
/// name (`--name-author-num 1` is `name.author-num`).
pub fn format_options(leaf: &Leaf) -> Vec<(String, Value)> {
    let mut options = Vec::new();
    for (name, kind) in papers_rs::config::settings::SETTINGS.iter() {
        if !(name.starts_with("name.") || name.starts_with("key.")) {
            continue;
        }
        let id = name.replace('.', "-");
        match kind {
            papers_rs::config::Kind::Bool => {
                let on = leaf.index(&id);
                let off = leaf.index(&format!("{id}-no"));
                match (on, off) {
                    (Some(a), Some(b)) => options.push((name.clone(), Value::Bool(a > b))),
                    (Some(_), None) => options.push((name.clone(), Value::Bool(true))),
                    (None, Some(_)) => options.push((name.clone(), Value::Bool(false))),
                    (None, None) => {}
                }
            }
            papers_rs::config::Kind::Int => {
                if let Some(value) = leaf.string(&id) {
                    let n = papers_rs::config::settings::py_int(&value).expect("checked by the parser");
                    options.push((name.clone(), Value::from(n)));
                }
            }
            _ => {
                if let Some(value) = leaf.string(&id) {
                    options.push((name.clone(), Value::String(value)));
                }
            }
        }
    }
    options
}

impl InitArgs {
    pub fn from_leaf(leaf: &Leaf) -> InitArgs {
        InitArgs {
            dir: leaf.string("dir"),
            bibtex: leaf.string("bibtex").map(|s| file_uri_to_path(&s)),
            filesdir: leaf.string("filesdir").map(|s| file_uri_to_path(&s)),
            pool: leaf.string("pool"),
            shallow: leaf.flag("shallow"),
            is_global: leaf.flag("is_global"),
            paths: leaf.string("paths"),
            editor: leaf.string("editor"),
            formats: format_options(leaf),
        }
    }
}

/// A path in the plan of a change: relative to the current directory if
/// under it, else with `~`.
pub fn show(path: &str) -> String {
    if is_inside(path, &getcwd()) { relpath(path, ".") } else { short_home(path) }
}

/// The directories that `os.makedirs(path)` would create, deepest first.
fn missing_dirs(path: &str) -> Vec<String> {
    let mut missing = Vec::new();
    let mut path = path.to_string();
    while !exists(&path) {
        missing.push(path.clone());
        let parent = papers_rs::config::paths::dirname(&path);
        if parent == path {
            break;
        }
        path = parent;
    }
    missing
}

/// Create a library in the current directory, or in DIR (created if
/// missing): self-contained, or shallow with `--pool PATH` (an existing
/// library without pool) or `--shallow` (the default library); with
/// `--global`, also registered as the default library. Nothing is created if
/// a check fails.
pub fn init(mut args: InitArgs, config: Config) -> Result<()> {
    let mut machine = config.machine.clone();
    if args.is_global && args.dir.is_none() {
        return Err(Error::exit(
            "--global needs the directory of the library: `papers init PATH --global` (`papers init . --global` for the current directory)",
        ));
    }
    if args.shallow {
        if args.pool.is_some() {
            return Err(Error::exit("--shallow is --pool with the default library: give one of them"));
        }
        match machine.default_library() {
            None => return Err(Error::exit(NO_DEFAULT)),
            Some(default) => args.pool = Some(default),
        }
    }
    let has_pool = args.pool.as_deref().is_some_and(|p| p != NO_POOL);
    if args.is_global && has_pool {
        return Err(Error::exit("--global: the default library has no pool (no --pool or --shallow)"));
    }
    let libdir = abspath(args.dir.as_deref().filter(|d| !d.is_empty()).unwrap_or("."));
    if let Some(pool) = args.pool.as_deref().filter(|p| *p != NO_POOL) {
        let path = abspath(&join(&libdir, &expanduser(pool)));
        if realpath(&path) == realpath(&libdir) {
            return Err(Error::exit(format!("--pool {pool}: this is the library itself (nothing was changed)")));
        }
        library_without_pool(&path, &machine, &format!("--pool {pool}:"))?;
    }
    match args.dir.clone() {
        None => init_here(&args, &config)?,
        Some(dir) => {
            let created = missing_dirs(&libdir);
            fs::create_dir_all(&dir).map_err(|e| Error::other(format!("{dir}: {e}")))?;
            let origin = getcwd();
            let result = (|| {
                std::env::set_current_dir(&dir).map_err(|e| Error::other(format!("{dir}: {e}")))?;
                let found = find_config(".", machine.clone())?;
                init_here(&args, &found)
            })();
            let _ = std::env::set_current_dir(&origin);
            if let Err(error) = result {
                // a check failed: remove the directories created above, if still empty
                for d in created {
                    if isdir(&d) && fs::read_dir(&d).map(|mut e| e.next().is_none()).unwrap_or(false) {
                        let _ = fs::remove_dir(&d);
                    }
                }
                return Err(error);
            }
        }
    }
    if args.is_global {
        machine.set(DEFAULT_LIBRARY, Some(Value::String(short_home(&libdir))));
        machine.save()?;
        log::println(&format!(
            "registered {} as the default library in {}",
            short_home(&libdir),
            short_home(machine.file.as_deref().unwrap_or(""))
        ));
    }
    Ok(())
}

fn init_here(args: &InitArgs, config: &Config) -> Result<()> {
    if let Some(existing) = library_config_file(".")? {
        let hint = if args.is_global {
            " (`papers config --global default PATH` registers an existing library as the default one)"
        } else {
            ""
        };
        return Err(Error::exit(format!(
            "a library already exists here ({existing}): change its settings with `papers config`{hint}"
        )));
    }
    if config.origin == Some(Origin::Library) {
        log::warning(&format!("the new library is inside the library of {}", config.file.as_deref().unwrap_or("")));
    }
    let options = InstallOptions {
        bibtex: args.bibtex.clone(),
        filesdir: args.filesdir.clone(),
        paths: args.paths.clone(),
        pool: args.pool.clone(),
        editor: args.editor.clone(),
        formats: args.formats.clone(),
    };
    let mut plan = resolve_install(Config::new(config.machine.clone()), &options, CONFIG_FILE_LOCAL)?;
    apply_install(&mut plan)?;
    for line in init_summary(&plan.config) {
        log::println(&line);
    }
    Ok(())
}

/// What `papers init` prints: the library created, its bibtex, where its
/// files go and its pool.
fn init_summary(config: &Config) -> Vec<String> {
    let bibtex = config.bibtex.as_deref().filter(|b| !b.is_empty()).map(show).unwrap_or_else(|| "none".to_string());
    let mut lines = vec![format!("created the library {}", short_home(&config.configdir())), format!("* bibtex:  {bibtex}")];
    if !config.has_pool() {
        let files = match config.filesdir.as_deref().filter(|f| !f.is_empty()) {
            Some(filesdir) => format!("{} (the files directory of the library)", show(filesdir)),
            None => "linked where they are (no files directory)".to_string(),
        };
        lines.push(format!("* files:   {files}"));
    } else {
        lines.push("* files:   in the pool (shallow library)".to_string());
        lines.push(format!("* pool:    {}", config.pool_label().unwrap_or_default()));
    }
    lines.push("`papers config` lists and sets the settings".to_string());
    lines
}
