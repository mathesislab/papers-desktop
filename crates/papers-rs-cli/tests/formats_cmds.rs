//! The commands of the formats of papers 3.0.0b6 (`formats.md`), through the binary: `list --field`
//! and `--porcelain`, the export message, `edit --from FILE`, `add` of RIS, CSL-JSON and BibJSON. Ported
//! from `tests/test_list_more.py`, `tests/test_porcelain.py`, `tests/test_formats.py` and
//! `tests/test_spec_formats.py` of papers; the reference can be run on the same cases:
//!
//! ```text
//! PAPERS_REFERENCE=papers cargo test -p papers-rs-cli --test formats_cmds -- --ignored
//! ```

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

const RS: &str = env!("CARGO_BIN_EXE_papers-rs");

const START: &str = "@article{one2000,\n author = {Doe, Jane},\n doi = {10.1000/one},\n file = {:files/one.txt:txt},\n title = {One},\n year = {2000}\n}\n\n@article{two2000,\n author = {Roe, R},\n title = {Two},\n year = {2000}\n}\n";

struct Sandbox {
    _tmp: tempfile::TempDir,
    root: String,
}

impl Sandbox {
    /// A library `lib` (bibtex `bibtex`) in a sandbox with a home of its own.
    fn new(bibtex: &str) -> Sandbox {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let s = Sandbox { _tmp: tmp, root };
        s.write(
            "lib/.papers/config.json",
            &format!("{{\n  \"bibtex\": \"{bibtex}\",\n  \"filesdir\": \"files\",\n  \"pool\": \"none\",\n  \"version\": 3\n}}"),
        );
        s.write(&format!("lib/{bibtex}"), START);
        s.write("lib/files/one.txt", "content\n");
        s
    }

    fn path(&self, rel: &str) -> String {
        format!("{}/{rel}", self.root)
    }

    fn write(&self, rel: &str, text: &str) {
        let path = self.path(rel);
        fs::create_dir_all(Path::new(&path).parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.path(rel)).unwrap_or_default()
    }

    fn output_in(&self, program: &str, dir: &str, args: &[&str], stdin: &str) -> Output {
        use std::io::Write;
        let mut words = program.split_whitespace();
        let mut child = Command::new(words.next().unwrap())
            .args(words)
            .args(args)
            .current_dir(self.path(dir))
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", self.path("home"))
            .env("LANG", "C.UTF-8")
            .env("LC_ALL", "C.UTF-8")
            .env("TZ", "UTC")
            .env("NO_COLOR", "1")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    /// (exit code, stdout, stderr) of a command in the library.
    fn run(&self, program: &str, args: &[&str]) -> (i32, String, String) {
        let output = self.output_in(program, "lib", args, "");
        (
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    }
}

/// The program to test: papers-rs, or the reference with `--ignored`.
fn program(reference: bool) -> String {
    if reference {
        std::env::var("PAPERS_REFERENCE").unwrap_or_else(|_| "papers".to_string())
    } else {
        RS.to_string()
    }
}

fn the_field_listing(reference: bool) {
    let s = Sandbox::new("papers.bib");
    let p = program(reference);
    // without the key: the values only, no leading space
    let (code, out, _) = s.run(
        &p,
        &["list", "one2000", "--field", "doi", "year", "--no-key"],
    );
    assert_eq!((code, out.as_str()), (0, "10.1000/one 2000\n"));
    let (_, out, _) = s.run(&p, &["list", "one2000", "--field", "doi", "year"]);
    assert_eq!(out, "one2000: 10.1000/one 2000\n");
    // a missing field is the empty string
    let (_, out, _) = s.run(&p, &["list", "--field", "doi", "year"]);
    assert_eq!(out, "one2000: 10.1000/one 2000\ntwo2000:  2000\n");
    let (_, out, _) = s.run(&p, &["list", "--field", "doi", "year", "--no-key"]);
    assert_eq!(out, "10.1000/one 2000\n 2000\n");
    // the line format keeps the space of the empty key
    let (_, out, _) = s.run(&p, &["list", "two2000", "--no-key"]);
    assert_eq!(out, " Two \n");
}

#[test]
fn field_listing() {
    the_field_listing(false);
}

#[test]
#[ignore = "runs the reference (PAPERS_REFERENCE)"]
fn field_listing_as_papers() {
    the_field_listing(true);
}

fn the_listing_with_porcelain(reference: bool) {
    let s = Sandbox::new("papers.bib");
    let p = program(reference);
    // the listing is the data of the command: on the standard output, with --porcelain too
    let (code, out, err) = s.run(&p, &["list", "-K", "--porcelain"]);
    assert_eq!(
        (code, out.as_str(), err.as_str()),
        (0, "one2000\ntwo2000\n", "")
    );
    let (_, out, _) = s.run(&p, &["list", "--field", "year", "--porcelain"]);
    assert_eq!(out, "one2000: 2000\ntwo2000: 2000\n");
    let (_, out, _) = s.run(&p, &["list", "two2000", "--porcelain"]);
    assert!(out.starts_with("two2000: Two"), "{out:?}");
    let (_, out, _) = s.run(
        &p,
        &["list", "two2000", "--format", "bibtex", "--porcelain"],
    );
    assert!(out.starts_with("@article{two2000,"), "{out:?}");
    let (_, out, _) = s.run(&p, &["list", "two2000", "--format", "ris", "--porcelain"]);
    assert!(out.starts_with("TY  - JOUR\nID  - two2000\n"), "{out:?}");
    // a list action prints the porcelain lines of its command
    let (_, out, _) = s.run(&p, &["list", "one2000", "--add-tag", "x", "--porcelain"]);
    assert_eq!(out, "modified one2000\n");
}

#[test]
fn listing_with_porcelain() {
    the_listing_with_porcelain(false);
}

#[test]
#[ignore = "runs the reference (PAPERS_REFERENCE)"]
fn listing_with_porcelain_as_papers() {
    the_listing_with_porcelain(true);
}

fn the_export_message_names_the_bibtex(reference: bool) {
    let s = Sandbox::new("refs.bib");
    let p = program(reference);
    let (code, out, _) = s.run(&p, &["export", "../out", "--format", "csl-json"]);
    assert_eq!(code, 0);
    assert!(
        out.contains("is not a papers library (no refs.bib)"),
        "{out:?}"
    );
    let (_, out, _) = s.run(&p, &["export", "../out2", "--format", "bib,ris"]);
    assert!(
        out.contains("(refs.bib, papers.ris; files: copy)"),
        "{out:?}"
    );
    assert!(Path::new(&s.path("out2/refs.bib")).exists());
    // the files are written with \n line ends
    for name in ["out/papers.json", "out2/papers.ris"] {
        assert!(!s.read(name).contains('\r'), "{name}");
    }
}

#[test]
fn export_message() {
    the_export_message_names_the_bibtex(false);
}

#[test]
#[ignore = "runs the reference (PAPERS_REFERENCE)"]
fn export_message_as_papers() {
    the_export_message_names_the_bibtex(true);
}

fn edit_from_a_file(reference: bool) {
    let s = Sandbox::new("papers.bib");
    let p = program(reference);
    // the links of FILE are relative to its directory (CSL-JSON, as for bibtex), as with `add FILE`
    s.write(
        "down/new.json",
        "[{\"id\": \"x\", \"type\": \"book\", \"title\": \"T_\", \"file\": \"paper.pdf\"}]",
    );
    let (code, _, err) = s.run(&p, &["edit", "one2000", "--from", "../down/new.json"]);
    assert_eq!((code, err.as_str()), (0, ""));
    let bib = s.read("lib/papers.bib");
    assert!(bib.contains("file = {:../down/paper.pdf:pdf}"), "{bib}");
    assert!(bib.contains("title = {T\\_}"), "{bib}");
    assert!(bib.contains("@book{one2000"), "{bib}");
    // the standard input: the links are stored as written, relative to the library
    let output = s.output_in(
        &p,
        "lib",
        &["edit", "two2000", "--from", "-"],
        "@article{x,\n title = {T},\n file = {:paper.pdf:pdf}\n}\n",
    );
    assert!(output.status.success());
    assert!(s.read("lib/papers.bib").contains("file = {:paper.pdf:pdf}"));
}

#[test]
fn edit_from() {
    edit_from_a_file(false);
}

#[test]
#[ignore = "runs the reference (PAPERS_REFERENCE)"]
fn edit_from_as_papers() {
    edit_from_a_file(true);
}

fn add_ris_with_stray_lines(reference: bool) {
    let s = Sandbox::new("papers.bib");
    let p = program(reference);
    // a blank line after TY, a line without tag, a last record without ER, & and _ escaped, the DOI of a link
    s.write(
        "in/a.ris",
        "TY  - JOUR\nKW  - a\nTI  - First & more\nER  - \n\nTY  - JOUR\n\nTI  - Second\nKW  - b\n\nAU  - Smith, J\nER  - \n\n\
         TY  - JOUR\nstray text\nTI  - Third\nUR  - https://example.org/p\nUR  - https://doi.org/10.1000/xyz\nER  - \n\n\
         TY  - THES\nTI  - Last\nPB  - ETH\nM3  - Master's thesis\n",
    );
    let (code, _, err) = s.run(&p, &["add", "../in/a.ris", "--no-pool", "--porcelain"]);
    assert_eq!((code, err.as_str()), (0, ""));
    let (_, out, _) = s.run(&p, &["list", "--field", "title", "--no-key"]);
    // in the order of the keys: from the DOI (Third), else from the title (first, last, second), as
    // an entry gets a key of its title when the key template gives none (bibtex.md 8)
    assert_eq!(out, "Third\nFirst \\& more\nLast\nOne\nSecond\nTwo\n");
    let (_, out, _) = s.run(&p, &["list", "--field", "doi", "url", "--no-key"]);
    assert!(out.contains("10.1000/xyz https://example.org/p"), "{out:?}");
    let (_, out, _) = s.run(&p, &["list", "--format", "ris"]);
    assert!(
        out.contains("TY  - THES\n")
            && out.contains("TI  - Last\nPB  - ETH\nM3  - Master's thesis\n"),
        "{out:?}"
    );
}

#[test]
fn add_ris() {
    add_ris_with_stray_lines(false);
}

#[test]
#[ignore = "runs the reference (PAPERS_REFERENCE)"]
fn add_ris_as_papers() {
    add_ris_with_stray_lines(true);
}

// the versions of the PDFs in papers list (formats.md 6, 6.1; tests/test_pdf_versions.py, TestList):
// the version of a file is that of its checksum in .papers/files.json, written here as papers file
// fetch writes it
// -------------------------------------------------------------------------------------------------

/// smith2020 has an accepted manuscript attached by papers file fetch, hand2000 a PDF attached by
/// hand, none2000 no file.
fn versions_sandbox() -> Sandbox {
    let s = Sandbox::new("papers.bib");
    s.write(
        "lib/papers.bib",
        "@article{hand2000,\n file = {:files/a.pdf:pdf},\n title = {Hand}\n}\n\n@article{none2000,\n title = {None}\n}\n\n\
         @article{smith2020,\n doi = {10.1000/ocean},\n file = {:files/am.pdf:pdf},\n title = {Ocean}\n}\n",
    );
    s.write("lib/files/a.pdf", "pdf a");
    s.write("lib/files/am.pdf", "%PDF am");
    let (a, am) = (
        papers_rs::history::store::sha256(b"pdf a"),
        papers_rs::history::store::sha256(b"%PDF am"),
    );
    s.write(
        "lib/.papers/files.json",
        &format!(
            "{{\n \"files\": {{\n  \"files/a.pdf\": \"{a}\",\n  \"files/am.pdf\": \"{am}\"\n }},\n \"versions\": {{\n  \"{am}\": \"accepted\"\n }}\n}}\n"
        ),
    );
    s
}

fn the_versions_in_the_listing(reference: bool) {
    let s = versions_sandbox();
    let p = program(reference);
    let (hand, file) = (s.path("lib/files/a.pdf"), s.path("lib/files/am.pdf"));
    // test_format_files
    let (code, out, _) = s.run(&p, &["list", "--format", "files", "--porcelain"]);
    assert_eq!(
        (code, out),
        (
            0,
            format!("hand2000 unknown {hand}\nsmith2020 accepted {file}\n")
        )
    );
    // test_json
    let (_, out, _) = s.run(&p, &["list", "--format", "csl-json", "smith2020"]);
    assert!(
        out.contains(&format!(
            "\"custom\": {{\n      \"file\": [\n        \"{file}\"\n      ],\n      \"file-version\": {{\n        \"{file}\": \"accepted\"\n      }}\n    }}"
        )),
        "{out}"
    );
    let (_, out, _) = s.run(&p, &["list", "--format", "csl-json", "hand2000"]);
    assert!(
        out.contains("\"custom\"") && !out.contains("file-version"),
        "{out}"
    );
    let (_, out, _) = s.run(&p, &["list", "--format", "bibjson"]);
    assert!(
        out.contains(&format!(
            "\"file-version\": {{\n        \"{file}\": \"accepted\"\n      }}"
        )),
        "{out}"
    );
    assert_eq!(out.matches("file-version").count(), 1, "{out}");
    // test_export_has_no_version
    let (code, _, _) = s.run(&p, &["export", "../out", "--format", "csl-json"]);
    assert_eq!(code, 0);
    assert!(!s.read("out/papers.json").contains("file-version"));
    // test_pdf_version_filter
    for (version, keys) in [
        ("accepted", "smith2020\n"),
        ("unknown", "hand2000\n"),
        ("published", ""),
        ("submitted", ""),
    ] {
        let (_, out, _) = s.run(&p, &["list", "--pdf-version", version, "-K"]);
        assert_eq!(out, keys, "{version}");
    }
    let (_, out, _) = s.run(&p, &["list", "--pdf-version", "accepted", "--invert", "-K"]);
    assert_eq!(out, "hand2000\nnone2000\n");
    // a files.json without versions: unknown (test_old_file_without_versions)
    let am = papers_rs::history::store::sha256(b"%PDF am");
    s.write(
        "lib/.papers/files.json",
        &format!("{{\n \"files\": {{\n  \"files/am.pdf\": \"{am}\"\n }}\n}}\n"),
    );
    let (_, out, _) = s.run(&p, &["list", "smith2020", "--format", "files"]);
    assert_eq!(out, format!("smith2020 unknown {file}\n"));
}

#[test]
fn versions_in_the_listing() {
    the_versions_in_the_listing(false);
}

#[test]
#[ignore = "runs the reference (PAPERS_REFERENCE)"]
fn versions_in_the_listing_as_papers() {
    the_versions_in_the_listing(true);
}

/// `papers file fetch --upgrade`: the options that do not go with it are usage errors, and nothing
/// to upgrade is said (`test_options_that_do_not_go_with_it`, `test_nothing_to_upgrade`).
fn the_upgrade_usage(reference: bool) {
    let s = versions_sandbox();
    let p = program(reference);
    for option in [
        &["--missing"][..],
        &["--again"],
        &["--supplements"],
        &["--version", "published"],
        &["--search", "."],
    ] {
        let mut args = vec!["file", "fetch", "--upgrade"];
        args.extend(option);
        let (code, _, err) = s.run(&p, &args);
        assert_eq!(code, 2, "{option:?}: {err}");
        assert!(
            err.contains(&format!(
                "papers file fetch: error: --upgrade and {}: --upgrade replaces an accepted manuscript or a submitted version by the published version",
                option[0]
            )),
            "{err}"
        );
    }
    let (code, _, err) = s.run(&p, &["file", "fetch"]);
    assert_eq!(code, 2);
    assert!(
        err.contains("error: papers file fetch KEY... or --missing [KEY...] or --upgrade [KEY...]"),
        "{err}"
    );
    // no entry has a lower version: nothing is looked up
    s.write("lib/.papers/files.json", "{\n \"files\": {}\n}\n");
    let (code, out, _) = s.run(&p, &["file", "fetch", "--upgrade"]);
    assert_eq!(code, 0);
    assert!(
        out.contains(
            "no entry has an accepted manuscript or a submitted version attached by papers file fetch"
        ),
        "{out}"
    );
    // an entry named without one is said
    let (code, out, _) = s.run(&p, &["file", "fetch", "--upgrade", "hand2000"]);
    assert_eq!(code, 0);
    assert!(
        out.contains("hand2000: no accepted manuscript or submitted version to replace"),
        "{out}"
    );
}

#[test]
fn upgrade_usage() {
    the_upgrade_usage(false);
}

#[test]
#[ignore = "runs the reference (PAPERS_REFERENCE)"]
fn upgrade_usage_as_papers() {
    the_upgrade_usage(true);
}
