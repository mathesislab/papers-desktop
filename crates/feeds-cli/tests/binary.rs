//! The program itself, with no network: the folder, the exit statuses, the help.

use std::path::Path;
use std::process::{Command, Output};

fn feeds(args: &[&str], env: &[(&str, &Path)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_feeds"));
    command.args(args).env_clear();
    for (name, value) in env {
        command.env(name, value);
    }
    command.output().unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn the_folder_is_the_option_else_feeds_dir_else_the_data_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let (by_env, by_option, home) = (
        tmp.path().join("env"),
        tmp.path().join("option"),
        tmp.path().join("home"),
    );
    let url = "https://example.org/feed.xml";
    let said = feeds(&["subscribe", url], &[("FEEDS_DIR", &by_env)]);
    assert_eq!(said.status.code(), Some(0), "{}", text(&said.stderr));
    assert!(by_env.join("config.json").is_file());
    let listed = feeds(&["subscriptions"], &[("FEEDS_DIR", &by_env)]);
    assert!(text(&listed.stdout).ends_with(&format!("{url}\n")));
    // the option wins
    feeds(
        &["--dir", by_option.to_str().unwrap(), "subscriptions"],
        &[("FEEDS_DIR", &by_env)],
    );
    assert!(by_option.join("lookups.json").is_file());
    assert_eq!(
        text(
            &feeds(
                &["--dir", by_option.to_str().unwrap(), "subscriptions"],
                &[]
            )
            .stdout
        ),
        ""
    );
    // neither: papers-feeds/ in the data folder of the platform
    let data = if cfg!(target_os = "macos") {
        home.join("Library/Application Support")
    } else {
        tmp.path().join("data")
    };
    let config = feeds(
        &["config"],
        &[("HOME", &home), ("XDG_DATA_HOME", &tmp.path().join("data"))],
    );
    assert_eq!(config.status.code(), Some(0), "{}", text(&config.stderr));
    assert!(text(&config.stdout).starts_with(&format!(
        "folder        {}\n",
        data.join("papers-feeds").display()
    )));
    assert!(data.join("papers-feeds").is_dir());
}

#[test]
fn exit_statuses_and_messages() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("feeds");
    let env = [("FEEDS_DIR", dir.as_path())];
    let missing = feeds(&["unsubscribe", "nope"], &env);
    assert_eq!(missing.status.code(), Some(1));
    assert_eq!(
        text(&missing.stderr),
        "feeds: error: nope: no such subscription\n"
    );
    assert_eq!(text(&missing.stdout), "");
    let usage = feeds(&[], &env);
    assert_eq!(usage.status.code(), Some(2));
    assert!(text(&usage.stderr).contains("Usage: feeds"));
    // nothing to fetch: nothing asked of the network
    let fetched = feeds(&["fetch"], &env);
    assert_eq!(text(&fetched.stdout), "new: 0; lookups: 0; pages: 0\n");
}

#[test]
fn the_help_names_the_commands_and_the_folder() {
    let help = feeds(&["--help"], &[]);
    assert_eq!(help.status.code(), Some(0));
    let out = text(&help.stdout);
    for command in [
        "subscribe",
        "fetch",
        "list",
        "show",
        "find",
        "mark",
        "prune",
        "interests",
        "rules",
        "config",
    ] {
        assert!(out.contains(&format!("\n  {command} ")), "{command}\n{out}");
    }
    assert!(
        out.contains("FEEDS_DIR") && out.contains("nothing locks it"),
        "{out}"
    );
    assert_eq!(
        text(&feeds(&["--version"], &[]).stdout),
        format!("feeds {}\n", env!("CARGO_PKG_VERSION"))
    );
}
