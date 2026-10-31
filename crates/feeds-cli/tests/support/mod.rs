//! A folder in a temporary directory, the program run on it with recorded replies.

#![allow(dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{TimeZone, Utc};
use feeds_cli::Host;
use papers_feeds::Reply;
use papers_feeds::http::Recorded;

/// What a run printed, and its exit status.
#[derive(Debug)]
pub struct Ran {
    pub code: i32,
    pub out: String,
    pub err: String,
}

/// A feeds folder in a temporary directory, with the replies the program gets.
pub struct Sandbox {
    pub tmp: tempfile::TempDir,
    pub http: Recorded,
}

/// The bytes of a fixture of the feeds crate.
pub fn fixture(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/../feeds/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(path).unwrap()
}

/// The bytes of a fixture of this crate.
pub fn own(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// A reply of status 200 to its own URL.
pub fn ok(url: &str, body: impl Into<Vec<u8>>) -> Reply {
    Reply::ok(url, body)
}

impl Sandbox {
    pub fn new(http: Recorded) -> Sandbox {
        Sandbox {
            tmp: tempfile::tempdir().unwrap(),
            http,
        }
    }

    /// The folder of the feeds.
    pub fn dir(&self) -> PathBuf {
        self.tmp.path().join("feeds")
    }

    /// A path in the temporary directory.
    pub fn path(&self, name: &str) -> PathBuf {
        self.tmp.path().join(name)
    }

    /// Writes `config.json` of the folder.
    pub fn config(&self, json: &str) {
        std::fs::create_dir_all(self.dir()).unwrap();
        std::fs::write(self.dir().join("config.json"), json).unwrap();
    }

    /// Runs `feeds --dir <folder> ARGS`, the clock at 2026-10-31 08:00 UTC.
    pub fn run(&self, args: &[&str]) -> Ran {
        self.run_env(args, &[])
    }

    /// [`Sandbox::run`] with environment variables.
    pub fn run_env(&self, args: &[&str], env: &[(&str, &str)]) -> Ran {
        let dir = self.dir();
        let mut argv = vec![
            "feeds".to_string(),
            "--dir".to_string(),
            dir.display().to_string(),
        ];
        argv.extend(args.iter().map(|a| a.to_string()));
        let vars: HashMap<String, String> = env
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let host = Host {
            env: Box::new(move |name| vars.get(name).cloned()),
            now: Arc::new(|| Utc.with_ymd_and_hms(2026, 10, 31, 8, 0, 0).unwrap()),
            http: Some(&self.http),
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = feeds_cli::run(argv, &host, &mut out, &mut err);
        Ran {
            code,
            out: String::from_utf8(out).unwrap(),
            err: String::from_utf8(err).unwrap(),
        }
    }

    /// A run that must have succeeded: what it printed.
    pub fn ok(&self, args: &[&str]) -> String {
        let ran = self.run(args);
        assert_eq!((ran.code, ran.err.as_str()), (0, ""), "feeds {args:?}");
        ran.out
    }

    /// The JSON a command printed.
    pub fn json(&self, args: &[&str]) -> serde_json::Value {
        serde_json::from_str(&self.ok(args)).unwrap()
    }
}
