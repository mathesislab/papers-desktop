//! The usage and the help of the commands, as papers prints them: recorded
//! from papers-cli (see `tests/record/record_help.py`), then the options
//! whose help shows a configured value (`--filesdir`, the name and key formats,
//! the global configuration) are written again with the values of the
//! configuration, wrapped as argparse wraps them.

use std::collections::BTreeMap;

use papers_rs::config::json::py_str;
use papers_rs::config::{Config, settings};

use crate::help_text::HELP;
use crate::textwrap::wrap;

fn entry(path: &str) -> &'static (&'static str, &'static str, &'static str) {
    HELP.iter().find(|(name, _, _)| *name == path).unwrap_or_else(|| panic!("no help for {path:?}"))
}

/// `usage: papers init [-h] ...` (ends with a newline).
pub fn usage(path: &str) -> String {
    entry(path).1.to_string()
}

/// The values that the help of the options shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelpValues {
    pub filesdir: Option<String>,
    pub bibtex: Option<String>,
    /// The global configuration file.
    pub machine_config: String,
    /// `paths` and the `name.*` and `key.*` settings, as Python prints them.
    pub settings: BTreeMap<String, String>,
}

impl HelpValues {
    /// The values of the help as recorded: no library, an empty global
    /// configuration, `HOME` at `/home/papers-help`.
    pub fn recorded() -> HelpValues {
        let config = Config::new(papers_rs::config::MachineConfig::empty());
        let mut values = HelpValues::from_config(&config);
        values.machine_config = "/home/papers-help/Library/Application Support/papers/config.json".to_string();
        values
    }

    pub fn from_config(config: &Config) -> HelpValues {
        let mut values = BTreeMap::new();
        values.insert("paths".to_string(), py_str(&config.inherited("paths")));
        for name in settings::names() {
            if name.starts_with("name.") || name.starts_with("key.") {
                values.insert(name.to_string(), py_str(&config.get(name)));
            }
        }
        HelpValues {
            filesdir: config.filesdir.clone(),
            bibtex: config.bibtex.clone(),
            machine_config: papers_rs::config::dirs::machine_config_file(),
            settings: values,
        }
    }
}

/// The help of the options that show a configured value: the invocation and the help.
fn dynamic_entries(v: &HelpValues) -> Vec<(String, String)> {
    let shown = |value: &Option<String>| value.clone().unwrap_or_else(|| "None".to_string());
    let mut entries = vec![
        ("--filesdir FILESDIR".to_string(), format!("files directory (default: {})", shown(&v.filesdir))),
        ("--bibtex BIBTEX".to_string(), format!("bibtex database (default: {})", shown(&v.bibtex))),
        (
            "--paths {relative,absolute}".to_string(),
            format!(
                "file links in the bibtex relative to the bibtex directory, or absolute (default: {})",
                v.settings["paths"]
            ),
        ),
        (
            "--global".to_string(),
            format!(
                "the global configuration, per user ({}), instead of the library's: the default library, and the default settings of the libraries",
                v.machine_config
            ),
        ),
    ];
    for (kind, what) in [("name", "filename"), ("key", "key")] {
        for field in ["template", "author-num", "author-sep", "title-word-num", "title-word-size", "title-length", "title-sep", "unknown-template", "unknown-strict"] {
            let default = &v.settings[&format!("{kind}.{field}")];
            let option = format!("--{kind}-{field}");
            let invocation = if field == "unknown-strict" {
                format!("{option}, --no-{kind}-{field}")
            } else {
                format!("{option} {}", option[2..].replace('-', "_").to_uppercase())
            };
            let help = match (kind, field) {
                ("name", "template") => format!("python template for renaming files (default: {default})"),
                ("key", "template") => format!(
                    "python template for generating keys (default: {default}). The template applies to entries generated from a PDF or a DOI; entries added from a bibtex file keep their original key unless -u/--update-key is passed."
                ),
                _ => {
                    let text = match field {
                        "author-num" => format!("number of authors in the {what} (not for {{authorX}})"),
                        "author-sep" => format!("separator of the authors in the {what}"),
                        "title-word-num" => format!("maximum number of title words in the {what}"),
                        "title-word-size" => format!("minimum length of a title word: shorter words are left out of the {what}"),
                        "title-length" => format!("maximum length of the title in the {what} (the last words are dropped until it fits; a longer first word is cut)"),
                        "title-sep" => format!("separator of the title words in the {what}"),
                        "unknown-template" => format!("template of the {what} of an entry without author, year or title"),
                        _ => "the unknown template applies when the author, the year or the title is missing; with this option, only when all three are missing".to_string(),
                    };
                    format!("{text} (default: {default})")
                }
            };
            entries.push((invocation, help));
        }
    }
    entries
}

/// An option of the help as argparse writes it: the invocation, and its help
/// wrapped at the column 24 (on the line of the invocation if that is short).
fn render_entry(invocation: &str, help: &str) -> String {
    const HELP_POSITION: usize = 24;
    const WIDTH: usize = 78;
    let normalised = help.split_whitespace().collect::<Vec<_>>().join(" ");
    let lines = wrap(&normalised, WIDTH - HELP_POSITION);
    let mut out = String::new();
    let action_width = HELP_POSITION - 2 - 2;
    let pad = " ".repeat(HELP_POSITION);
    let mut lines = lines.into_iter();
    if invocation.chars().count() <= action_width {
        out.push_str(&format!("  {invocation:<action_width$}  {}\n", lines.next().unwrap_or_default()));
    } else {
        out.push_str(&format!("  {invocation}\n{pad}{}\n", lines.next().unwrap_or_default()));
    }
    for line in lines {
        out.push_str(&format!("{pad}{line}\n"));
    }
    out
}

/// The help of a command (`--help`) with the values of the configuration.
pub fn help(path: &str, values: &HelpValues) -> String {
    let mut text = entry(path).2.to_string();
    let recorded = HelpValues::recorded();
    if *values == recorded {
        return text;
    }
    for ((invocation, recorded_help), (_, help)) in dynamic_entries(&recorded).into_iter().zip(dynamic_entries(values)) {
        let before = render_entry(&invocation, &recorded_help);
        if text.contains(&before) {
            text = text.replace(&before, &render_entry(&invocation, &help));
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block_of(path: &str, invocation: &str) -> bool {
        let recorded = HelpValues::recorded();
        let (_, help) = dynamic_entries(&recorded).into_iter().find(|(i, _)| i == invocation).expect("an entry");
        entry(path).2.contains(&render_entry(invocation, &help))
    }

    #[test]
    fn the_entries_are_rendered_as_argparse_renders_them() {
        // each is found as written in the recorded help of papers
        for invocation in ["--filesdir FILESDIR", "--bibtex BIBTEX"] {
            assert!(block_of("rm", invocation), "rm {invocation}");
            assert!(block_of("list", invocation), "list {invocation}");
        }
        assert!(block_of("config", "--global"));
        assert!(block_of("init", "--paths {relative,absolute}"));
        for kind in ["name", "key"] {
            for field in ["template", "author-num", "author-sep", "title-word-num", "title-word-size", "title-length", "title-sep", "unknown-template"] {
                let invocation = format!("--{kind}-{field} {}", format!("{kind}-{field}").replace('-', "_").to_uppercase());
                assert!(block_of("init", &invocation), "init {invocation}");
                assert!(block_of("add", &invocation), "add {invocation}");
            }
            assert!(block_of("init", &format!("--{kind}-unknown-strict, --no-{kind}-unknown-strict")));
        }
    }

    /// The help recorded in a library with a global configuration that sets formats.
    #[test]
    fn the_values_of_the_configuration_are_written_again() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!("../tests/fixtures/help.json")).unwrap();
        let v = &fixture["values"];
        let mut settings: BTreeMap<String, String> =
            v["settings"].as_object().unwrap().iter().map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string())).collect();
        settings.insert("paths".to_string(), v["paths"].as_str().unwrap().to_string());
        let values = HelpValues {
            filesdir: Some(v["filesdir"].as_str().unwrap().to_string()),
            bibtex: Some(v["bibtex"].as_str().unwrap().to_string()),
            machine_config: v["machine_config"].as_str().unwrap().to_string(),
            settings,
        };
        for case in fixture["cases"].as_array().unwrap() {
            let path = case["path"].as_str().unwrap();
            let recorded = case["text"].as_str().unwrap();
            assert_eq!(help(path, &values), recorded, "papers {path} -h");
        }
    }
}
