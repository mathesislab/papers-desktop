//! `papers-rs fetch` and `papers-rs extract` where they need no network: the options and the
//! errors, the DOI of the PDF metadata. The cases of `spec/cases/fetch` and `extract` check them
//! (and, with `--with network`, the queries) against papers itself; the queries and the
//! extraction are tested in the library (`crates/papers-rs/tests/fetch.rs`) with the answers
//! of the network replayed.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn pdfs() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../papers-rs/tests/fixtures/pdf")
}

fn papers(args: &[&str], dir: &Path) -> Output {
    let home = dir.join("home");
    fs::create_dir_all(&home).unwrap();
    Command::new(env!("CARGO_BIN_EXE_papers-rs"))
        .args(args)
        .current_dir(dir)
        .env_clear()
        .env("HOME", home)
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// A folder with copies of the PDFs.
fn folder(names: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for name in names {
        fs::copy(pdfs().join(name), dir.path().join(name)).unwrap();
    }
    dir
}

#[test]
fn the_doi_of_the_metadata_of_a_pdf() {
    let dir = folder(&["xmp-doi.pdf", "info-doi.pdf"]);
    let output = papers(&["extract", "--doi-only", "xmp-doi.pdf"], dir.path());
    assert_eq!(
        (
            output.status.code(),
            text(&output.stdout),
            text(&output.stderr)
        ),
        (Some(0), "10.1234/ICE.2020.1\n".into(), String::new())
    );
    // several files: a header for each
    let output = papers(
        &["extract", "--doi-only", "info-doi.pdf", "xmp-doi.pdf"],
        dir.path(),
    );
    assert_eq!(
        text(&output.stdout),
        "% info-doi.pdf\n10.1234/ice.2020.1\n% xmp-doi.pdf\n10.1234/ICE.2020.1\n"
    );
}

#[test]
fn a_directory_needs_r_and_is_scanned_for_pdf_files() {
    let dir = folder(&["xmp-doi.pdf"]);
    fs::create_dir_all(dir.path().join("sub")).unwrap();
    fs::copy(pdfs().join("info-doi.pdf"), dir.path().join("sub/B.PDF")).unwrap();
    fs::write(dir.path().join("sub/notes.txt"), "x").unwrap();
    let output = papers(&["extract", "--doi-only", "sub"], dir.path());
    assert_eq!(output.status.code(), Some(2));
    assert!(
        text(&output.stderr).ends_with(
            "papers extract: error: sub is a directory, requires -R/--recursive to scan\n"
        ),
        "{}",
        text(&output.stderr)
    );
    let output = papers(&["extract", "--doi-only", "-R", "./", "sub/"], dir.path());
    assert_eq!(
        text(&output.stdout),
        "% sub/B.PDF\n10.1234/ice.2020.1\n% xmp-doi.pdf\n10.1234/ICE.2020.1\n% sub/B.PDF\n10.1234/ice.2020.1\n"
    );
    // no PDF: a warning, and nothing else
    fs::create_dir_all(dir.path().join("empty")).unwrap();
    let output = papers(&["extract", "-R", "empty"], dir.path());
    assert_eq!(
        (
            output.status.code(),
            text(&output.stdout),
            text(&output.stderr)
        ),
        (
            Some(0),
            String::new(),
            "papers: warning: no PDF file found\n".into()
        )
    );
}

#[test]
fn the_errors_of_the_files() {
    let dir = folder(&["xmp-doi.pdf", "no-doi.pdf"]);
    fs::write(dir.path().join("not-a-pdf.pdf"), "text").unwrap();
    for (args, stderr) in [
        (
            &["extract", "--doi-only", "missing.pdf"][..],
            "papers: error: missing.pdf: no such file\n",
        ),
        (
            &["extract", "--doi-only", "no-doi.pdf"],
            "papers: error: no-doi.pdf: no DOI found in the text\n",
        ),
        // papers does not catch that one: a traceback (that ends with this message)
        (
            &["extract", "--doi-only", "not-a-pdf.pdf"],
            "papers: error: Failed to open file 'not-a-pdf.pdf'.\n",
        ),
    ] {
        let output = papers(args, dir.path());
        assert_eq!(
            (output.status.code(), text(&output.stdout)),
            (Some(1), String::new()),
            "{args:?}"
        );
        assert_eq!(text(&output.stderr), stderr, "{args:?}");
    }
    // several files: the others are done, the count of those that failed is the error
    let output = papers(
        &[
            "extract",
            "--doi-only",
            "xmp-doi.pdf",
            "missing.pdf",
            "no-doi.pdf",
        ],
        dir.path(),
    );
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        text(&output.stdout),
        "% xmp-doi.pdf\n10.1234/ICE.2020.1\n% missing.pdf\n% no-doi.pdf\n"
    );
    assert_eq!(
        text(&output.stderr),
        "papers: error: missing.pdf: no such file\npapers: error: no-doi.pdf: no DOI found in the text\npapers: error: 2 of 3 files failed\n"
    );
}

#[test]
fn the_usage_of_fetch() {
    let dir = tempfile::tempdir().unwrap();
    let output = papers(&["fetch"], dir.path());
    assert_eq!(output.status.code(), Some(2));
    assert!(
        text(&output.stderr)
            .ends_with("papers fetch: error: the following arguments are required: doi_or_text\n")
    );
    let output = papers(
        &[
            "fetch",
            "doi:10.1000/abc",
            "https://doi.org/10.1000/def",
            "--scholar",
        ],
        dir.path(),
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(text(&output.stderr).ends_with(
        "papers fetch: error: Fetching from DOI does not support Google Scholar option\n"
    ));
    // a search by Google Scholar is not ported
    let output = papers(&["fetch", "some", "words", "--scholar"], dir.path());
    assert_eq!(output.status.code(), Some(1));
    assert!(text(&output.stderr).contains("not implemented in papers-rs yet"));
}

#[test]
fn doi_is_the_deprecated_extract_doi_only() {
    let dir = folder(&["xmp-doi.pdf"]);
    let output = papers(&["doi", "xmp-doi.pdf"], dir.path());
    assert_eq!(text(&output.stdout), "10.1234/ICE.2020.1\n");
    assert_eq!(
        text(&output.stderr),
        "papers: warning: `papers doi` is deprecated: use `papers extract --doi-only`\n"
    );
}

/// The network: `cargo test -- --ignored`
#[test]
#[ignore]
fn fetch_a_doi_from_crossref() {
    let dir = tempfile::tempdir().unwrap();
    let output = papers(&["fetch", "10.5194/tc-11-1015-2017"], dir.path());
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    let bibtex = text(&output.stdout);
    assert!(
        bibtex.starts_with("@article{10.5194/tc-11-1015-2017,\n author = {Fettweis, Xavier"),
        "{bibtex}"
    );
    assert!(bibtex.ends_with("}\n\n"));
}
