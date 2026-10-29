//! The arguments of papers' commands, as clap commands, with what is needed
//! to report their errors as argparse does (the names of the arguments as
//! argparse writes them).

use std::collections::HashMap;

use clap::builder::ValueParser;
use clap::{Arg, ArgAction, Command};

/// How many values an argument takes (argparse's `nargs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nargs {
    Flag,
    One,
    /// `?`
    Opt,
    /// `*`
    Star,
    /// `+`
    Plus,
    Exact(usize),
}

#[derive(Debug, Clone)]
pub struct ArgInfo {
    /// How argparse names the argument in its errors: `-k/--key`, or the
    /// metavar of a positional.
    pub display: String,
    pub nargs: Nargs,
    pub positional: bool,
}

/// A command of papers: its clap definition and what to report its errors.
pub struct Spec {
    pub command: Command,
    /// By id of the argument.
    pub info: HashMap<String, ArgInfo>,
    /// The long options (and their aliases) in the order of the definition,
    /// for the error of an abbreviation that fits several.
    pub long_options: Vec<String>,
    /// The rendering of an argument in clap's errors, to the id.
    pub rendered: HashMap<String, String>,
}

/// A value restricted to choices, with argparse's message.
fn choice_parser(choices: &'static [&'static str]) -> ValueParser {
    ValueParser::new(
        move |value: &str| -> Result<String, String> {
            if choices.contains(&value) {
                Ok(value.to_string())
            } else {
                let shown: Vec<String> = choices.iter().map(|c| format!("'{c}'")).collect();
                Err(format!("invalid choice: {} (choose from {})", papers_rs::config::json::py_repr_str(value), shown.join(", ")))
            }
        },
    )
}

fn int_parser() -> ValueParser {
    ValueParser::new(move |value: &str| -> Result<String, String> {
        match papers_rs::config::settings::py_int(value) {
            Some(_) => Ok(value.to_string()),
            None => Err(format!("invalid int value: {}", papers_rs::config::json::py_repr_str(value))),
        }
    })
}

/// `FIELD=VALUE`
fn field_value_parser() -> ValueParser {
    ValueParser::new(move |value: &str| -> Result<String, String> {
        match value.split_once('=') {
            Some((name, _)) if !name.trim().is_empty() => Ok(value.to_string()),
            _ => Err(format!("{}: FIELD=VALUE expected", papers_rs::config::json::py_repr_str(value))),
        }
    })
}

/// `-f` or `--format`, as a clap short or long.
enum Name {
    Short(char),
    Long(String),
}

fn names_of(names: &[&str]) -> Vec<Name> {
    names
        .iter()
        .map(|n| {
            if let Some(long) = n.strip_prefix("--") {
                Name::Long(long.to_string())
            } else {
                Name::Short(n.trim_start_matches('-').chars().next().expect("a short name"))
            }
        })
        .collect()
}

pub struct Builder {
    command: Command,
    info: HashMap<String, ArgInfo>,
    long_options: Vec<String>,
}

impl Builder {
    pub fn new(prog: &str) -> Builder {
        let command = Command::new(prog.to_string())
            .no_binary_name(true)
            .disable_help_flag(true)
            .disable_version_flag(true)
            .disable_help_subcommand(true)
            .infer_long_args(true)
            .allow_negative_numbers(true);
        let mut builder = Builder { command, info: HashMap::new(), long_options: Vec::new() };
        builder.flag("help", &["-h", "--help"]);
        builder
    }

    /// Without negative numbers as values (`-1` is an option).
    pub fn numbers_are_options(mut self) -> Builder {
        self.command = self.command.allow_negative_numbers(false);
        self
    }

    fn add(&mut self, id: &str, mut arg: Arg, names: &[&str], nargs: Nargs, positional: bool, display: Option<String>) {
        if !positional {
            let mut long_seen = false;
            let mut short_seen = false;
            for name in names_of(names) {
                match name {
                    Name::Long(l) if !long_seen => {
                        arg = arg.long(l);
                        long_seen = true;
                    }
                    Name::Long(l) => arg = arg.alias(l),
                    Name::Short(s) if !short_seen => {
                        arg = arg.short(s);
                        short_seen = true;
                    }
                    Name::Short(s) => arg = arg.short_alias(s),
                }
            }
        }
        if !positional && !matches!(arg.get_action(), ArgAction::Append) {
            // a repeated option: the last one wins, as in argparse
            arg = arg.overrides_with(id.to_string());
        }
        for name in names {
            if let Some(long) = name.strip_prefix("--") {
                self.long_options.push(format!("--{long}"));
            }
        }
        let display = display.unwrap_or_else(|| names.join("/"));
        self.info.insert(id.to_string(), ArgInfo { display, nargs, positional });
        self.command = self.command.clone().arg(arg);
    }

    /// A flag (`store_true`).
    pub fn flag(&mut self, id: &str, names: &[&str]) -> &mut Builder {
        let arg = Arg::new(id.to_string()).action(ArgAction::SetTrue);
        self.add(id, arg, names, Nargs::Flag, false, None);
        self
    }

    /// An option with a value (a string).
    pub fn opt(&mut self, id: &str, names: &[&str], nargs: Nargs) -> &mut Builder {
        let arg = Arg::new(id.to_string()).action(ArgAction::Set);
        let arg = Self::with_nargs(arg, nargs);
        self.add(id, arg, names, nargs, false, None);
        self
    }

    /// An option with a value that can be repeated (`action='append'`).
    pub fn append(&mut self, id: &str, names: &[&str], nargs: Nargs) -> &mut Builder {
        let arg = Arg::new(id.to_string()).action(ArgAction::Append);
        let arg = Self::with_nargs(arg, nargs);
        self.add(id, arg, names, nargs, false, None);
        self
    }

    /// An option with one of the choices as value.
    pub fn choice(&mut self, id: &str, names: &[&str], choices: &'static [&'static str]) -> &mut Builder {
        let arg = Arg::new(id.to_string()).action(ArgAction::Set).num_args(1).value_parser(choice_parser(choices));
        self.add(id, arg, names, Nargs::One, false, None);
        self
    }

    /// An option with an integer as value.
    pub fn int(&mut self, id: &str, names: &[&str]) -> &mut Builder {
        let arg = Arg::new(id.to_string()).action(ArgAction::Set).num_args(1).value_parser(int_parser());
        self.add(id, arg, names, Nargs::One, false, None);
        self
    }

    /// An option with values of the form `FIELD=VALUE`.
    pub fn fields(&mut self, id: &str, names: &[&str]) -> &mut Builder {
        let arg = Arg::new(id.to_string()).action(ArgAction::Set).num_args(1..).value_parser(field_value_parser());
        self.add(id, arg, names, Nargs::Plus, false, None);
        self
    }

    /// A positional argument.
    pub fn pos(&mut self, id: &str, metavar: &str, nargs: Nargs) -> &mut Builder {
        let required = matches!(nargs, Nargs::One | Nargs::Plus);
        let action = if matches!(nargs, Nargs::One | Nargs::Opt) { ArgAction::Set } else { ArgAction::Append };
        let arg = Self::with_nargs(Arg::new(id.to_string()).action(action), nargs)
            .value_name(metavar.to_string())
            .required(required);
        self.add(id, arg, &[], nargs, true, Some(metavar.to_string()));
        self
    }

    /// A positional argument that takes the rest of the command line
    /// (argparse's `REMAINDER`).
    pub fn remainder(&mut self, id: &str, metavar: &str) -> &mut Builder {
        let arg = Arg::new(id.to_string())
            .action(ArgAction::Append)
            .num_args(0..)
            .allow_hyphen_values(true)
            .trailing_var_arg(true)
            .value_name(metavar.to_string());
        self.add(id, arg, &[], Nargs::Star, true, Some(metavar.to_string()));
        self
    }

    fn with_nargs(arg: Arg, nargs: Nargs) -> Arg {
        match nargs {
            Nargs::Flag => arg.num_args(0),
            Nargs::One => arg.num_args(1),
            Nargs::Opt => arg.num_args(0..=1),
            Nargs::Star => arg.num_args(0..),
            Nargs::Plus => arg.num_args(1..),
            Nargs::Exact(n) => arg.num_args(n),
        }
    }

    /// `--name` and `--no-name` (argparse's `BooleanOptionalAction`): the last
    /// one wins.
    pub fn boolean(&mut self, id: &str, name: &str) -> &mut Builder {
        let negative = format!("{id}-no");
        let on = Arg::new(id.to_string()).action(ArgAction::SetTrue).overrides_with(negative.clone());
        let off = Arg::new(negative.clone()).action(ArgAction::SetTrue).overrides_with(id.to_string());
        let on_name = format!("--{name}");
        let off_name = format!("--no-{name}");
        let display = format!("{on_name}/{off_name}");
        self.add(id, on, &[&on_name], Nargs::Flag, false, Some(display.clone()));
        self.add(&negative, off, &[&off_name], Nargs::Flag, false, Some(display));
        self
    }

    /// The arguments that exclude one another (a mutually exclusive group).
    pub fn exclusive(&mut self, ids: &[&str]) -> &mut Builder {
        for id in ids {
            let others: Vec<String> = ids.iter().filter(|o| *o != id).map(|o| o.to_string()).collect();
            let id = id.to_string();
            self.command = self.command.clone().mut_arg(id, |arg| arg.conflicts_with_all(others));
        }
        self
    }

    /// Without a mention in the help (argparse's `SUPPRESS`).
    pub fn hide(&mut self, ids: &[&str]) -> &mut Builder {
        for id in ids {
            self.command = self.command.clone().mut_arg(*id, |arg| arg.hide(true));
        }
        self
    }

    pub fn finish(self) -> Spec {
        // how clap renders each argument in its errors, to find the argument again
        let mut built = self.command.clone();
        built.build();
        let rendered = built.get_arguments().map(|a| (a.to_string(), a.get_id().to_string())).collect();
        Spec { command: self.command, info: self.info, long_options: self.long_options, rendered }
    }
}
