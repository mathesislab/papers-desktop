//! Parsing the command line as papers' argparse parser does: the command
//! (and the subcommand of `file`), the options of the command, and the
//! errors of a command line that does not fit (usage, message, status 2).

use clap::ArgMatches;
use clap::error::{ContextKind, ContextValue, ErrorKind};
use papers_rs::config::json::py_repr_str;

use crate::commands::{self, COMMANDS, FILE_COMMANDS};
use crate::spec::{Nargs, Spec};

/// What ends the parsing before a command runs.
#[derive(Debug)]
pub enum Stop {
    /// `-h`: the help of the command (its path) is printed on the standard output, status 0.
    Help(String),
    /// An error of the command line: the usage and the message are printed
    /// on the standard error, status 2. `path` is the command whose parser
    /// reports it (empty: papers itself).
    Usage { path: String, message: String },
}

/// The command line parsed: `command` is `None` for papers itself, `file`
/// without subcommand, or the command.
pub struct Parsed {
    pub version: bool,
    /// `init`, `file rename`... (`None`: no command given)
    pub command: Option<String>,
    pub leaf: Option<Leaf>,
}

pub struct Leaf {
    pub matches: ArgMatches,
}

impl Leaf {
    pub fn flag(&self, id: &str) -> bool {
        self.matches.try_get_one::<bool>(id).ok().flatten().copied().unwrap_or(false)
    }

    pub fn string(&self, id: &str) -> Option<String> {
        self.matches.try_get_one::<String>(id).ok().flatten().cloned()
    }

    pub fn strings(&self, id: &str) -> Vec<String> {
        self.matches
            .try_get_many::<String>(id)
            .ok()
            .flatten()
            .map(|values| values.cloned().collect())
            .unwrap_or_default()
    }

    /// Position of an option on the command line (to tell which of two came
    /// last).
    pub fn index(&self, id: &str) -> Option<usize> {
        if self.flag(id) { self.matches.index_of(id) } else { None }
    }
}

fn is_help(arg: &str) -> bool {
    arg == "-h" || (arg.len() >= 3 && "--help".starts_with(arg) && arg.starts_with("--"))
}

fn looks_like_negative_number(arg: &str) -> bool {
    let rest = arg.strip_prefix('-').unwrap_or("");
    !rest.is_empty()
        && (rest.chars().all(|c| c.is_ascii_digit())
            || rest
                .split_once('.')
                .is_some_and(|(a, b)| a.chars().all(|c| c.is_ascii_digit()) && !b.is_empty() && b.chars().all(|c| c.is_ascii_digit())))
}

fn is_option(arg: &str) -> bool {
    arg.starts_with('-') && arg != "-" && !looks_like_negative_number(arg)
}

/// The argparse message of an error of clap.
fn translate(spec: &Spec, error: &clap::Error) -> String {
    let id_of = |text: &str| spec.rendered.get(text).cloned();
    let name_of = |value: Option<&ContextValue>| -> Option<String> {
        match value? {
            ContextValue::String(text) => id_of(text).and_then(|id| spec.info.get(&id)).map(|i| i.display.clone()),
            _ => None,
        }
    };
    let invalid_arg = error.get(ContextKind::InvalidArg);
    match error.kind() {
        ErrorKind::MissingRequiredArgument => {
            let names: Vec<String> = match invalid_arg {
                Some(ContextValue::Strings(texts)) => texts
                    .iter()
                    .filter_map(|t| id_of(t).and_then(|id| spec.info.get(&id)).map(|i| i.display.clone()))
                    .collect(),
                Some(ContextValue::String(text)) => {
                    id_of(text).and_then(|id| spec.info.get(&id)).map(|i| vec![i.display.clone()]).unwrap_or_default()
                }
                _ => Vec::new(),
            };
            format!("the following arguments are required: {}", names.join(", "))
        }
        ErrorKind::ValueValidation => {
            let name = name_of(invalid_arg).unwrap_or_default();
            let reason = std::error::Error::source(error).map(|s| s.to_string()).unwrap_or_default();
            format!("argument {name}: {reason}")
        }
        ErrorKind::InvalidValue | ErrorKind::TooFewValues | ErrorKind::WrongNumberOfValues => {
            let name = name_of(invalid_arg).unwrap_or_default();
            let nargs = id_of(match invalid_arg {
                Some(ContextValue::String(text)) => text,
                _ => "",
            })
            .and_then(|id| spec.info.get(&id).map(|i| i.nargs));
            let expected = match nargs {
                Some(Nargs::Plus) => "expected at least one argument".to_string(),
                Some(Nargs::Exact(1)) | Some(Nargs::One) | None => "expected one argument".to_string(),
                Some(Nargs::Exact(n)) => format!("expected {n} arguments"),
                Some(Nargs::Opt) => "expected at most one argument".to_string(),
                Some(Nargs::Star) | Some(Nargs::Flag) => "expected one argument".to_string(),
            };
            format!("argument {name}: {expected}")
        }
        ErrorKind::ArgumentConflict => {
            let name = name_of(invalid_arg).unwrap_or_default();
            let prior = match error.get(ContextKind::PriorArg) {
                Some(value) => name_of(Some(value)).unwrap_or_default(),
                None => String::new(),
            };
            format!("argument {prior}: not allowed with argument {name}")
        }
        ErrorKind::TooManyValues => {
            let name = name_of(invalid_arg).unwrap_or_default();
            let value = match error.get(ContextKind::InvalidValue) {
                Some(ContextValue::String(v)) => v.clone(),
                _ => String::new(),
            };
            format!("argument {name}: ignored explicit argument {}", py_repr_str(&value))
        }
        _ => error.to_string().trim().to_string(),
    }
}

/// The text of an argument that clap did not know.
fn unknown_token(error: &clap::Error) -> Option<String> {
    match error.get(ContextKind::InvalidArg)? {
        ContextValue::String(text) => Some(text.clone()),
        _ => None,
    }
}

/// The warnings that papers prints for the deprecated and ignored options as it reads them, so
/// before the errors of the command line that come after them.
pub fn warn_options(path: &str, args: &[String], limit: usize) {
    const NO_GIT: &str = "papers keeps the history of the libraries without git (see `papers history`)";
    const NO_GIT_LFS: &str = "papers keeps the files that its commands delete in its history (see `papers history`)";
    let Some(spec) = commands::spec(path) else { return };
    let mut warned: Vec<&str> = Vec::new();
    for arg in args[..limit.min(args.len())].iter().take_while(|a| *a != "--") {
        let text = match (arg.as_str(), path) {
            ("--info", _) if spec.info.contains_key("info") => "--info is deprecated: use -v".to_string(),
            ("--no-git", _) if spec.info.contains_key("no_git") => format!("--no-git is ignored: {NO_GIT}"),
            ("--git", "init") => format!("--git is ignored: {NO_GIT}"),
            ("--git-lfs", "init") => format!("--git-lfs is ignored: {NO_GIT_LFS}"),
            _ => continue,
        };
        if !warned.contains(&arg.as_str()) {
            warned.push(arg.as_str());
            papers_rs::log::warning(&text);
        }
    }
}

/// Parse the arguments of a command (`path`: `init`, `file rename`...),
/// leaving the arguments it does not know in `extras`.
pub fn parse_leaf(path: &str, args: &[String], extras: &mut Vec<String>) -> Result<Leaf, Stop> {
    let mut limit = args.len();
    let result = parse_leaf_inner(path, args, extras, &mut limit);
    if result.is_err() {
        warn_options(path, args, limit);
    }
    result
}

/// The position of the option that an error is about: papers read what comes before it only.
fn position_of(spec: &Spec, error: &clap::Error, args: &[String]) -> Option<usize> {
    let Some(ContextValue::String(text)) = error.get(ContextKind::InvalidArg) else { return None };
    let id = spec.rendered.get(text)?;
    let info = spec.info.get(id)?;
    if info.positional {
        return None;
    }
    let names: Vec<&str> = info.display.split('/').collect();
    args.iter().position(|a| names.iter().any(|n| a == n || a.starts_with(&format!("{n}="))))
}

fn parse_leaf_inner(path: &str, args: &[String], extras: &mut Vec<String>, limit: &mut usize) -> Result<Leaf, Stop> {
    let spec = commands::spec(path).expect("a command of papers");
    let usage_error = |message: String| Stop::Usage { path: path.to_string(), message };
    let original = args;
    let mut args: Vec<String> = args.to_vec();
    let wants_help = args.iter().take_while(|a| *a != "--").any(|a| is_help(a));
    for _ in 0..args.len() + 1 {
        match spec.command.clone().try_get_matches_from(args.iter()) {
            Ok(matches) => {
                let leaf = Leaf { matches };
                if leaf.flag("help") {
                    return Err(Stop::Help(path.to_string()));
                }
                return Ok(leaf);
            }
            Err(error) if error.kind() == ErrorKind::UnknownArgument => {
                let Some(token) = unknown_token(&error) else {
                    return Err(usage_error(error.to_string().trim().to_string()));
                };
                // an abbreviation of several options is an error of its own
                if let Some(long) = token.strip_prefix("--") {
                    let name = long.split('=').next().unwrap_or(long);
                    let prefix = format!("--{name}");
                    let matches: Vec<&String> = spec.long_options.iter().filter(|o| o.starts_with(&prefix)).collect();
                    if !name.is_empty() && !matches.contains(&&prefix) && matches.len() > 1 {
                        let names: Vec<&str> = matches.iter().map(|m| m.as_str()).collect();
                        return Err(usage_error(format!("ambiguous option: {} could match {}", token, names.join(", "))));
                    }
                }
                // the argument as written: it may have been given with a value (--x=1)
                let position = args.iter().position(|a| *a == token || a.starts_with(&format!("{token}=")));
                match position {
                    Some(i) => {
                        extras.push(args.remove(i));
                    }
                    None => {
                        // an abbreviation or a combined short option: the first argument that starts as it does
                        match args.iter().position(|a| is_option(a) && (a.starts_with(&token) || token.starts_with(a.as_str()))) {
                            Some(i) => extras.push(args.remove(i)),
                            None => return Err(usage_error(format!("unrecognized arguments: {token}"))),
                        }
                    }
                }
            }
            Err(error)
                if error.kind() == ErrorKind::ArgumentConflict
                    && error.get(ContextKind::InvalidArg) == error.get(ContextKind::PriorArg)
                    && matches!(error.get(ContextKind::InvalidArg), Some(ContextValue::String(text))
                        if spec.rendered.get(text).and_then(|id| spec.info.get(id)).is_some_and(|i| i.positional)) =>
            {
                // a positional argument given again: the last value that makes the command line fit is not recognized
                let found = (0..args.len()).rev().filter(|i| !is_option(&args[*i])).find(|i| {
                    let mut trial = args.clone();
                    trial.remove(*i);
                    spec.command.clone().try_get_matches_from(trial.iter()).is_ok()
                });
                match found {
                    Some(i) => extras.push(args.remove(i)),
                    None => return Err(usage_error(translate(&spec, &error))),
                }
            }
            Err(error)
                if error.kind() == ErrorKind::TooManyValues
                    && matches!(error.get(ContextKind::InvalidArg), Some(ContextValue::String(text))
                        if spec.rendered.get(text).and_then(|id| spec.info.get(id)).is_some_and(|i| i.positional)) =>
            {
                // a positional argument that has its value already: the value is not recognized
                let Some(ContextValue::String(value)) = error.get(ContextKind::InvalidValue) else {
                    return Err(usage_error(error.to_string().trim().to_string()));
                };
                match args.iter().rposition(|a| a == value) {
                    Some(i) => extras.push(args.remove(i)),
                    None => return Err(usage_error(format!("unrecognized arguments: {value}"))),
                }
            }
            Err(error) => {
                if wants_help {
                    return Err(Stop::Help(path.to_string()));
                }
                *limit = position_of(&spec, &error, original).unwrap_or(*limit);
                return Err(usage_error(translate(&spec, &error)));
            }
        }
    }
    Err(usage_error("unrecognized arguments".to_string()))
}

fn commands_list(names: &[&str]) -> String {
    names.join(", ")
}

/// Parse the command line of papers (without `-C`, which the caller took).
pub fn parse(argv: &[String]) -> Result<Parsed, Stop> {
    let mut extras: Vec<String> = Vec::new();
    let mut version = false;
    let top_error = |message: String| Stop::Usage { path: String::new(), message };
    let mut i = 0;
    let mut command: Option<String> = None;
    while i < argv.len() {
        let arg = &argv[i];
        i += 1;
        if arg == "--" {
            if let Some(next) = argv.get(i) {
                command = Some(next.clone());
                i += 1;
            }
            break;
        }
        if !is_option(arg) {
            command = Some(arg.clone());
            break;
        }
        if is_help(arg) {
            return Err(Stop::Help(String::new()));
        }
        let (name, explicit) = match arg.split_once('=') {
            Some((name, value)) if arg.starts_with("--") => (name, Some(value)),
            _ => (arg.as_str(), None),
        };
        if name.len() >= 3 && "--version".starts_with(name) && name.starts_with("--") {
            if let Some(value) = explicit {
                return Err(top_error(format!("argument --version: ignored explicit argument {}", py_repr_str(value))));
            }
            version = true;
        } else if name == "-C" || (name.starts_with("-C") && explicit.is_none()) {
            // -C DIR is taken by the caller when it comes first; here it is accepted and ignored
            if name == "-C" {
                if i >= argv.len() {
                    return Err(top_error("argument -C: expected one argument".to_string()));
                }
                i += 1;
            }
        } else {
            extras.push(arg.clone());
        }
    }
    let Some(command) = command else {
        if !extras.is_empty() {
            return Err(top_error(format!("unrecognized arguments: {}", extras.join(" "))));
        }
        return Ok(Parsed { version, command: None, leaf: None });
    };
    if !commands::is_command(&command) {
        return Err(top_error(format!("unknown command {} (commands: {})", py_repr_str(&command), commands_list(&COMMANDS))));
    }
    let rest = &argv[i..];
    if command == "file" {
        return parse_file(version, rest, extras);
    }
    let leaf = parse_leaf(&command, rest, &mut extras)?;
    finish(version, command, leaf, extras, rest)
}

fn finish(version: bool, command: String, leaf: Leaf, extras: Vec<String>, args: &[String]) -> Result<Parsed, Stop> {
    if !extras.is_empty() {
        warn_options(&command, args, args.len());
        return Err(Stop::Usage { path: command, message: format!("unrecognized arguments: {}", extras.join(" ")) });
    }
    Ok(Parsed { version, command: Some(command), leaf: Some(leaf) })
}

fn parse_file(version: bool, rest: &[String], mut extras: Vec<String>) -> Result<Parsed, Stop> {
    let usage = |message: String| Stop::Usage { path: "file".to_string(), message };
    let mut i = 0;
    let mut sub: Option<String> = None;
    while i < rest.len() {
        let arg = &rest[i];
        i += 1;
        if !is_option(arg) {
            sub = Some(arg.clone());
            break;
        }
        if is_help(arg) {
            return Err(Stop::Help("file".to_string()));
        }
        extras.push(arg.clone());
    }
    let Some(sub) = sub else {
        if !extras.is_empty() {
            return Err(usage(format!("unrecognized arguments: {}", extras.join(" "))));
        }
        return Ok(Parsed { version, command: Some("file".to_string()), leaf: None });
    };
    if !FILE_COMMANDS.contains(&sub.as_str()) {
        return Err(usage(format!("unknown command {} (commands: {})", py_repr_str(&sub), commands_list(&FILE_COMMANDS))));
    }
    let path = format!("file {sub}");
    let leaf = parse_leaf(&path, &rest[i..], &mut extras)?;
    finish(version, path, leaf, extras, &rest[i..])
}
