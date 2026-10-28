//! `papers-rs COMMAND -h` prints the help of papers (recorded by
//! tests/record/record_help.py), and the errors of a command line print the
//! usage of the command.

#[path = "../src/help_text.rs"]
mod help_text;

use std::process::Command;

#[path = "common/openalex.rs"]
mod openalex_option;

fn papers(args: &[&str], dir: &std::path::Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_papers-rs"))
        .args(args)
        .current_dir(dir)
        .env_clear()
        .env("HOME", "/home/papers-help")
        .output()
        .unwrap()
}

/// The extension of papers-rs: `--openalex` where papers has `--scholar` (not in papers). The help
/// of these commands is what argparse prints with the option added to papers' parser
/// (`tests/fixtures/help-openalex`, made by `tests/record/record_help_openalex.py`).
const OPENALEX: [(&str, &str); 3] = [
    ("add", include_str!("fixtures/help-openalex/add.txt")),
    ("fetch", include_str!("fixtures/help-openalex/fetch.txt")),
    ("extract", include_str!("fixtures/help-openalex/extract.txt")),
];

fn openalex(path: &str) -> Option<&'static str> {
    OPENALEX.iter().find(|(name, _)| *name == path).map(|(_, text)| *text)
}

#[test]
fn help_of_every_command() {
    let dir = tempfile::tempdir().unwrap();
    for (path, usage, text) in help_text::HELP {
        let mut args: Vec<&str> = path.split_whitespace().collect();
        args.push("-h");
        let output = papers(&args, dir.path());
        assert_eq!(output.status.code(), Some(0), "papers {path} -h");
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        match openalex(path) {
            None => assert_eq!(stdout, *text, "papers {path} -h"),
            Some(extended) => {
                assert_eq!(stdout, extended, "papers {path} -h");
                // it is the help of papers with the option, and nothing else
                assert_eq!(openalex_option::without_openalex(&stdout).as_deref(), Some(openalex_option::words(text).as_str()), "papers {path} -h");
            }
        }
        assert!(text.starts_with(usage));
        assert!(output.stderr.is_empty());
    }
}

/// The usage of a command is printed with the errors of its command line.
#[test]
fn usage_with_an_error() {
    let dir = tempfile::tempdir().unwrap();
    for (path, usage, _) in help_text::HELP {
        // `git` takes the rest of the command line; the others lack arguments that they require
        let required = ["file attach", "file mv", "edit", "rm", "export", "open", "doi", "fetch", "extract"];
        if path.is_empty() || *path == "git" || required.contains(path) {
            continue;
        }
        let mut args: Vec<&str> = path.split_whitespace().collect();
        args.push("--no-such-option");
        let output = papers(&args, dir.path());
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let prog = format!("papers {path}");
        assert_eq!(output.status.code(), Some(2), "{prog}: {stderr}");
        // the usage of the commands with `--openalex` is the first paragraph of their help
        let usage = match openalex(path) {
            Some(extended) => format!("{}\n", extended.split("\n\n").next().unwrap()),
            None => usage.to_string(),
        };
        assert_eq!(stderr, format!("{usage}{prog}: error: unrecognized arguments: --no-such-option\n"));
    }
}
