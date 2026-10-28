//! The file commands (`papers_rs::command::file_cmd`) and what `add` does with PDFs, folders and URLs,
//! on libraries made in a temporary folder. The output of the commands is checked against papers by
//! the cases of `spec/` (`file/*`, `add/pdf-*`, `add/folder*`, `list/file-actions`).

use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;

use papers_rs::command::add::{AddRequest, add};
use papers_rs::command::add_pdf::{Item, group_attachments, scan_dir_items};
use papers_rs::command::file_cmd::{
    Attach, CheckOptions, MvOptions, attach_files, check_entries_files, checksums, clean,
    detach_entries, move_files, plan_moves, rename_entries, rm_files,
};
use papers_rs::command::files::Mode;
use papers_rs::command::queries::Queries;
use papers_rs::command::{Options, Session};
use papers_rs::config::{Config, MachineConfig};
use papers_rs::merge::{OnDuplicate, Resolver};
use serde_json::{Map, json};

struct Sandbox {
    _tmp: tempfile::TempDir,
    root: String,
}

fn sandbox() -> Sandbox {
    let tmp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(tmp.path())
        .unwrap()
        .to_string_lossy()
        .into_owned();
    Sandbox { _tmp: tmp, root }
}

fn entry(key: &str, files: &[&str], extra: &str) -> String {
    let file = if files.is_empty() {
        String::new()
    } else {
        let links: Vec<String> = files.iter().map(|f| format!(":{f}:")).collect();
        format!(" file = {{{}}},\n", links.join(";"))
    };
    format!(
        "@article{{{key},\n author = {{Doe, Jane}},\n{file}{extra} title = {{Title of {key}}},\n year = {{2000}}\n}}\n\n"
    )
}

impl Sandbox {
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

    fn exists(&self, rel: &str) -> bool {
        Path::new(&self.path(rel)).exists()
    }

    fn library(&self, bibtex: &str) {
        self.write(
            "lib/.papers/config.json",
            "{\n  \"bibtex\": \"papers.bib\",\n  \"filesdir\": \"files\",\n  \"pool\": \"none\",\n  \"version\": 3\n}",
        );
        self.write("lib/papers.bib", bibtex);
        fs::create_dir_all(self.path("lib/files")).unwrap();
    }

    fn config(&self) -> Config {
        let mut machine = Map::new();
        machine.insert("history-dir".into(), json!(self.path("history")));
        Config::load(
            &self.path("lib/.papers/config.json"),
            MachineConfig::new(None, machine),
            None,
        )
        .unwrap()
    }

    fn session(&self, message: &str) -> Session {
        self.session_with(message, false)
    }

    fn session_with(&self, message: &str, dry_run: bool) -> Session {
        Session::open(
            self.config(),
            Options {
                message: message.to_string(),
                dry_run,
                ..Options::default()
            },
        )
        .unwrap()
    }
}

fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| w.to_string()).collect()
}

#[test]
fn attach_places_the_file_and_detach_rm_take_it_away() {
    let s = sandbox();
    s.library(&entry("a2000", &[], ""));
    s.write("lib/x.txt", "x\n");
    s.write("lib/y.txt", "y\n");
    let mut session = s.session("papers file attach");
    attach_files(
        &mut session,
        "a2000",
        &[s.path("lib/x.txt"), s.path("lib/y.txt")],
        &Attach::new(Mode::Move),
    )
    .unwrap();
    assert!(session.commit().unwrap().saved);
    session.finish();
    assert!(s.exists("lib/files/doe_2000_title-of-a2000.txt"));
    assert!(s.exists("lib/files/doe_2000_title-of-a2000--y.txt"));
    assert!(!s.exists("lib/x.txt"));
    assert!(s.read("lib/papers.bib").contains(
        "file = {:files/doe_2000_title-of-a2000.txt:txt;:files/doe_2000_title-of-a2000--y.txt:txt}"
    ));

    // detach: the file stays
    let mut session = s.session("papers file detach");
    let first = s.path("lib/files/doe_2000_title-of-a2000.txt");
    detach_entries(&mut session, None, &[first], false, false).unwrap();
    let outcome = session.finish();
    assert_eq!(
        outcome.out,
        [format!(
            "a2000: removed the link to {}",
            s.path("lib/files/doe_2000_title-of-a2000.txt")
        )]
    );
    assert!(s.exists("lib/files/doe_2000_title-of-a2000.txt"));

    // rm: the link and the file; a nonexistent link is an error that changes nothing
    let mut session = s.session("papers file rm");
    let error = rm_files(&mut session, None, &strings(&["nothing.txt"]), false).unwrap_err();
    assert!(error.message().ends_with("(nothing was changed)"));
    let second = s.path("lib/files/doe_2000_title-of-a2000--y.txt");
    rm_files(&mut session, None, std::slice::from_ref(&second), false).unwrap();
    session.finish();
    assert!(!Path::new(&second).exists());
    assert!(!s.read("lib/papers.bib").contains("file ="));
}

#[test]
fn rename_places_the_files_by_the_name_template_and_a_dry_run_changes_nothing() {
    let s = sandbox();
    s.library(&entry("a2000", &["loose/a.txt"], ""));
    s.write("lib/loose/a.txt", "a\n");
    let before = s.read("lib/papers.bib");
    let mut session = s.session_with("papers file rename", true);
    rename_entries(&mut session, &strings(&["a2000"]), false, None).unwrap();
    session.commit().unwrap();
    session.finish();
    assert_eq!(s.read("lib/papers.bib"), before);
    assert!(s.exists("lib/loose/a.txt"));

    let mut session = s.session("papers file rename");
    rename_entries(&mut session, &strings(&["a2000"]), true, None).unwrap();
    session.commit().unwrap();
    session.finish();
    // --keep-source: copied (a hard link), the source stays
    assert!(s.exists("lib/loose/a.txt"));
    assert!(s.exists("lib/files/doe_2000_title-of-a2000.txt"));
}

#[test]
fn mv_follows_the_links_and_the_files_directory() {
    let s = sandbox();
    s.library(&(entry("a2000", &["files/a.txt"], "") + &entry("b2001", &["files/sub/b.txt"], "")));
    s.write("lib/files/a.txt", "a\n");
    s.write("lib/files/sub/b.txt", "b\n");
    s.write("lib/loose.txt", "l\n");
    let session = s.session("papers file mv");
    // the rules of git mv: nothing moves if a check fails
    let error = plan_moves(
        &session.library,
        &[s.path("lib/loose.txt")],
        &s.path("lib/elsewhere"),
        false,
        &[],
    )
    .unwrap_err();
    assert!(
        error.message().contains("no entry links to this file"),
        "{}",
        error.message()
    );
    session.finish();

    let mut session = s.session("papers file mv");
    move_files(
        &mut session,
        &[s.path("lib/files")],
        &s.path("lib/archive"),
        &MvOptions::default(),
    )
    .unwrap();
    let outcome = session.finish();
    assert!(
        outcome
            .out
            .contains(&"moved 1 of 1 file(s) or folder(s), 2 entries updated".to_string()),
        "{:?}",
        outcome.out
    );
    assert!(s.exists("lib/archive/a.txt") && s.exists("lib/archive/sub/b.txt"));
    assert!(!s.exists("lib/files"));
    let bibtex = s.read("lib/papers.bib");
    assert!(bibtex.contains(":archive/a.txt:txt") && bibtex.contains(":archive/sub/b.txt:txt"));
    assert!(
        s.read("lib/.papers/config.json")
            .contains("\"filesdir\": \"archive\"")
    );
}

#[test]
fn check_reports_and_fix_relinks_the_files_found_in_a_search_folder() {
    let s = sandbox();
    s.library(&entry("a2000", &["lost/a.txt", "files/here.txt"], ""));
    s.write("lib/files/here.txt", "here\n");
    s.write("lib/search/deep/a.txt", "a\n");
    let queries = Queries::default();
    let options = CheckOptions {
        search: vec![s.path("lib/search")],
        fix_command: "papers file fix".into(),
        ..CheckOptions::default()
    };
    let mut session = s.session_with("papers file check", true);
    check_entries_files(
        &mut session,
        &options,
        false,
        &strings(&["a2000"]),
        &queries,
    )
    .unwrap();
    let outcome = session.finish();
    assert_eq!(
        outcome.out[0],
        format!(
            "a2000: broken link: {} -> {} (found in {})",
            s.path("lib/lost/a.txt"),
            s.path("lib/search/deep/a.txt"),
            s.path("lib/search")
        )
    );
    assert!(
        outcome
            .out
            .contains(&"1 link can be relinked, 0 without a file found".to_string())
    );
    assert!(outcome.out.last().unwrap().contains("`papers file fix`"));

    // fix copies the file in the files directory: the source stays
    let mut session = s.session("papers file fix");
    check_entries_files(&mut session, &options, true, &strings(&["a2000"]), &queries).unwrap();
    session.finish();
    assert!(s.exists("lib/search/deep/a.txt"));
    assert!(s.exists("lib/files/doe_2000_title-of-a2000.txt"));
    assert!(
        s.read("lib/papers.bib")
            .contains(":files/doe_2000_title-of-a2000.txt:txt;:files/here.txt:txt")
    );
    let mut session = s.session_with("papers file check", true);
    check_entries_files(
        &mut session,
        &options,
        false,
        &strings(&["a2000"]),
        &queries,
    )
    .unwrap();
    assert_eq!(
        session.finish().out,
        ["no issue found in the files of 1 entries"]
    );
}

#[test]
fn clean_removes_the_unlinked_files_and_keeps_the_hidden_ones() {
    let s = sandbox();
    s.library(&entry("a2000", &["files/a.txt"], ""));
    for f in ["a.txt", "u.txt", ".hidden", "sub/s.txt", "n.bib"] {
        s.write(&format!("lib/files/{f}"), "x\n");
    }
    let mut session = s.session("papers file clean");
    clean(&mut session, true).unwrap();
    let outcome = session.finish();
    assert!(outcome.out.contains(&format!(
        "removed 2 unlinked file(s) from {}",
        s.path("lib/files")
    )));
    assert!(
        s.exists("lib/files/a.txt") && s.exists("lib/files/.hidden") && s.exists("lib/files/n.bib")
    );
    assert!(!s.exists("lib/files/u.txt") && !s.exists("lib/files/sub/s.txt"));
}

#[test]
fn checksum_writes_files_json_and_reports_a_changed_file() {
    let s = sandbox();
    s.library(&entry("a2000", &["files/a.txt"], ""));
    s.write("lib/files/a.txt", "a\n");
    let mut session = s.session("papers file checksum");
    checksums(&mut session, &strings(&["a2000"]), false).unwrap();
    assert_eq!(session.finish().out, ["1 checksum written (0 changed)"]);
    let written = s.read("lib/.papers/files.json");
    assert!(written.contains("\"files/a.txt\""), "{written}");
    s.write("lib/files/a.txt", "changed\n");
    let mut session = s.session("papers file checksum --check");
    checksums(&mut session, &strings(&["a2000"]), true).unwrap();
    let out = session.finish().out;
    assert_eq!(
        out[0],
        format!("{}: changed since its checksum", s.path("lib/files/a.txt"))
    );
    assert_eq!(
        s.read("lib/.papers/files.json"),
        written,
        "--check writes nothing"
    );
}

fn add_request(files: &[String]) -> AddRequest {
    AddRequest {
        files: files.to_vec(),
        mode: Mode::Move,
        ..AddRequest::default()
    }
}

const XMP: &[u8] = include_bytes!("fixtures/pdf/info-doi.pdf");

#[test]
fn a_pdf_of_an_entry_of_the_library_is_attached_without_a_query() {
    let s = sandbox();
    s.library(&entry("ice2020", &[], " doi = {10.1234/ICE.2020.1},\n"));
    fs::write(s.path("lib/paper.pdf"), XMP).unwrap();
    let mut session = s.session("papers add paper.pdf");
    let mut resolver = Resolver::new(OnDuplicate::Error);
    let touched = add(
        &mut session,
        &add_request(&[s.path("lib/paper.pdf")]),
        &mut resolver,
    )
    .unwrap();
    session.commit().unwrap();
    session.finish();
    assert_eq!(touched, ["ice2020"]);
    assert!(s.exists("lib/files/doe_2000_title-of-ice2020.pdf"));
    assert!(!s.exists("lib/paper.pdf"));
    // a pdf that cannot give metadata, without query, says so and changes nothing
    s.write("lib/bad.pdf", "not a pdf\n");
    let mut session = s.session("papers add bad.pdf");
    let mut resolver = Resolver::new(OnDuplicate::Error);
    let error = add(
        &mut session,
        &add_request(&[s.path("lib/bad.pdf")]),
        &mut resolver,
    )
    .unwrap_err();
    assert!(
        error.message().starts_with("Failed to open file"),
        "{}",
        error.message()
    );
    session.finish();
}

#[test]
fn a_folder_is_scanned_for_pdfs_with_their_attachments() {
    let s = sandbox();
    s.library(&entry("ice2020", &[], " doi = {10.1234/ICE.2020.1},\n"));
    fs::create_dir_all(s.path("lib/in/sub")).unwrap();
    fs::create_dir_all(s.path("lib/in/_skipped")).unwrap();
    fs::write(s.path("lib/in/paper.pdf"), XMP).unwrap();
    s.write("lib/in/paper--notes.txt", "notes\n");
    s.write("lib/in/sub/readme.txt", "readme\n");
    fs::write(s.path("lib/in/_skipped/other.pdf"), XMP).unwrap();
    let items = scan_dir_items(&s.path("lib/in"), true);
    assert_eq!(
        items,
        [
            Item::File(
                s.path("lib/in/paper.pdf"),
                vec![s.path("lib/in/paper--notes.txt")]
            ),
            Item::File(s.path("lib/in/sub/readme.txt"), vec![]),
        ]
    );

    let mut session = s.session("papers add -R in");
    let mut resolver = Resolver::new(OnDuplicate::Error);
    let mut request = add_request(&[s.path("lib/in")]);
    request.recursive = true;
    let touched = add(&mut session, &request, &mut resolver).unwrap();
    session.commit().unwrap();
    session.finish();
    assert_eq!(touched, ["ice2020"]);
    assert!(s.exists("lib/files/doe_2000_title-of-ice2020.pdf"));
    assert!(s.exists("lib/files/doe_2000_title-of-ice2020--notes.txt"));
    assert!(s.exists("lib/in/_skipped/other.pdf"));
}

#[test]
fn the_attachments_of_a_pdf_are_the_files_named_after_it() {
    let names = strings(&[
        "a.pdf",
        "a--b.txt",
        "a--b--c.txt",
        "d.txt",
        "e--f.pdf",
        "e.pdf",
    ]);
    assert_eq!(
        group_attachments(&names),
        [
            ("a.pdf".to_string(), strings(&["a--b.txt", "a--b--c.txt"])),
            ("d.txt".to_string(), vec![]),
            ("e.pdf".to_string(), strings(&["e--f.pdf"])),
        ]
    );
}

/// A server that answers one request with a file.
fn serve(content_type: &'static str, body: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut request = [0u8; 2048];
            let _ = stream.read(&mut request);
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    format!("http://127.0.0.1:{port}")
}

#[test]
fn a_url_is_downloaded_and_placed_in_the_files_directory_whatever_the_mode() {
    let s = sandbox();
    s.library(&entry("a2000", &[], ""));
    let url = serve("text/plain", "downloaded\n");
    let mut session = s.session("papers file attach");
    // link mode: a file that papers downloaded is still moved into the files directory
    attach_files(
        &mut session,
        "a2000",
        &[format!("{url}/notes")],
        &Attach::new(Mode::Link),
    )
    .unwrap();
    session.commit().unwrap();
    session.finish();
    assert_eq!(
        s.read("lib/files/doe_2000_title-of-a2000.txt"),
        "downloaded\n"
    );

    // a page that is not a file is refused
    let page = serve("text/html", "<html>login</html>");
    let mut session = s.session("papers file attach");
    let error = attach_files(
        &mut session,
        "a2000",
        &[format!("{page}/x")],
        &Attach::new(Mode::Link),
    )
    .unwrap_err();
    assert!(
        error
            .message()
            .starts_with("URL returned an HTML page rather than a file")
    );
    session.finish();
}

#[test]
fn an_attachment_that_does_not_exist_is_an_error() {
    let s = sandbox();
    s.library(&entry("a2000", &[], ""));
    let mut session = s.session("papers file attach");
    let error = attach_files(
        &mut session,
        "a2000",
        &strings(&["nothing.txt"]),
        &Attach::new(Mode::Move),
    )
    .unwrap_err();
    assert_eq!(error.message(), "file nothing.txt does not exist");
    session.finish();
}
