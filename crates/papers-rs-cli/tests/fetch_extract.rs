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
    papers_with(args, dir, &[])
}

fn papers_with(args: &[&str], dir: &Path, env: &[(&str, &str)]) -> Output {
    let home = dir.join("home");
    fs::create_dir_all(&home).unwrap();
    Command::new(env!("CARGO_BIN_EXE_papers-rs"))
        .args(args)
        .current_dir(dir)
        .env_clear()
        .env("HOME", home)
        .envs(env.iter().copied())
        .output()
        .unwrap()
}

/// A proxy that refuses the connection (on this machine): a request fails without leaving it.
const NO_NETWORK: [(&str, &str); 2] = [
    ("HTTPS_PROXY", "http://127.0.0.1:9"),
    ("https_proxy", "http://127.0.0.1:9"),
];

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
        // a file that is not a PDF: the error of the file, not a traceback (papers 3, issue 189)
        (
            &["extract", "--doi-only", "not-a-pdf.pdf"],
            "papers: error: not-a-pdf.pdf: Failed to open file 'not-a-pdf.pdf'.\n",
        ),
        (
            &["extract", "not-a-pdf.pdf"],
            "papers: error: not-a-pdf.pdf: Failed to open file 'not-a-pdf.pdf'.\n",
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

/// A network error is an error of the DOI or the file, not a traceback (papers 3, issue 189): the
/// DOI, then the reason. (The reason is that of the program's HTTP client, not of Python's requests.)
#[test]
fn a_network_error_is_an_error_of_the_doi() {
    let dir = tempfile::tempdir().unwrap();
    let output = papers_with(&["fetch", "10.1000/nw"], dir.path(), &NO_NETWORK);
    assert_eq!(
        (output.status.code(), text(&output.stdout)),
        (Some(1), String::new())
    );
    assert!(
        text(&output.stderr).starts_with("papers: error: 10.1000/nw: ")
            && text(&output.stderr).lines().count() == 1,
        "{}",
        text(&output.stderr)
    );
    // a full-text search has no DOI to name
    let output = papers_with(
        &["fetch", "Climate", "sensitivity"],
        dir.path(),
        &NO_NETWORK,
    );
    assert_eq!(output.status.code(), Some(1));
    let stderr = text(&output.stderr);
    assert!(
        stderr.starts_with("papers: error: ")
            && stderr.lines().count() == 1
            && !stderr.contains("Climate: "),
        "{stderr}"
    );
    // add --doi, in a library
    let output = papers_with(&["init"], dir.path(), &NO_NETWORK);
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    let output = papers_with(&["add", "--doi", "10.1000/nw"], dir.path(), &NO_NETWORK);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        text(&output.stderr).starts_with("papers: error: 10.1000/nw: ")
            && text(&output.stderr).lines().count() == 1,
        "{}",
        text(&output.stderr)
    );
    // the file whose metadata cannot be had says it, with the file
    let dir = folder(&["no-doi.pdf"]);
    let output = papers_with(
        &["extract", "--fulltext", "no-doi.pdf"],
        dir.path(),
        &NO_NETWORK,
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(
        text(&output.stderr).contains("papers: error: no-doi.pdf: "),
        "{}",
        text(&output.stderr)
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
        "papers fetch: error: Fetching from DOI does not support the options --scholar and --openalex\n"
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

/// `--openalex` (papers #205): a DOI has no search.
#[test]
fn fetch_a_doi_takes_neither_scholar_nor_openalex() {
    let dir = tempfile::tempdir().unwrap();
    for option in ["--scholar", "--openalex"] {
        let output = papers(&["fetch", "10.1000/abc", option], dir.path());
        assert_eq!(output.status.code(), Some(2));
        assert!(
            text(&output.stderr).ends_with(
                "papers fetch: error: Fetching from DOI does not support the options --scholar and --openalex\n"
            ),
            "{}",
            text(&output.stderr)
        );
    }
}

#[test]
fn openalex_is_not_with_scholar() {
    let dir = tempfile::tempdir().unwrap();
    for args in [vec!["fetch", "Climate", "sensitivity"], vec!["extract", "a.pdf"], vec!["add", "a.pdf"]] {
        let mut args = args.clone();
        args.extend(["--scholar", "--openalex"]);
        let output = papers(&args, dir.path());
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(
            text(&output.stderr).ends_with(": error: argument --openalex: not allowed with argument --scholar\n"),
            "{}",
            text(&output.stderr)
        );
    }
}

#[test]
fn openalex_changes_nothing_before_the_search() {
    let dir = tempfile::tempdir().unwrap();
    for args in [vec!["extract", "missing.pdf"], vec!["extract", "--doi-only", "missing.pdf"]] {
        let plain = papers(&args, dir.path());
        let mut with = args.clone();
        with.push("--openalex");
        let with = papers(&with, dir.path());
        assert_eq!(plain.status.code(), with.status.code());
        assert_eq!(text(&plain.stdout), text(&with.stdout));
        assert_eq!(text(&plain.stderr), text(&with.stderr), "{args:?}");
    }
}

/// A search that fails is a clean error, without the key and the email address of the user.
#[test]
fn a_failed_search_of_openalex_is_a_clean_error_without_the_key() {
    let dir = tempfile::tempdir().unwrap();
    let mut env = NO_NETWORK.to_vec();
    env.extend([("OPENALEX_API_KEY", "secret-key"), ("PAPERS_MAILTO", "me@example.org")]);
    let output = papers_with(&["fetch", "Climate", "sensitivity", "revisited", "--openalex"], dir.path(), &env);
    let stderr = text(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.starts_with("papers: error: OpenAlex: "), "{stderr}");
    assert!(!stderr.contains("secret-key") && !stderr.contains("example.org"), "{stderr}");
    assert!(text(&output.stdout).is_empty());
}

/// The network: `cargo test -- --ignored`
#[test]
#[ignore]
fn fetch_an_unknown_doi_is_a_clean_error() {
    let dir = tempfile::tempdir().unwrap();
    let output = papers(&["fetch", "10.1000/does-not-exist-zzz"], dir.path());
    assert_eq!(
        (
            output.status.code(),
            text(&output.stdout),
            text(&output.stderr)
        ),
        (
            Some(1),
            String::new(),
            "papers: error: 10.1000/does-not-exist-zzz: unknown to doi.org\n".into()
        )
    );
}

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
