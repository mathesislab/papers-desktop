//! `papers-rs COMMAND -h` prints the help of papers (recorded by
//! tests/record/record_help.py), and the errors of a command line print the
//! usage of the command.

#[path = "../src/help_text.rs"]
mod help_text;
#[path = "fixtures/help_linux.rs"]
mod help_linux;

use std::process::Command;

fn papers(args: &[&str], dir: &std::path::Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_papers-rs"))
        .args(args)
        .current_dir(dir)
        .env_clear()
        .env("HOME", "/home/papers-help")
        .output()
        .unwrap()
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
        // on Linux, papers' help shows the path of the global configuration there
        let linux = help_linux::HELP_LINUX.iter().find(|(name, _)| name == path).map(|(_, text)| *text);
        let text = match linux {
            Some(linux) if cfg!(target_os = "linux") => linux,
            _ => *text,
        };
        assert_eq!(stdout, text, "papers {path} -h");
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
        assert_eq!(stderr, format!("{usage}{prog}: error: unrecognized arguments: --no-such-option\n"));
    }
}
