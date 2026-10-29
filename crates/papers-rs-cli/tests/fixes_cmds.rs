//! The commands and formats fixes of papers 3.0.0b7.dev49 (perrette/papers#188 and #191), through
//! the binary. Ported from `tests/test_spec_formats.py` (`TestExitCodes`, `TestListOutput`),
//! `tests/test_porcelain.py`, `tests/test_entry_cmds.py` (`TestEditSet`), `tests/test_spec_bibtex.py`
//! (`TestTags`, `TestDistinctFile`), `tests/test_spec_library.py` (`TestLayout`, `TestFindingTheLibrary`)
//! and `tests/test_parsecache.py` of papers (design/tests.md); the reference can be run on the same
//! cases:
//!
//! ```text
//! PAPERS_REFERENCE=papers cargo test -p papers-rs-cli --test fixes_cmds -- --ignored
//! ```

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

const RS: &str = env!("CARGO_BIN_EXE_papers-rs");

fn entry(key: &str, extra: &str) -> String {
    format!(
        "@article{{{key},\n author = {{Doe, Jane}},\n title = {{Title of {key}}},\n{extra} year = {{2000}}\n}}\n\n"
    )
}

struct Sandbox {
    _tmp: tempfile::TempDir,
    root: String,
}

impl Sandbox {
    /// A library `lib` (bibtex `papers.bib`, files directory `files`) with these entries, in a
    /// sandbox with a home of its own.
    fn new(entries: &[String]) -> Sandbox {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let s = Sandbox { _tmp: tmp, root };
        s.write(
            "lib/.papers/config.json",
            "{\n  \"bibtex\": \"papers.bib\",\n  \"filesdir\": \"files\",\n  \"pool\": \"none\",\n  \"version\": 3\n}",
        );
        s.write("lib/papers.bib", &entries.concat());
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

    fn output_in(&self, program: &str, dir: &str, args: &[&str]) -> Output {
        let mut words = program.split_whitespace();
        Command::new(words.next().unwrap())
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
            // `edit --key ''` must not open an editor
            .env("EDITOR", "false")
            .env("VISUAL", "false")
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap()
    }

    /// (exit code, stdout, stderr) of a command in the library, with its paths as `<R>`.
    fn run(&self, program: &str, args: &[&str]) -> (i32, String, String) {
        self.run_in(program, "lib", args)
    }

    fn run_in(&self, program: &str, dir: &str, args: &[&str]) -> (i32, String, String) {
        let output = self.output_in(program, dir, args);
        (
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stdout).replace(&self.root, "<R>"),
            String::from_utf8_lossy(&output.stderr).replace(&self.root, "<R>"),
        )
    }

    /// The links of the `file` field of an entry, from the bibtex.
    fn links(&self, key: &str) -> Vec<String> {
        let bib = self.read("lib/papers.bib");
        let block = bib
            .split("\n@")
            .find(|b| b.contains(&format!("{{{key},")))
            .unwrap_or_default();
        block
            .lines()
            .find_map(|l| l.trim().strip_prefix("file = {"))
            .map(|v| v.trim_end_matches(['}', ',']).to_string())
            .map(|v| v.split(';').map(str::to_string).collect())
            .unwrap_or_default()
    }

    /// The names of the files of the files directory, sorted.
    fn libfiles(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.path("lib/files"))
            .map(|d| {
                d.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
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

/// A test of papers-rs, and the same test run with the reference (`--ignored`).
macro_rules! both {
    ($rs:ident, $papers:ident, $body:ident) => {
        #[test]
        fn $rs() {
            $body(false);
        }

        #[test]
        #[ignore = "runs the reference (PAPERS_REFERENCE)"]
        fn $papers() {
            $body(true);
        }
    };
}

// exit codes
// ----------

/// `TestExitCodes::test_refused_rename`: a refused `file rename` and `list --rename` exit 1, the
/// files stay; the other entries are placed.
fn a_refused_rename_is_an_error(reference: bool) {
    let p = program(reference);
    let s = Sandbox::new(&[entry("one2000", ""), entry("two2000", "")]);
    s.write("a.pdf", "a");
    let (code, _, _) = s.run(&p, &["file", "attach", "one2000", "../a.pdf", "--move"]);
    assert_eq!(code, 0);
    let before = s.links("one2000");
    assert_eq!(before.len(), 1);
    for template in ["../{author}", "{nope}"] {
        for keys in [vec![], vec!["one2000"]] {
            let mut args = vec!["file", "rename", "--move"];
            args.extend(&keys);
            args.extend(["--name-template", template]);
            let (code, out, err) = s.run(&p, &args);
            assert_eq!((code, out.as_str()), (1, ""), "{args:?}: {err}");
            assert!(err.starts_with("papers: error: one2000: "), "{err}");
            assert_eq!(s.links("one2000"), before, "{args:?}");
        }
    }
    s.run(&p, &["config", "name.template", "../{author}"]);
    let (code, _, _) = s.run(&p, &["list", "one2000", "--rename"]);
    assert_eq!(code, 1);
    s.run(&p, &["config", "name.template", "{key}-x"]);
    let (code, _, err) = s.run(&p, &["file", "rename", "--move"]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(s.libfiles(), ["one2000-x.pdf"]);
}
both!(
    refused_rename,
    refused_rename_as_papers,
    a_refused_rename_is_an_error
);

/// Of two entries only one has a name under the template: the other is refused, this one is
/// placed and saved, and the porcelain lines are still printed.
fn the_other_entries_are_still_placed(reference: bool) {
    let p = program(reference);
    let s = Sandbox::new(&[
        entry("one2000", " doi = {10.1/x},\n file = {:../a.pdf:pdf},\n"),
        entry("two2000", " file = {:../b.pdf:pdf},\n"),
    ]);
    s.write("a.pdf", "a");
    s.write("b.pdf", "b");
    let (code, out, err) = s.run(
        &p,
        &[
            "file",
            "rename",
            "--move",
            "--name-template",
            "{doi}",
            "--porcelain",
        ],
    );
    assert_eq!((code, out.as_str()), (1, "modified one2000\n"), "{err}");
    assert!(
        err.contains("two2000: the file name '' points outside"),
        "{err}"
    );
    assert_eq!(s.libfiles(), ["10.1-x.pdf"]);
    assert_eq!(s.links("two2000"), [":../b.pdf:pdf"]);
    assert!(Path::new(&s.path("b.pdf")).exists());
}
both!(
    the_other_entries,
    the_other_entries_as_papers,
    the_other_entries_are_still_placed
);

/// `TestExitCodes::test_refused_attach`: a refused `file attach` exits 1, the link is kept.
fn a_refused_attach_is_an_error(reference: bool) {
    let p = program(reference);
    let s = Sandbox::new(&[entry("one2000", "")]);
    s.write("a.pdf", "a");
    s.write("b.pdf", "b");
    let pdf = s.path("a.pdf");
    let (code, out, err) = s.run(
        &p,
        &[
            "file",
            "attach",
            "one2000",
            &pdf,
            "--copy",
            "--name-template",
            "../{author}",
        ],
    );
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("points outside the files directory"), "{err}");
    // the library is saved with the link, and says it
    assert!(out.starts_with("Modified -> one2000: "), "{out}");
    assert_eq!(s.links("one2000").len(), 1);
    assert!(s.links("one2000")[0].contains("a.pdf"));
    s.run(&p, &["config", "name.template", "../{author}"]);
    s.run(&p, &["config", "files", "move"]);
    let other = s.path("b.pdf");
    let (code, _, err) = s.run(&p, &["list", "-k", "one2000", "--attach", &other]);
    assert_eq!(code, 1, "{err}");
}
both!(
    refused_attach,
    refused_attach_as_papers,
    a_refused_attach_is_an_error
);

/// `TestExitCodes::test_template_error`: a template that cannot be formatted is a clean error.
fn a_template_error_is_clean(reference: bool) {
    let p = program(reference);
    let s = Sandbox::new(&[entry("one2000", "")]);
    s.write("a.pdf", "a");
    let pdf = s.path("a.pdf");
    for template in ["{year.real}", "{year[x]}", "{nope}"] {
        let (code, _, err) = s.run(
            &p,
            &[
                "file",
                "attach",
                "one2000",
                &pdf,
                "--copy",
                "--name-template",
                template,
            ],
        );
        assert_eq!(code, 1, "{template}");
        assert!(err.starts_with("papers: error: "), "{err}");
        assert!(!err.contains("Traceback"), "{err}");
    }
    let (code, _, err) = s.run(
        &p,
        &[
            "add",
            "--title",
            "T",
            "--author",
            "A, B",
            "--year",
            "2001",
            "--key-template",
            "{year.real}",
        ],
    );
    assert_eq!(code, 1, "{err}");
    assert!(
        err.starts_with("papers: error: invalid key template"),
        "{err}"
    );
}
both!(
    template_error,
    template_error_as_papers,
    a_template_error_is_clean
);

/// `TestFindingTheLibrary::test_status_bibtex`: `status --bibtex FILE` for a FILE that is missing
/// is an error (exit 1), as for the other commands; for one that exists it shows that library.
fn status_of_a_bibtex(reference: bool) {
    let p = program(reference);
    let s = Sandbox::new(&[entry("one2000", "")]);
    let (code, out, err) = s.run(&p, &["status", "--bibtex", "missing.bib"]);
    assert_eq!((code, out.as_str()), (1, ""));
    assert_eq!(
        err,
        "papers: error: no bibtex file found, do `touch missing.bib` or `papers init --bibtex missing.bib`\n"
    );
    let (code, _, err) = s.run(
        &p,
        &["status", "--bibtex", "../x/y.bib", "--no-check-files"],
    );
    assert_eq!(code, 1, "{err}");
    let (code, out, _) = s.run(&p, &["status", "--bibtex", "papers.bib"]);
    assert_eq!(code, 0);
    assert!(
        out.starts_with("papers library: <R>/lib\n* bibtex: papers.bib (1 entries"),
        "{out}"
    );
    // --filesdir is read too
    let (_, out, _) = s.run(&p, &["status", "--filesdir", "nofiles"]);
    assert!(
        out.contains("* files:  nofiles (the files directory of the library, missing)"),
        "{out}"
    );
    // outside of any library
    s.write("other/x.txt", "");
    let (code, _, err) = s.run_in(&p, "other", &["status", "--bibtex", "missing.bib"]);
    assert_eq!(code, 1, "{err}");
    assert!(
        err.starts_with("papers: error: no bibtex file found"),
        "{err}"
    );
}
both!(status_bibtex, status_bibtex_as_papers, status_of_a_bibtex);

// dry runs and the files of .papers/
// ----------------------------------

/// `TestLayout::test_no_move_with_other_spellings_of_dry_run`: the files of the `.papers/` next to
/// the bibtex are not moved with `--dry-run` written otherwise (an abbreviation, `file mv -n`); a
/// command without it moves them.
fn dry_run_spellings_move_nothing(reference: bool) {
    let p = program(reference);
    let s = Sandbox::new(&[entry("one2000", "")]);
    s.write(
        "lib/.papers/config.json",
        "{\n  \"bibtex\": \"refs/papers.bib\",\n  \"filesdir\": \"files\",\n  \"pool\": \"none\",\n  \"version\": 3\n}",
    );
    s.write("lib/refs/papers.bib", &entry("one2000", ""));
    s.write("lib/refs/.papers/distinct.json", "{\"distinct\": []}\n");
    for args in [
        vec!["list", "--dry"],
        vec!["list", "--dry-r"],
        vec!["file", "mv", "-n", "x.pdf", "y.pdf"],
    ] {
        s.run(&p, &args);
        assert!(
            Path::new(&s.path("lib/refs/.papers/distinct.json")).exists(),
            "{args:?}"
        );
        assert!(
            !Path::new(&s.path("lib/.papers/distinct.json")).exists(),
            "{args:?}"
        );
    }
    s.run(&p, &["list"]);
    assert!(!Path::new(&s.path("lib/refs/.papers/distinct.json")).exists());
    assert!(Path::new(&s.path("lib/.papers/distinct.json")).exists());
}
both!(
    dry_run_spellings,
    dry_run_spellings_as_papers,
    dry_run_spellings_move_nothing
);

// porcelain
// ---------

/// `TestPorcelain::test_tag_rename`: `tag --rename` prints `modified KEY` for each entry changed.
fn tag_rename_porcelain(reference: bool) {
    let p = program(reference);
    let s = Sandbox::new(&[
        entry("one2000", " keywords = {climate},\n"),
        entry("two2000", " keywords = {climate, x},\n"),
        entry("three2000", ""),
    ]);
    let (code, out, err) = s.run(&p, &["tag", "--rename", "climate", "ocean", "--porcelain"]);
    assert_eq!(
        (code, out.as_str(), err.as_str()),
        (
            0,
            "modified one2000\nmodified two2000\n",
            "renamed tag climate to ocean in 2 entries\n"
        )
    );
    // nothing changed: no line
    let (_, out, _) = s.run(&p, &["tag", "--rename", "nothing", "other", "--porcelain"]);
    assert_eq!(out, "");
    // without --porcelain: the line only
    let (_, out, _) = s.run(&p, &["tag", "--rename", "ocean", "climate"]);
    assert_eq!(out, "renamed tag ocean to climate in 2 entries\n");
}
both!(tag_rename, tag_rename_as_papers, tag_rename_porcelain);

/// `TestPorcelain::test_add_skipped_duplicate_is_existing`: a duplicate skipped by
/// `--on-duplicate skip` is `existing KEY` (the library entry), with its line `Existing`.
fn a_skipped_duplicate_is_existing(reference: bool) {
    let p = program(reference);
    let s = Sandbox::new(&[entry("one2000", " doi = {10.1/one},\n")]);
    s.write(
        "new.bib",
        &entry("other2000", " doi = {10.1/one},\n journal = {J},\n"),
    );
    let (code, out, err) = s.run(
        &p,
        &["add", "../new.bib", "--on-duplicate", "skip", "--porcelain"],
    );
    assert_eq!((code, out.as_str()), (0, "existing one2000\n"));
    assert!(err.contains("skipped: other2000"), "{err}");
    let (_, out, _) = s.run(&p, &["add", "../new.bib", "--on-duplicate", "skip"]);
    assert_eq!(
        out,
        "skipped: other2000 (already in library: one2000)\nExisting -> one2000: Title of one2000 (doi:10.1/one)\n"
    );
}
both!(
    skipped_duplicate,
    skipped_duplicate_as_papers,
    a_skipped_duplicate_is_existing
);

/// `TestEditSet::test_key_case_only` (formats.md 7.2, bibtex.md 7.3): a change of case of the key
/// is a rename, and `crossref` follows.
fn a_key_changed_in_case_is_renamed(reference: bool) {
    let p = program(reference);
    let s = Sandbox::new(&[
        entry("one2000", ""),
        "@incollection{chap2000,\n crossref = {one2000},\n title = {A chapter}\n}\n\n".to_string(),
    ]);
    let (code, out, err) = s.run(&p, &["edit", "one2000", "--key", "One2000", "--porcelain"]);
    assert_eq!(
        (code, out.as_str(), err.as_str()),
        (0, "renamed one2000 One2000\nmodified chap2000\n", "")
    );
    let bib = s.read("lib/papers.bib");
    assert!(bib.contains("crossref = {One2000}"), "{bib}");
    assert!(bib.contains("@article{One2000,"), "{bib}");
    // and back, without --porcelain
    let (_, out, _) = s.run(&p, &["edit", "One2000", "--key", "one2000"]);
    assert_eq!(
        out,
        "Renamed -> one2000: Title of one2000 (was One2000)\nModified -> chap2000: A chapter \n"
    );
}
both!(
    key_case_only,
    key_case_only_as_papers,
    a_key_changed_in_case_is_renamed
);

// edit
// ----

/// `TestEditSet::test_set_refused`: `--set key=...` is refused in any case, like `ID`.
fn set_of_the_key_is_refused(reference: bool) {
    let p = program(reference);
    let s = Sandbox::new(&[entry("one2000", " file = {:files/one.pdf:pdf},\n")]);
    s.write("lib/files/one.pdf", "x");
    let before = s.read("lib/papers.bib");
    for value in [
        "file=x.pdf",
        "ID=x",
        "id=x",
        "key=x",
        "KEY=x",
        "Key=x",
        "bad.name=x",
    ] {
        let (code, out, _) = s.run(&p, &["edit", "one2000", "--set", value]);
        assert_eq!((code, out.as_str()), (1, ""), "{value}");
    }
    let (_, _, err) = s.run(&p, &["edit", "one2000", "--set", "KEY=x"]);
    assert_eq!(
        err,
        "papers: error: KEY: the key is changed by `papers edit KEY --key NEW`, not --set\n"
    );
    let (code, _, _) = s.run(&p, &["edit", "one2000", "--unset", "file"]);
    assert_eq!(code, 1);
    let (code, _, _) = s.run(&p, &["edit", "one2000", "--set", "novalue"]);
    assert_eq!(code, 2);
    let (code, _, _) = s.run(&p, &["edit", "one2000", "--set", "a=b", "--key", "x"]);
    assert_eq!(code, 2);
    assert_eq!(s.read("lib/papers.bib"), before);
}
both!(
    set_refused,
    set_refused_as_papers,
    set_of_the_key_is_refused
);

/// `TestEditSet::test_empty_key_refused` (bibtex.md 7.2): `--key ''` is refused, it does not open
/// the editor.
fn an_empty_key_is_refused(reference: bool) {
    let p = program(reference);
    let s = Sandbox::new(&[entry("one2000", "")]);
    let before = s.read("lib/papers.bib");
    let (code, out, err) = s.run(&p, &["edit", "one2000", "--key", ""]);
    assert_eq!((code, out.as_str()), (1, ""));
    assert_eq!(
        err,
        "papers: error: '' is not a valid key (no spaces, commas, braces, quotes, or #%~=()\\; not starting with a digit)\n"
    );
    assert_eq!(s.read("lib/papers.bib"), before);
}
both!(empty_key, empty_key_as_papers, an_empty_key_is_refused);

// tags and search
// ---------------

/// `TestTags::test_counting_order` (bibtex.md 6): the tags by count, then without case, then by
/// the tag as written.
fn the_tags_are_counted_in_order(reference: bool) {
    let p = program(reference);
    let s = Sandbox::new(&[
        entry("a2000", " keywords = {Zeta, alpha, Alpha, beta, b, B},\n"),
        entry("b2000", " keywords = {zeta, beta, ALPHA},\n"),
    ]);
    let (code, out, _) = s.run(&p, &["tag"]);
    assert_eq!(
        (code, out.as_str()),
        (
            0,
            "beta (2)\nALPHA (1)\nAlpha (1)\nalpha (1)\nB (1)\nb (1)\nZeta (1)\nzeta (1)\n"
        )
    );
}
both!(
    counting_order,
    counting_order_as_papers,
    the_tags_are_counted_in_order
);

/// `TestListOutput::test_search_normalized` (formats.md 6.1): the words and the texts of the
/// entries are compared after `norm` (bibtex.md 9.1).
fn the_search_compares_normalized_texts(reference: bool) {
    let p = program(reference);
    let s = Sandbox::new(&[
        "@article{latex2000,\n author = {M{\\\"u}ller, Jan},\n title = {{S}ea level},\n year = {2000}\n}\n\n".to_string(),
        "@article{plain2000,\n author = {Müller, Jan},\n title = {Straße},\n year = {2000}\n}\n\n".to_string(),
    ]);
    let keys = |args: &[&str]| {
        let mut all = vec!["list", "-K"];
        all.extend(args);
        s.run(&p, &all).1
    };
    let both = "latex2000\nplain2000\n";
    assert_eq!(keys(&["Muller"]), both);
    assert_eq!(keys(&["Müller"]), both);
    assert_eq!(keys(&["--author", "muller"]), both);
    assert_eq!(keys(&["--first-author", "M{\\\"u}ller", "--strict"]), both);
    assert_eq!(keys(&["--title", "sea level"]), "latex2000\n");
    assert_eq!(keys(&["--title", "strasse"]), "plain2000\n");
    assert_eq!(keys(&["--title", "str?sse", "--strict"]), "plain2000\n");
}
both!(
    search_normalized,
    search_normalized_as_papers,
    the_search_compares_normalized_texts
);

// distinct.json, warnings
// -----------------------

/// `TestDistinctFile::test_reading_bad_values` and `test_distinct_read_once` (bibtex.md 9.4): a
/// `distinct` that is not an array makes the file unreadable, with one warning; pairs that are not
/// pairs of objects are ignored.
fn a_distinct_file_with_bad_values(reference: bool) {
    let p = program(reference);
    let s = Sandbox::new(&[entry("one2000", ""), entry("two2000", "")]);
    for text in [
        "{\"distinct\": 5}",
        "{\"distinct\": null}",
        "{\"distinct\": {}}",
        "[1, 2]",
    ] {
        s.write("lib/.papers/distinct.json", text);
        let (code, out, err) = s.run(&p, &["list", "-K"]);
        assert_eq!((code, out.as_str()), (0, "one2000\ntwo2000\n"), "{text}");
        assert_eq!(
            err,
            "papers: warning: <R>/lib/.papers/distinct.json: could not be read (not an object with a list \"distinct\"); ignored\n",
            "{text}"
        );
    }
    // the pairs that are not pairs of objects: ignored, the file is read
    s.write(
        "lib/.papers/distinct.json",
        "{\"distinct\": [[\"a\", \"b\"], [{\"title\": \"t\"}, 3], [{\"title\": \"t\"}, {\"title\": \"u\"}]]}",
    );
    let (code, out, err) = s.run(&p, &["list", "-K"]);
    assert_eq!(
        (code, out.as_str(), err.as_str()),
        (0, "one2000\ntwo2000\n", "")
    );
}
both!(
    distinct_bad_values,
    distinct_bad_values_as_papers,
    a_distinct_file_with_bad_values
);

// formats
// -------

/// `TestPartsToNames::test_one_name_not_in_an_array` (formats.md 2.3, 3.3): a CSL-JSON `author` that
/// is a string or an object is one name.
fn one_name_not_in_an_array(reference: bool) {
    let p = program(reference);
    let s = Sandbox::new(&[entry("one2000", "")]);
    s.write(
        "a.json",
        "[{\"id\": \"a\", \"type\": \"book\", \"title\": \"A\", \"author\": \"Smith, John\"},\
         {\"id\": \"b\", \"type\": \"book\", \"title\": \"B\", \"author\": {\"family\": \"Doe\", \"given\": \"Jane\"}, \"editor\": {\"literal\": \"The Team\"}},\
         {\"id\": \"c\", \"type\": \"book\", \"title\": \"C\", \"author\": \"\", \"editor\": {}},\
         {\"id\": \"d\", \"type\": \"book\", \"title\": \"D\", \"author\": [\"X Y\", {\"family\": \"Z\"}, \"\"], \"editor\": \"A B and C D\"}]",
    );
    let (code, out, err) = s.run(&p, &["add", "../a.json", "--porcelain"]);
    assert_eq!(
        (code, out.as_str(), err.as_str()),
        (0, "added a\nadded b\nadded c\nadded d\n", "")
    );
    let (_, out, _) = s.run(&p, &["list", "--field", "author", "editor"]);
    assert_eq!(
        out,
        "a: {Smith, John} \nb: Doe, Jane {The Team}\nc:  \nd: {X Y} and Z {A B and C D}\none2000: Doe, Jane \n"
    );
}
both!(one_name, one_name_as_papers, one_name_not_in_an_array);
