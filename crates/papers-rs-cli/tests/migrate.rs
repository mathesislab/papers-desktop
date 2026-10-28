//! `papers migrate`: the global configuration of papers v2 becomes a library (the expected texts are
//! what papers-cli prints and writes for the same files).

use std::fs;
use std::path::Path;
use std::process::Command;

fn user_config_dir(home: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("{home}/Library/Application Support")
    } else {
        format!("{home}/.config")
    }
}

struct Sandbox {
    _tmp: tempfile::TempDir,
    root: String,
}

impl Sandbox {
    fn new(config: &str) -> Sandbox {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let s = Sandbox { _tmp: tmp, root };
        s.write("lib/refs.bib", &format!("@article{{a2000,\n author = {{Author, A.}},\n file = {{:{}/lib/pdfs/a.pdf:pdf}},\n title = {{One}},\n year = {{2000}}\n}}\n", s.root));
        s.write("lib/pdfs/a.pdf", "pdf\n");
        let v2 = format!("{}/papersconfig.json", user_config_dir(&s.path("home")));
        s.write_abs(&v2, &config.replace("$ROOT", &s.root));
        s
    }

    fn path(&self, rel: &str) -> String {
        format!("{}/{rel}", self.root)
    }

    fn write(&self, rel: &str, text: &str) {
        self.write_abs(&self.path(rel), text);
    }

    fn write_abs(&self, path: &str, text: &str) {
        fs::create_dir_all(Path::new(path).parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn papers(&self, args: &[&str]) -> (String, String, i32) {
        let out = Command::new(env!("CARGO_BIN_EXE_papers-rs"))
            .args(args)
            .current_dir(self.path("work"))
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", self.path("home"))
            .env("LANG", "C.UTF-8")
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        let text = |b: &[u8]| String::from_utf8_lossy(b).replace(&self.root, "$ROOT");
        (
            text(&out.stdout),
            text(&out.stderr),
            out.status.code().unwrap_or(-1),
        )
    }
}

const V2: &str = r#"{
  "bibtex": "$ROOT/lib/refs.bib",
  "filesdir": "$ROOT/lib/pdfs",
  "absolute_paths": true,
  "backup_files": true,
  "local": false
}"#;

#[test]
fn the_global_configuration_becomes_the_default_library() {
    let s = Sandbox::new(V2);
    fs::create_dir_all(s.path("work")).unwrap();
    let v2 = format!("{}/papersconfig.json", user_config_dir(&s.path("home")));
    let shown = v2.replace(&s.root, "$ROOT");
    let (out, _, code) = s.papers(&["migrate", "--dry-run"]);
    assert_eq!(code, 0);
    assert_eq!(
        out,
        format!(
            "global configuration of papers v2: {shown}\n  DRYRUN: create the library configuration $ROOT/lib/.papers/config.json (paths: absolute)\n  DRYRUN: register $ROOT/lib as the default library in {}/papers/config.json\n  DRYRUN: rename {shown} to {shown}.v2.bak\n",
            user_config_dir(&s.path("home")).replace(&s.root, "$ROOT")
        )
    );
    assert!(Path::new(&v2).exists());
    assert!(!Path::new(&s.path("lib/.papers/config.json")).exists());

    let (out, _, code) = s.papers(&["migrate"]);
    assert_eq!(code, 0);
    assert!(out.contains(
        "  create the library configuration $ROOT/lib/.papers/config.json (paths: absolute)\n"
    ));
    assert!(!Path::new(&v2).exists());
    assert!(Path::new(&format!("{v2}.v2.bak")).exists());
    assert_eq!(
        fs::read_to_string(s.path("lib/.papers/config.json")).unwrap(),
        "{\n  \"bibtex\": \"refs.bib\",\n  \"filesdir\": \"pdfs\",\n  \"paths\": \"absolute\",\n  \"pool\": \"none\",\n  \"version\": 3\n}"
    );
    let machine = fs::read_to_string(format!(
        "{}/papers/config.json",
        user_config_dir(&s.path("home"))
    ))
    .unwrap();
    assert_eq!(
        machine,
        format!(
            "{{\n  \"default\": \"{}/lib\",\n  \"version\": 3\n}}",
            s.root
        )
    );
    // it is run once
    let (out, _, code) = s.papers(&["migrate"]);
    assert_eq!(code, 0);
    assert_eq!(
        out,
        "nothing to migrate: no global configuration of papers v2 was found, and the library configuration is already .papers/config.json\n"
    );
    // and the library is found from anywhere
    let (out, _, code) = s.papers(&["list", "-K"]);
    assert_eq!((out.as_str(), code), ("a2000\n", 0));
}

#[test]
fn relative_converts_the_file_links_and_a_default_library_is_kept() {
    let s = Sandbox::new(V2);
    fs::create_dir_all(s.path("work")).unwrap();
    s.write(
        &format!(
            "home/{}/papers/config.json",
            user_config_dir("").trim_start_matches('/')
        ),
        "{\n  \"default\": \"/elsewhere\",\n  \"version\": 3\n}",
    );
    let (out, _, code) = s.papers(&["migrate", "--relative"]);
    assert_eq!(code, 0);
    assert!(out.contains(
        "  create the library configuration $ROOT/lib/.papers/config.json (paths: relative)\n"
    ));
    assert!(
        out.contains("  the file links of $ROOT/lib/refs.bib are now relative to its directory\n")
    );
    assert!(out.contains("  the default library is already /elsewhere: not changed (change it with `papers config --global default PATH`)\n"));
    let bib = fs::read_to_string(s.path("lib/refs.bib")).unwrap();
    assert!(bib.contains("file = {:pdfs/a.pdf:pdf}"), "{bib}");
}

#[test]
fn a_configuration_without_bibtex_creates_no_library() {
    let s = Sandbox::new("{\n  \"local\": false\n}");
    fs::create_dir_all(s.path("work")).unwrap();
    let (out, _, code) = s.papers(&["migrate"]);
    assert_eq!(code, 0);
    assert!(
        out.contains("  it defines no bibtex: no library to create\n"),
        "{out}"
    );
    assert!(!Path::new(&s.path("lib/.papers")).exists());
}
