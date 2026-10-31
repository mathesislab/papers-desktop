//! `papers file fetch`, `add --fetch-pdf`, `fetch --pdf`: the free PDF of an entry
//! (`papers_rs::fetch::freepdf`, `papers_rs::command::fetch_pdf`), with the network mocked: the
//! tests of papers (`tests/test_file_fetch.py`), on the API.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use papers_rs::command::add::{AddRequest, add};
use papers_rs::command::fetch_pdf::{
    FetchSummary, Fetched, FileFetch, FileUpgrade, PdfOptions, confirm_openalex, fetch_entry_pdfs,
    fetch_free_pdf, file_fetch, file_upgrade, one_line, pdf_porcelain_lines,
};
use papers_rs::command::file_cmd::{
    Attach, CheckOptions, MvOptions, attach_files, check_entries_files, checksums, move_files,
    rm_files,
};
use papers_rs::command::files::Mode;
use papers_rs::command::queries::Queries;
use papers_rs::command::{Options, Prompt, Session};
use papers_rs::config::{Config, MachineConfig};
use papers_rs::fetch::Fetcher;
use papers_rs::fetch::freepdf::{
    self, Least, Search, Version, browser_links, fetch_pdf, first_pdf,
};
use papers_rs::fetch::http::{Response, Transport};
use papers_rs::fetch::openalex::{OpenAlex, WORK_SELECT};
use papers_rs::history::restore::file_digest;
use papers_rs::history::{FilesJson, History, OpKind};
use papers_rs::merge::{OnDuplicate, Resolver};
use serde_json::{Map, Value, json};

const DOI: &str = "10.1000/ocean";
const PAGE: &str = "https://journals.example.org/article/ocean";
const PDF: &str = "https://journals.example.org/article/ocean.pdf";
const PDF_ACCEPT: &str = "application/pdf,*/*;q=0.8";

fn pdf_bytes(name: &str) -> Vec<u8> {
    format!("%PDF-1.4\n{name}\n%%EOF\n").into_bytes()
}

fn page(pdf: Option<&str>, links: &[(&str, &str)], meta: &[&str]) -> String {
    let mut head = pdf
        .map(|p| format!(r#"<meta name="citation_pdf_url" content="{p}">"#))
        .unwrap_or_default();
    for url in meta {
        head.push_str(&format!(
            r#"<meta name="citation_supplementary_material" content="{url}">"#
        ));
    }
    let body: String = links
        .iter()
        .map(|(url, text)| format!(r#"<a href="{url}">{text}</a>"#))
        .collect();
    format!("<html><head>{head}</head><body>{body}</body></html>")
}

fn location(pdf_url: &str, version: &str, landing: Option<&str>, is_oa: bool) -> Value {
    json!({"pdf_url": pdf_url, "landing_page_url": landing, "version": version, "is_oa": is_oa,
           "source": {"display_name": "somewhere"}})
}

/// A file of the web: its content, or an HTTP status.
#[derive(Clone)]
enum File {
    Bytes(Vec<u8>),
    Status(u16),
    /// HTTP 429, with its `Retry-After` header
    TooMany(Option<&'static str>),
    /// the answers of the requests in turn (the last one stays)
    Answers(Vec<File>),
}

/// The sites: pages `{url: (final url, html)}` (`busy`: a page that answers HTTP 429 once, with
/// its `Retry-After`), OpenAlex `{doi: locations}` (none: 404), files, and the requests made
/// (`(kind, url)`).
#[derive(Default)]
struct WebState {
    pages: HashMap<String, (String, String)>,
    busy: HashMap<String, Option<&'static str>>,
    openalex: HashMap<String, Vec<Value>>,
    files: HashMap<String, File>,
    crossref: HashMap<String, Value>,
    requests: Vec<(String, String)>,
}

#[derive(Clone, Default)]
struct Web(Arc<Mutex<WebState>>);

fn answer(status: u16, content_type: &str, body: Vec<u8>, url: &str) -> Response {
    Response {
        status,
        reason: if status < 400 { "OK" } else { "Error" }.to_string(),
        content_type: content_type.to_string(),
        body,
        url: url.to_string(),
        retry_after: None,
    }
}

fn too_many(retry_after: Option<&str>, url: &str) -> Response {
    Response {
        retry_after: retry_after.map(str::to_string),
        ..answer(429, "text/html", Vec::new(), url)
    }
}

impl Transport for Web {
    fn get(&self, url: &str, accept: Option<&str>) -> Result<Response, String> {
        let mut web = self.0.lock().unwrap();
        if let Some(rest) = url.strip_prefix("https://api.openalex.org/works/doi:") {
            web.requests.push(("openalex".into(), url.into()));
            let doi = rest.split('?').next().unwrap();
            return Ok(match web.openalex.get(doi) {
                Some(locations) => answer(
                    200,
                    "application/json",
                    json!({"id": "https://openalex.org/W1", "locations": locations})
                        .to_string()
                        .into_bytes(),
                    url,
                ),
                None => answer(
                    404,
                    "application/json",
                    br#"{"error": "not found"}"#.to_vec(),
                    url,
                ),
            });
        }
        if let Some(doi) = url.strip_prefix("https://api.crossref.org/works/") {
            web.requests.push(("crossref".into(), url.into()));
            return Ok(match web.crossref.get(doi) {
                Some(record) => answer(
                    200,
                    "application/json",
                    json!({"message": record}).to_string().into_bytes(),
                    url,
                ),
                None => answer(404, "text/plain", b"Resource not found.".to_vec(), url),
            });
        }
        if accept == Some(PDF_ACCEPT) {
            web.requests.push(("file".into(), url.into()));
            let file = match web.files.get_mut(url) {
                Some(File::Answers(answers)) if answers.len() > 1 => answers.remove(0),
                Some(File::Answers(answers)) => answers[0].clone(),
                Some(file) => file.clone(),
                None => File::Status(404),
            };
            return Ok(match file {
                File::Status(code) => answer(code, "text/html", Vec::new(), url),
                File::TooMany(retry_after) => too_many(retry_after, url),
                File::Bytes(bytes) if bytes.starts_with(b"%PDF") => {
                    answer(200, "application/pdf", bytes, url)
                }
                File::Bytes(bytes) => answer(200, "text/html", bytes, url),
                File::Answers(_) => unreachable!("answers in answers"),
            });
        }
        web.requests.push(("page".into(), url.into()));
        if let Some(retry_after) = web.busy.remove(url) {
            return Ok(too_many(retry_after, url));
        }
        Ok(match web.pages.get(url) {
            Some((last, html)) => answer(200, "text/html", html.clone().into_bytes(), last),
            None => answer(404, "text/html", Vec::new(), url),
        })
    }
}

impl Web {
    /// The article page of the DOI, with its PDF.
    fn publisher(
        &self,
        doi: &str,
        pdf: Option<&str>,
        content: Option<Vec<u8>>,
        links: &[(&str, &str)],
        meta: &[&str],
    ) {
        let mut web = self.0.lock().unwrap();
        web.pages.insert(
            format!("https://doi.org/{doi}"),
            (format!("{PAGE}{doi}"), page(pdf, links, meta)),
        );
        if let Some(pdf) = pdf {
            web.files.insert(
                pdf.to_string(),
                File::Bytes(content.unwrap_or_else(|| pdf_bytes(pdf))),
            );
        }
    }

    fn file(&self, url: &str, file: File) {
        self.0.lock().unwrap().files.insert(url.to_string(), file);
    }

    fn openalex(&self, doi: &str, locations: Vec<Value>) {
        self.0
            .lock()
            .unwrap()
            .openalex
            .insert(doi.to_string(), locations);
    }

    fn downloads(&self) -> Vec<String> {
        self.requests("file")
    }

    fn requests(&self, kind: &str) -> Vec<String> {
        self.0
            .lock()
            .unwrap()
            .requests
            .iter()
            .filter(|(k, _)| k == kind)
            .map(|(_, u)| u.clone())
            .collect()
    }

    fn clear(&self) {
        self.0.lock().unwrap().requests.clear();
    }
}

struct Sandbox {
    _tmp: tempfile::TempDir,
    root: String,
    web: Web,
    fetcher: Fetcher,
    summary: Mutex<Option<Vec<String>>>,
}

fn entry(key: &str, files: &[&str], doi: Option<&str>) -> String {
    let file: Vec<String> = files.iter().map(|f| format!(":{f}:pdf")).collect();
    format!(
        "@article{{{key},\n author = {{Some One}},\n{}{} title = {{Title of {key}}},\n year = {{2000}}\n}}\n\n",
        doi.map(|d| format!(" doi = {{{d}}},\n"))
            .unwrap_or_default(),
        if files.is_empty() {
            String::new()
        } else {
            format!(" file = {{{}}},\n", file.join(";"))
        }
    )
}

thread_local! {
    /// The waits after an HTTP 429 of the test (`time.sleep` mocked).
    static SLEPT: std::cell::RefCell<Vec<Duration>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn record_sleep(duration: Duration) {
    SLEPT.with(|slept| slept.borrow_mut().push(duration));
}

fn slept() -> Vec<Duration> {
    SLEPT.with(|slept| slept.borrow().clone())
}

fn sandbox() -> Sandbox {
    freepdf::set_interval(Duration::ZERO);
    freepdf::set_sleep(record_sleep);
    let tmp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(tmp.path())
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let web = Web::default();
    let cache = format!("{root}/cache");
    let fetcher = Fetcher::with_transport(Box::new(web.clone()), &cache);
    let s = Sandbox {
        _tmp: tmp,
        root,
        web,
        fetcher,
        summary: Mutex::new(None),
    };
    s.write(
        "lib/.papers/config.json",
        "{\n  \"bibtex\": \"papers.bib\",\n  \"filesdir\": \"files\",\n  \"pool\": \"none\",\n  \"version\": 3\n}",
    );
    fs::create_dir_all(s.path("lib/files")).unwrap();
    s.bib(&[]);
    s
}

impl Sandbox {
    fn path(&self, rel: &str) -> String {
        format!("{}/{rel}", self.root)
    }

    fn write(&self, rel: &str, text: &str) -> String {
        let path = self.path(rel);
        fs::create_dir_all(Path::new(&path).parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
    }

    fn bib(&self, entries: &[String]) {
        fs::write(self.path("lib/papers.bib"), entries.concat()).unwrap();
    }

    fn bibtext(&self) -> String {
        fs::read_to_string(self.path("lib/papers.bib")).unwrap()
    }

    /// The files setting of the library (the tests of papers link by default).
    fn files_setting(&self, mode: &str) {
        let path = self.path("lib/.papers/config.json");
        let text = fs::read_to_string(&path).unwrap();
        fs::write(
            &path,
            text.replace(
                "\"filesdir\"",
                &format!("\"files\": \"{mode}\",\n  \"filesdir\""),
            ),
        )
        .unwrap();
    }

    fn config(&self) -> Config {
        self.config_of("lib")
    }

    /// The configuration of the library of the folder `dir`, with the history of the sandbox.
    fn config_of(&self, dir: &str) -> Config {
        let mut machine = Map::new();
        machine.insert("history-dir".into(), json!(self.path("history")));
        machine.insert("files".into(), json!("link"));
        Config::load(
            &self.path(&format!("{dir}/.papers/config.json")),
            MachineConfig::new(None, machine),
            None,
        )
        .unwrap()
    }

    fn session(&self, dry_run: bool) -> Session {
        self.session_with(dry_run, Box::new(papers_rs::command::NoPrompt))
    }

    fn session_with(&self, dry_run: bool, prompt: Box<dyn Prompt>) -> Session {
        Session::open(
            self.config(),
            Options {
                message: "papers file fetch".to_string(),
                dry_run,
                prompt,
                ..Options::default()
            },
        )
        .unwrap()
    }

    /// `papers file fetch` of these keys (`--missing` without keys: all of them): the lines, and
    /// the number of entries without PDF.
    fn fetch(
        &self,
        keys: &[&str],
        missing: bool,
        pdf: PdfOptions,
        dry_run: bool,
    ) -> (Vec<String>, usize) {
        self.fetch_with(keys, missing, pdf, &[], dry_run)
    }

    /// [`Sandbox::fetch`] with the folders of `--search`.
    fn fetch_with(
        &self,
        keys: &[&str],
        missing: bool,
        pdf: PdfOptions,
        search: &[&str],
        dry_run: bool,
    ) -> (Vec<String>, usize) {
        let mut session = self.session(dry_run);
        let keys: Vec<String> = if keys.is_empty() {
            session.library.entries().map(|e| e.key.clone()).collect()
        } else {
            keys.iter().map(|k| k.to_string()).collect()
        };
        let request = FileFetch {
            keys,
            missing,
            pdf,
            search: search.iter().map(|d| d.to_string()).collect(),
            yes: false,
        };
        let fetched = file_fetch(&mut session, &self.fetcher, &request).unwrap();
        self.summary.lock().unwrap().clone_from(&fetched.summary());
        let Fetched { failed, .. } = fetched;
        (session.finish().out, failed)
    }

    /// The summary of the last [`Sandbox::fetch`] (none for one entry or a dry run).
    fn summary(&self) -> Option<Vec<String>> {
        self.summary.lock().unwrap().clone()
    }

    fn out(&self, keys: &[&str]) -> Vec<String> {
        self.fetch(keys, false, PdfOptions::default(), false).0
    }

    fn files(&self, key: &str) -> Vec<String> {
        Session::open(self.config(), Options::default())
            .unwrap()
            .library
            .files(key)
    }

    fn libfiles(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.path("lib/files"))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn publisher(&self) {
        self.web.publisher(DOI, Some(PDF), None, &[], &[]);
    }
}

fn joined(lines: &[String]) -> String {
    lines.join("\n")
}

fn options(f: impl FnOnce(&mut PdfOptions)) -> PdfOptions {
    let mut options = PdfOptions::default();
    f(&mut options);
    options
}

// TestVersions
// ------------

#[test]
fn published_version_from_the_publisher() {
    let s = sandbox();
    s.bib(&[entry("smith2020", &[], Some(DOI))]);
    s.publisher();
    let out = s.out(&["smith2020"]);
    let files = s.files("smith2020");
    assert_eq!(files.len(), 1);
    assert!(files[0].starts_with(&s.path("lib/files/")));
    assert_eq!(fs::read(&files[0]).unwrap(), pdf_bytes(PDF));
    let name = Path::new(&files[0])
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(
        joined(&out).contains("(published version, from journals.example.org)"),
        "{out:?}"
    );
    assert!(joined(&out).contains(&name));
    // OpenAlex is not needed
    assert!(s.web.requests("openalex").is_empty());
}

#[test]
fn the_publisher_refuses_then_openalex_published_before_accepted() {
    let s = sandbox();
    s.bib(&[entry("smith2020", &[], Some(DOI))]);
    s.web.publisher(
        DOI,
        Some(PDF),
        Some(b"<html>login</html>".to_vec()),
        &[],
        &[],
    );
    s.web.openalex(
        DOI,
        vec![
            location(
                "https://repo.example.edu/am.pdf",
                "acceptedVersion",
                None,
                true,
            ),
            location(
                "https://pmc.example.gov/vor.pdf",
                "publishedVersion",
                None,
                true,
            ),
            location(
                "https://closed.example.com/x.pdf",
                "publishedVersion",
                None,
                false,
            ),
        ],
    );
    s.web.file(
        "https://repo.example.edu/am.pdf",
        File::Bytes(pdf_bytes("am")),
    );
    s.web.file(
        "https://pmc.example.gov/vor.pdf",
        File::Bytes(pdf_bytes("vor")),
    );
    let out = s.out(&["smith2020"]);
    assert!(
        joined(&out).contains("(published version, from pmc.example.gov)"),
        "{out:?}"
    );
    assert_eq!(
        fs::read(&s.files("smith2020")[0]).unwrap(),
        pdf_bytes("vor")
    );
    assert_eq!(s.web.downloads(), [PDF, "https://pmc.example.gov/vor.pdf"]);
}

#[test]
fn accepted_then_submitted() {
    let s = sandbox();
    s.bib(&[entry("smith2020", &[], Some(DOI))]);
    s.web.publisher(DOI, None, None, &[], &[]);
    s.web.openalex(
        DOI,
        vec![
            location(
                "https://preprints.example.org/s.pdf",
                "submittedVersion",
                None,
                true,
            ),
            location(
                "https://repo.example.edu/am.pdf",
                "acceptedVersion",
                None,
                true,
            ),
        ],
    );
    s.web
        .file("https://repo.example.edu/am.pdf", File::Status(403));
    s.web.file(
        "https://preprints.example.org/s.pdf",
        File::Bytes(pdf_bytes("s")),
    );
    let out = s.out(&["smith2020"]);
    assert!(joined(&out).contains("(submitted version, from preprints.example.org)"));
    assert_eq!(
        s.web.downloads(),
        [
            "https://repo.example.edu/am.pdf",
            "https://preprints.example.org/s.pdf"
        ]
    );
}

#[test]
fn version_published_refuses_the_others() {
    let s = sandbox();
    s.bib(&[entry("smith2020", &[], Some(DOI))]);
    s.web.publisher(DOI, None, None, &[], &[]);
    s.web.openalex(
        DOI,
        vec![location(
            "https://repo.example.edu/am.pdf",
            "acceptedVersion",
            Some("https://repo.example.edu/am"),
            true,
        )],
    );
    s.web.file(
        "https://repo.example.edu/am.pdf",
        File::Bytes(pdf_bytes("am")),
    );
    let (_, failed) = s.fetch(
        &["smith2020"],
        false,
        options(|o| o.least = Least::Published),
        false,
    );
    assert_eq!(failed, 1);
    assert!(s.files("smith2020").is_empty());
    assert!(s.web.downloads().is_empty());
    // the message of the error
    let mut search = Search::new(
        &s.fetcher,
        &OpenAlex::default(),
        Some(DOI),
        None,
        Least::Published,
    );
    let message = fetch_pdf(&mut search, None).unwrap_err().message;
    assert!(message.starts_with("no free PDF found"), "{message}");
    assert!(
        message.contains(
            "open in a browser: https://doi.org/10.1000/ocean, https://repo.example.edu/am"
        ),
        "{message}"
    );
}

#[test]
fn none_works_the_links_to_open_and_the_others_attached() {
    let s = sandbox();
    s.bib(&[
        entry("smith2020", &[], Some(DOI)),
        entry("other2020", &[], Some("10.1000/other")),
    ]);
    s.web.publisher(
        DOI,
        Some(PDF),
        Some(b"<html>a challenge</html>".to_vec()),
        &[],
        &[],
    );
    s.web.openalex(DOI, vec![]);
    s.web.publisher(
        "10.1000/other",
        Some("https://journals.example.org/other.pdf"),
        None,
        &[],
        &[],
    );
    let (_, failed) = s.fetch(
        &["smith2020", "other2020"],
        false,
        PdfOptions::default(),
        false,
    );
    assert_eq!(failed, 1);
    assert!(s.files("smith2020").is_empty());
    assert_eq!(s.files("other2020").len(), 1); // saved
    let mut search = Search::new(
        &s.fetcher,
        &OpenAlex::default(),
        Some(DOI),
        None,
        Least::Any,
    );
    let message = fetch_pdf(&mut search, None).unwrap_err().message;
    assert!(
        message.contains(&format!("{PDF} (published version): not a PDF")),
        "{message}"
    );
    assert!(
        message.contains(&format!("open in a browser: {PDF}, https://doi.org/{DOI}")),
        "{message}"
    );
    // several entries: one line for the entry without PDF (on the standard error), not an error
    assert_eq!(
        one_line(&message),
        format!(
            "{PDF} (published version): not a PDF; OpenAlex: no open-access PDF; open {PDF}, https://doi.org/{DOI} in a browser"
        )
    );
}

/// What a program gets of a search that gave no PDF (`first_pdf`, `browser_links`; not in papers):
/// the links tried with their error, the links to open as the message has them.
#[test]
fn the_links_tried_and_those_to_open() {
    let s = sandbox();
    s.web.publisher(
        DOI,
        Some(PDF),
        Some(b"<html>a challenge</html>".to_vec()),
        &[],
        &[],
    );
    s.web.openalex(DOI, vec![]);
    let mut search = Search::new(
        &s.fetcher,
        &OpenAlex::default(),
        Some(DOI),
        None,
        Least::Any,
    );
    let tried = first_pdf(&mut search, None).unwrap_err();
    assert_eq!(tried.len(), 1);
    assert_eq!(tried[0].0.url, PDF);
    assert_eq!(tried[0].0.version, Version::Published);
    assert_eq!(freepdf::reason(&tried[0].1), "not a PDF");
    assert_eq!(
        browser_links(&search, &tried),
        [PDF.to_string(), format!("https://doi.org/{DOI}")]
    );
}

/// library.md "The free PDF of an entry": an accepted version attached after the published one
/// was refused: the message gives the published link to open in a browser.
#[test]
fn published_refused_gives_its_link() {
    let s = sandbox();
    s.bib(&[entry("smith2020", &[], Some(DOI))]);
    s.web.publisher(DOI, Some(PDF), None, &[], &[]);
    s.web.file(PDF, File::Status(403));
    s.web.openalex(
        DOI,
        vec![location(
            "https://repo.example.edu/am.pdf",
            "acceptedVersion",
            None,
            true,
        )],
    );
    s.web.file(
        "https://repo.example.edu/am.pdf",
        File::Bytes(pdf_bytes("am")),
    );
    let out = s.out(&["smith2020"]);
    let files = s.files("smith2020");
    let name = Path::new(&files[0])
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(
        joined(&out).contains(&format!(
            "/files/{name} (accepted manuscript, from repo.example.edu; the published version was refused: open {PDF} in a browser)"
        )),
        "{out:?}"
    );
}

/// The publisher's page itself refused (no PDF link): the page is the link to open.
#[test]
fn publisher_page_refused_gives_its_link() {
    let s = sandbox();
    s.bib(&["@article{arx2021,\n doi = {10.1000/ocean},\n eprint = {2101.01234},\n title = {T},\n year = {2021}\n}\n".to_string()]);
    s.web.openalex(DOI, vec![]);
    s.web.file(
        "https://arxiv.org/pdf/2101.01234",
        File::Bytes(pdf_bytes("arxiv")),
    );
    let out = s.out(&["arx2021"]);
    assert!(
        joined(&out).contains(&format!(
            "(submitted version, from arxiv.org; the published version was refused: open https://doi.org/{DOI} in a browser)"
        )),
        "{out:?}"
    );
}

/// The page gave no PDF link and OpenAlex no published location: nothing was refused.
#[test]
fn no_published_link_says_nothing() {
    let s = sandbox();
    s.bib(&[entry("smith2020", &[], Some(DOI))]);
    s.web.publisher(DOI, None, None, &[], &[]);
    s.web.openalex(
        DOI,
        vec![location(
            "https://repo.example.edu/am.pdf",
            "acceptedVersion",
            None,
            true,
        )],
    );
    s.web.file(
        "https://repo.example.edu/am.pdf",
        File::Bytes(pdf_bytes("am")),
    );
    let out = joined(&s.out(&["smith2020"]));
    assert!(
        out.contains("(accepted manuscript, from repo.example.edu)"),
        "{out}"
    );
    assert!(!out.contains("refused"), "{out}");
}

#[test]
fn arxiv() {
    let s = sandbox();
    s.bib(&["@article{arx2021,\n eprint = {2101.01234},\n archiveprefix = {arXiv},\n title = {T},\n year = {2021}\n}\n".to_string()]);
    s.web.file(
        "https://arxiv.org/pdf/2101.01234",
        File::Bytes(pdf_bytes("arxiv")),
    );
    let out = s.out(&["arx2021"]);
    assert!(
        joined(&out).contains("(submitted version, from arxiv.org)"),
        "{out:?}"
    );
    assert_eq!(
        fs::read(&s.files("arx2021")[0]).unwrap(),
        pdf_bytes("arxiv")
    );
    assert_eq!(s.web.downloads(), ["https://arxiv.org/pdf/2101.01234"]);
}

#[test]
fn arxiv_after_the_doi() {
    let s = sandbox();
    s.bib(&["@article{arx2021,\n doi = {10.1000/ocean},\n eprint = {2101.01234},\n title = {T},\n year = {2021}\n}\n".to_string()]);
    s.web.publisher(DOI, None, None, &[], &[]);
    s.web.openalex(DOI, vec![]);
    s.web.file(
        "https://arxiv.org/pdf/2101.01234",
        File::Bytes(pdf_bytes("arxiv")),
    );
    assert!(joined(&s.out(&["arx2021"])).contains("(submitted version, from arxiv.org)"));
    assert_eq!(s.web.downloads(), ["https://arxiv.org/pdf/2101.01234"]);
}

#[test]
fn an_arxiv_doi_is_the_paper_of_arxiv() {
    let s = sandbox();
    s.web.file(
        "https://arxiv.org/pdf/2101.01234",
        File::Bytes(pdf_bytes("arxiv")),
    );
    let mut search = Search::new(
        &s.fetcher,
        &OpenAlex::default(),
        Some("10.48550/arXiv.2101.01234"),
        None,
        Least::Any,
    );
    let found = fetch_pdf(&mut search, None).unwrap();
    assert_eq!(found.link.describe(), "submitted version, from arxiv.org");
    assert!(s.web.requests("page").is_empty());
    freepdf::discard(&found.path);
}

#[test]
fn no_identifier() {
    let s = sandbox();
    s.bib(&[entry("nodoi2000", &[], None)]);
    let (_, failed) = s.fetch(&["nodoi2000"], false, PdfOptions::default(), false);
    assert_eq!(failed, 1);
    let mut search = Search::new(&s.fetcher, &OpenAlex::default(), None, None, Least::Any);
    assert_eq!(
        fetch_pdf(&mut search, None).unwrap_err().message,
        "no DOI or arXiv id to look for a PDF"
    );
}

#[test]
fn openalex_not_found_is_cached() {
    let s = sandbox();
    s.bib(&[entry("smith2020", &[], Some(DOI))]);
    s.web.publisher(DOI, None, None, &[], &[]);
    for _ in 0..2 {
        assert_eq!(
            s.fetch(&["smith2020"], false, PdfOptions::default(), false)
                .1,
            1
        );
    }
    assert_eq!(s.web.requests("openalex").len(), 1);
}

#[test]
fn the_openalex_locations_are_asked_again_after_a_month() {
    let s = sandbox();
    s.web.openalex(
        DOI,
        vec![location(
            "https://x.example.org/a.pdf",
            "publishedVersion",
            None,
            true,
        )],
    );
    s.fetcher.work_locations(DOI, &OpenAlex::default()).unwrap();
    s.fetcher.work_locations(DOI, &OpenAlex::default()).unwrap();
    assert_eq!(s.web.requests("openalex").len(), 1);
    // stored 31 days ago
    let file = s.path("cache/openalex-work/10.1000/ocean.json");
    let mut data: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    let time = data["time"].as_f64().unwrap();
    data["time"] = json!(time - 31.0 * 24.0 * 3600.0);
    fs::write(&file, data.to_string()).unwrap();
    s.fetcher.work_locations(DOI, &OpenAlex::default()).unwrap();
    assert_eq!(s.web.requests("openalex").len(), 2);
}

// TestSelection
// -------------

fn selection() -> Sandbox {
    let s = sandbox();
    s.write("lib/files/a.pdf", "a.pdf");
    s.bib(&[
        entry("has2000", &["files/a.pdf"], Some("10.1000/has")),
        entry("smith2020", &[], Some(DOI)),
    ]);
    s.publisher();
    s
}

#[test]
fn missing() {
    let s = selection();
    let (out, _) = s.fetch(&[], true, PdfOptions::default(), false);
    assert_eq!(s.files("has2000"), [s.path("lib/files/a.pdf")]);
    assert_eq!(s.files("smith2020").len(), 1);
    assert_eq!(s.web.downloads(), [PDF]);
    assert!(!joined(&out).contains("has2000"));
}

#[test]
fn a_dry_run_changes_nothing() {
    let s = selection();
    let before = s.bibtext();
    let (out, _) = s.fetch(&[], true, PdfOptions::default(), true);
    assert!(
        out.contains(&format!(
            "DRYRUN: smith2020: would download {PDF} (published version, from journals.example.org)"
        )),
        "{out:?}"
    );
    assert_eq!(s.bibtext(), before);
    assert!(s.web.downloads().is_empty());
    assert_eq!(s.libfiles(), ["a.pdf"]);
}

#[test]
fn has_a_pdf_already() {
    let s = selection();
    s.web.publisher(
        "10.1000/has",
        Some("https://journals.example.org/has.pdf"),
        None,
        &[],
        &[],
    );
    let out = s.out(&["has2000"]);
    assert!(out.contains(&"has2000: has a PDF already (--again to fetch it anyway)".to_string()));
    assert!(s.web.downloads().is_empty());
    s.fetch(&["has2000"], false, options(|o| o.again = true), false);
    assert_eq!(s.files("has2000").len(), 2);
}

#[test]
fn a_broken_link_is_no_pdf() {
    let s = selection();
    fs::remove_file(s.path("lib/files/a.pdf")).unwrap();
    s.web.publisher(
        "10.1000/has",
        Some("https://journals.example.org/has.pdf"),
        None,
        &[],
        &[],
    );
    let (out, _) = s.fetch(&[], true, PdfOptions::default(), false);
    assert!(!joined(&out).contains("has a PDF already"));
    assert!(
        s.web
            .downloads()
            .contains(&"https://journals.example.org/has.pdf".to_string())
    );
}

#[test]
fn undo_and_redo() {
    let s = selection();
    s.out(&["smith2020"]);
    assert_eq!(s.files("smith2020").len(), 1);
    let mut config = s.config();
    History::require(&mut config, false)
        .unwrap()
        .undo(
            1,
            false,
            None,
            "papers undo",
            &mut Resolver::new(OnDuplicate::Skip),
        )
        .unwrap();
    assert!(s.files("smith2020").is_empty());
    let mut config = s.config();
    History::require(&mut config, false)
        .unwrap()
        .redo(
            1,
            false,
            None,
            "papers redo",
            &mut Resolver::new(OnDuplicate::Skip),
        )
        .unwrap();
    assert_eq!(s.files("smith2020").len(), 1);
}

// TestSummary: library.md "The free PDF of an entry", a fetch of several entries ends with a
// summary
// -----------------------------------------------------------------------------------------------

fn several() -> Sandbox {
    let s = sandbox();
    s.write("lib/files/a.pdf", "a.pdf");
    s.write("downloads/old.pdf", "pdf old");
    s.bib(&[
        entry("pub2020", &[], Some("10.1000/pub")),
        entry("acc2020", &[], Some("10.1000/acc")),
        entry("none2020", &[], Some("10.1000/none")),
        entry("old2020", &["files/old.pdf"], Some("10.1000/old")),
        entry("has2000", &["files/a.pdf"], Some("10.1000/has")),
    ]);
    s.web.publisher(
        "10.1000/pub",
        Some("https://journals.example.org/pub.pdf"),
        None,
        &[],
        &[],
    );
    s.web.publisher(
        "10.1000/acc",
        Some("https://journals.example.org/acc.pdf"),
        None,
        &[],
        &[],
    );
    s.web
        .file("https://journals.example.org/acc.pdf", File::Status(403));
    s.web.openalex(
        "10.1000/acc",
        vec![location(
            "https://repo.example.edu/acc.pdf",
            "acceptedVersion",
            None,
            true,
        )],
    );
    s.web.file(
        "https://repo.example.edu/acc.pdf",
        File::Bytes(pdf_bytes("acc")),
    );
    s.web.publisher(
        "10.1000/none",
        Some("https://journals.example.org/none.pdf"),
        Some(b"<html></html>".to_vec()),
        &[],
        &[],
    );
    s.web.openalex("10.1000/none", vec![]);
    s
}

#[test]
fn summary_of_missing() {
    let s = several();
    let downloads = s.path("downloads");
    let (_, failed) = s.fetch_with(&[], true, PdfOptions::default(), &[&downloads], false);
    assert_eq!(failed, 1);
    assert_eq!(
        s.summary().unwrap(),
        [
            "summary of 4 entries:",
            "  2 PDFs attached: 1 published version, 1 accepted manuscript",
            "  1 found locally (repaired, not downloaded)",
            "  1 without PDF",
            "open in a browser:",
            "  acc2020: https://journals.example.org/acc.pdf (published version refused; accepted manuscript found)",
            "  none2020: https://journals.example.org/none.pdf, https://doi.org/10.1000/none (no PDF)",
        ]
    );
}

#[test]
fn no_summary_for_one_entry_or_a_dry_run() {
    let s = several();
    s.out(&["pub2020"]);
    assert_eq!(s.summary(), None);
    let downloads = s.path("downloads");
    s.fetch_with(&[], true, PdfOptions::default(), &[&downloads], true);
    assert_eq!(s.summary(), None);
}

/// The accepted manuscript found is the file the entry has: counted once, as had, and not listed
/// as refused (`test_refused_but_not_attached`).
#[test]
fn summary_refused_but_not_attached() {
    let s = several();
    fs::write(s.path("lib/files/acc.pdf"), pdf_bytes("acc")).unwrap();
    s.bib(&[
        entry("pub2020", &[], Some("10.1000/pub")),
        entry("acc2020", &["files/acc.pdf"], Some("10.1000/acc")),
    ]);
    let (out, _) = s.fetch(
        &["pub2020", "acc2020"],
        false,
        options(|o| o.again = true),
        false,
    );
    assert!(
        joined(&out).contains(
            "acc2020: https://repo.example.edu/acc.pdf: not attached, the entry has the same file"
        ),
        "{out:?}"
    );
    assert_eq!(
        s.summary().unwrap(),
        [
            "summary of 2 entries:",
            "  1 PDF attached: 1 published version",
            "  1 had the PDF already",
        ]
    );
}

/// The other lines of the summary: the entries that had the PDF, the supplements, nothing attached.
#[test]
fn summary_lines() {
    let summary = FetchSummary {
        had: 2,
        supplements: 1,
        attached: [(Version::Submitted, 1)].into_iter().collect(),
        ..FetchSummary::default()
    };
    assert_eq!(
        summary.lines(3),
        [
            "summary of 3 entries:",
            "  1 PDF attached: 1 submitted version",
            "  2 had the PDF already",
            "  1 supplement attached",
        ]
    );
    assert_eq!(
        FetchSummary::default().lines(2),
        ["summary of 2 entries:", "  nothing attached"]
    );
}

// TestBrokenLinkRepair
// --------------------

/// smith2020 links to lib/files/old.pdf, which is gone; downloads/old.pdf has its name.
fn broken() -> (Sandbox, String, String) {
    let s = sandbox();
    s.bib(&[entry("smith2020", &["files/old.pdf"], Some(DOI))]);
    s.publisher();
    let found = s.write("downloads/old.pdf", "pdf old");
    let old = s.path("lib/files/old.pdf");
    (s, old, found)
}

fn repair(
    s: &Sandbox,
    keys: &[&str],
    missing: bool,
    pdf: PdfOptions,
    dry_run: bool,
) -> Vec<String> {
    let downloads = s.path("downloads");
    s.fetch_with(keys, missing, pdf, &[downloads.as_str()], dry_run)
        .0
}

#[test]
fn found_in_a_search_folder() {
    // the files setting of the tests is link: linked where it is, as file fix does
    let (s, _, found) = broken();
    let out = repair(&s, &["smith2020"], false, PdfOptions::default(), false);
    assert!(
        out.contains(&format!(
            "smith2020: relinked {found} (found in {}), not downloaded",
            s.path("downloads")
        )),
        "{out:?}"
    );
    assert_eq!(s.files("smith2020"), [found]);
    assert!(s.web.downloads().is_empty());
}

#[test]
fn copied_into_the_files_directory() {
    let (s, _, found) = broken();
    s.files_setting("copy");
    let out = repair(&s, &["smith2020"], false, PdfOptions::default(), false);
    let file = s.files("smith2020").remove(0);
    assert!(
        joined(&out).contains(&format!("smith2020: relinked {file} (found in ")),
        "{out:?}"
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "pdf old");
    assert_eq!(
        Path::new(&file).parent().unwrap(),
        Path::new(&s.path("lib/files"))
    );
    assert!(Path::new(&found).exists());
    assert!(s.web.downloads().is_empty());
}

#[test]
fn missing_with_a_search_folder() {
    let (s, _, _) = broken();
    let out = repair(&s, &[], true, PdfOptions::default(), false);
    assert!(joined(&out).contains("not downloaded"), "{out:?}");
    assert!(s.web.downloads().is_empty());
}

#[test]
fn restored_by_checksum() {
    // deleted outside papers: its checksum in files.json, a copy of the same content in the history
    let (s, old, _) = broken();
    fs::write(&old, "pdf a").unwrap();
    let mut session = s.session(false);
    checksums(&mut session, &["smith2020".to_string()], false).unwrap();
    session.finish();
    let copy = s.write("copy.pdf", "pdf a");
    let digest = papers_rs::merge::checksum_hex(&old).unwrap();
    papers_rs::history::Store::new(s.path("history"))
        .put_file(&copy, &digest)
        .unwrap();
    fs::remove_file(&old).unwrap();
    let out = s.out(&["smith2020"]);
    assert!(
        out.contains(&format!(
            "smith2020: {old}: restored from the history (deleted, its checksum known), not downloaded"
        )),
        "{out:?}"
    );
    assert_eq!(fs::read_to_string(&old).unwrap(), "pdf a");
    assert_eq!(s.files("smith2020"), [old]);
    assert!(s.web.downloads().is_empty());
}

#[test]
fn not_found_downloads() {
    let (s, _, found) = broken();
    fs::remove_file(&found).unwrap();
    let out = repair(&s, &["smith2020"], false, PdfOptions::default(), false);
    assert!(!joined(&out).contains("relinked"));
    assert_eq!(s.web.downloads(), [PDF]);
    // the broken link stays, as file fix leaves it
    assert_eq!(s.files("smith2020").len(), 2);
}

#[test]
fn a_dry_run_of_the_repair() {
    let (s, old, found) = broken();
    let before = s.bibtext();
    let out = repair(&s, &["smith2020"], false, PdfOptions::default(), true);
    assert!(
        out.contains(&format!(
            "DRYRUN: smith2020: would relink {old} -> {found} (found in {})",
            s.path("downloads")
        )),
        "{out:?}"
    );
    assert!(!joined(&out).contains("would download"));
    assert_eq!(s.bibtext(), before);
    assert!(s.libfiles().is_empty());
}

#[test]
fn again_downloads_too() {
    let (s, _, _) = broken();
    let out = repair(
        &s,
        &["smith2020"],
        false,
        options(|o| o.again = true),
        false,
    );
    assert!(!joined(&out).contains("not downloaded"));
    assert_eq!(s.web.downloads(), [PDF]);
    assert_eq!(s.files("smith2020").len(), 2);
}

#[test]
fn one_step_with_the_downloads() {
    let (s, old, found) = broken();
    s.bib(&[
        entry("smith2020", &["files/old.pdf"], Some(DOI)),
        entry("other2001", &["files/gone.pdf"], Some("10.1000/b")),
    ]);
    s.web.publisher(
        "10.1000/b",
        Some("https://journals.example.org/b.pdf"),
        None,
        &[],
        &[],
    );
    repair(
        &s,
        &["smith2020", "other2001"],
        false,
        PdfOptions::default(),
        false,
    );
    assert_eq!(s.web.downloads(), ["https://journals.example.org/b.pdf"]);
    assert_eq!(s.files("smith2020"), std::slice::from_ref(&found));
    assert_eq!(s.libfiles().len(), 1);
    // the relink and the download together
    let mut config = s.config();
    History::require(&mut config, false)
        .unwrap()
        .undo(
            1,
            false,
            None,
            "papers undo",
            &mut Resolver::new(OnDuplicate::Skip),
        )
        .unwrap();
    assert_eq!(s.files("smith2020"), [old]);
    assert_eq!(s.files("other2001"), [s.path("lib/files/gone.pdf")]);
    let mut config = s.config();
    History::require(&mut config, false)
        .unwrap()
        .redo(
            1,
            false,
            None,
            "papers redo",
            &mut Resolver::new(OnDuplicate::Skip),
        )
        .unwrap();
    assert_eq!(s.files("smith2020"), [found]);
    assert_eq!(s.files("other2001").len(), 2);
}

// TestConfirmation
// ----------------

/// A terminal that answers.
struct Answers(Vec<String>, Arc<Mutex<Vec<String>>>);

impl Prompt for Answers {
    fn interactive(&self) -> bool {
        true
    }

    fn input(&mut self, prompt: &str) -> Option<String> {
        self.1.lock().unwrap().push(prompt.to_string());
        (!self.0.is_empty()).then(|| self.0.remove(0))
    }
}

fn many() -> (Sandbox, Vec<String>) {
    let s = sandbox();
    let entries: Vec<String> = (0..51)
        .map(|i| entry(&format!("e{i}x"), &[], Some(&format!("10.1000/e{i}"))))
        .collect();
    s.bib(&entries);
    let keys = (0..51).map(|i| format!("e{i}x")).collect();
    (s, keys)
}

#[test]
fn no_terminal_refuses() {
    let (s, keys) = many();
    let session = s.session(false);
    let error = confirm_openalex(&session, &keys, false, &OpenAlex::default()).unwrap_err();
    assert!(
        error.message().starts_with("51 entries with a DOI"),
        "{}",
        error.message()
    );
    assert!(error.message().contains("OPENALEX_API_KEY, pass -y/--yes"));
}

#[test]
fn asks_in_a_terminal() {
    let (s, keys) = many();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let session = s.session_with(false, Box::new(Answers(vec!["n".into()], asked.clone())));
    let error = confirm_openalex(&session, &keys, false, &OpenAlex::default()).unwrap_err();
    assert_eq!(error.message(), "nothing was changed");
    drop(session);
    let session = s.session_with(false, Box::new(Answers(vec!["y".into()], asked.clone())));
    confirm_openalex(&session, &keys, false, &OpenAlex::default()).unwrap();
    assert!(asked.lock().unwrap()[0].ends_with(". Continue? [y/N] "));
}

#[test]
fn yes_key_or_fewer_entries() {
    let (s, keys) = many();
    let session = s.session(false);
    confirm_openalex(&session, &keys, true, &OpenAlex::default()).unwrap();
    let key = OpenAlex {
        api_key: Some("key".into()),
        mailto: None,
    };
    confirm_openalex(&session, &keys, false, &key).unwrap();
    confirm_openalex(&session, &keys[..50], false, &OpenAlex::default()).unwrap();
}

// TestSupplements
// ---------------

const SUPPL: &str = "https://journals.example.org/article/ocean-supplement.pdf";
const TAGGED: &str = "https://journals.example.org/files/si-tables.pdf";

fn supplements() -> Sandbox {
    let s = sandbox();
    s.bib(&[entry("smith2020", &[], Some(DOI))]);
    s.web.publisher(
        DOI,
        Some(PDF),
        None,
        &[
            (SUPPL, "Supplement"),
            ("https://journals.example.org/data.zip", "Supplement"),
            ("https://journals.example.org/other.pdf", "Other paper"),
            (
                "https://journals.example.org/s2.pdf",
                "Supplementary material",
            ),
        ],
        &[TAGGED],
    );
    for url in [SUPPL, TAGGED] {
        s.web.file(url, File::Bytes(pdf_bytes(url)));
    }
    s.web.file(
        "https://journals.example.org/s2.pdf",
        File::Bytes(b"<html>login</html>".to_vec()),
    );
    s
}

#[test]
fn the_supplements() {
    let s = supplements();
    let (out, _) = s.fetch(
        &["smith2020"],
        false,
        options(|o| o.supplements = true),
        false,
    );
    // the tags first, then the links
    let files = s.files("smith2020");
    assert_eq!(files.len(), 3, "{files:?}");
    let name = |f: &str| {
        Path::new(f)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned()
    };
    let stem = name(&files[0]).trim_end_matches(".pdf").to_string();
    assert_eq!(name(&files[1]), format!("{stem}--si-tables.pdf"));
    assert_eq!(name(&files[2]), format!("{stem}--ocean-supplement.pdf"));
    assert!(joined(&out).contains("(supplement, from journals.example.org)"));
    let downloads = s.web.downloads();
    assert!(!downloads.contains(&"https://journals.example.org/other.pdf".to_string()));
    assert!(!downloads.contains(&"https://journals.example.org/data.zip".to_string()));
}

#[test]
fn no_supplements_without_the_option() {
    let s = supplements();
    s.out(&["smith2020"]);
    assert_eq!(s.files("smith2020").len(), 1);
}

#[test]
fn a_second_pass_with_supplements() {
    let s = supplements();
    s.out(&["smith2020"]);
    s.web.clear();
    let (out, _) = s.fetch(
        &["smith2020"],
        false,
        options(|o| o.supplements = true),
        false,
    );
    assert!(joined(&out).contains("smith2020: has a PDF already"));
    assert_eq!(s.files("smith2020").len(), 3);
    assert!(!s.web.downloads().contains(&PDF.to_string()));
}

// TestNoDuplicate
// ---------------

fn smith() -> Sandbox {
    let s = sandbox();
    s.bib(&[entry("smith2020", &[], Some(DOI))]);
    s.publisher();
    s
}

#[test]
fn the_same_content_is_not_attached() {
    let s = smith();
    // a file of the entry with the content of the PDF, which this machine did not download
    let local = s.path("lib/files/mine.pdf");
    fs::write(&local, pdf_bytes(PDF)).unwrap();
    s.bib(&[entry("smith2020", &["files/mine.pdf"], Some(DOI))]);
    let (out, _) = s.fetch(&["smith2020"], false, options(|o| o.again = true), false);
    assert!(
        joined(&out).contains(&format!(
            "smith2020: {PDF}: not attached, the entry has the same file: "
        )),
        "{out:?}"
    );
    assert_eq!(s.files("smith2020"), [local]);
}

#[test]
fn cache_a_the_entry_has_it() {
    let s = smith();
    s.out(&["smith2020"]);
    let file = s.files("smith2020").remove(0);
    s.web.clear();
    let (out, _) = s.fetch(&["smith2020"], false, options(|o| o.again = true), false);
    let name = Path::new(&file)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(
        joined(&out).contains("smith2020: already has ")
            && joined(&out).contains(&format!("{name} (from journals.example.org)")),
        "{out:?}"
    );
    assert!(s.web.downloads().is_empty());
    assert_eq!(s.files("smith2020"), [file]);
}

#[test]
fn cache_b_another_entry_has_it() {
    let s = smith();
    s.out(&["smith2020"]);
    let file = s.files("smith2020").remove(0);
    let text = s.bibtext() + &entry("copy2020", &[], Some("10.1000/copy"));
    fs::write(s.path("lib/papers.bib"), text).unwrap();
    s.web.publisher("10.1000/copy", Some(PDF), None, &[], &[]); // the same PDF URL
    s.web.clear();
    let out = s.out(&["copy2020"]);
    assert!(s.web.downloads().is_empty());
    assert!(
        joined(&out).contains("same file as smith2020, not downloaded"),
        "{out:?}"
    );
    assert_eq!(s.files("copy2020"), std::slice::from_ref(&file)); // linked, not moved
    assert_eq!(s.files("smith2020"), [file]);
}

#[test]
fn cache_c_a_miss_downloads() {
    let s = smith();
    s.out(&["smith2020"]);
    let file = s.files("smith2020").remove(0);
    fs::remove_file(&file).unwrap();
    s.bib(&[entry("smith2020", &[], Some(DOI))]);
    s.web.clear();
    s.out(&["smith2020"]);
    assert_eq!(s.web.downloads(), [PDF]);
    assert_eq!(s.files("smith2020").len(), 1);
}

#[test]
fn the_library_does_not_record_the_url() {
    let s = smith();
    s.out(&["smith2020"]);
    fn walk(dir: &Path, found: &mut Vec<std::path::PathBuf>) {
        for e in fs::read_dir(dir).unwrap().flatten() {
            if e.path().is_dir() {
                walk(&e.path(), found);
            } else {
                found.push(e.path());
            }
        }
    }
    let mut files = Vec::new();
    walk(Path::new(&s.path("lib")), &mut files);
    for path in files {
        if path.extension().is_some_and(|e| e == "pdf") {
            continue;
        }
        let text = String::from_utf8_lossy(&fs::read(&path).unwrap()).into_owned();
        assert!(!text.contains("journals.example.org"), "{}", path.display());
    }
    assert!(s.fetcher.known_download(PDF).is_some());
}

// TestAddAndFetch
// ---------------

#[test]
fn add_doi_fetch_pdf() {
    let s = sandbox();
    s.publisher();
    s.web.0.lock().unwrap().crossref.insert(
        DOI.to_string(),
        json!({"DOI": DOI, "type": "journal-article", "title": ["The ocean"], "author": [{"family": "Smith", "given": "Ann"}],
               "container-title": ["Ocean"], "issued": {"date-parts": [[2020]]}}),
    );
    let fetcher = Fetcher::with_transport(Box::new(s.web.clone()), &s.path("cache"));
    let mut session = s.session(false);
    let request = AddRequest {
        doi: Some(DOI.to_string()),
        mode: Mode::Link,
        queries: Queries::with(Fetcher::with_transport(
            Box::new(s.web.clone()),
            &s.path("cache"),
        )),
        ..AddRequest::default()
    };
    let touched = add(
        &mut session,
        &request,
        &mut Resolver::new(OnDuplicate::Skip),
    )
    .unwrap();
    let failed = fetch_entry_pdfs(
        &mut session,
        &fetcher,
        &touched,
        Mode::Link,
        &PdfOptions::default(),
        &HashSet::new(),
        &mut FetchSummary::default(),
    )
    .unwrap();
    assert_eq!(failed, 0);
    session.commit().unwrap();
    let out = session.finish().out;
    assert!(
        joined(&out).contains("(published version, from journals.example.org)"),
        "{out:?}"
    );
    let files = s.files(&touched[0]);
    assert_eq!(fs::read(&files[0]).unwrap(), pdf_bytes(PDF));
    // its version (`test_pdf_versions.TestFilesJson.test_add_fetch_pdf`)
    let rel = files[0].strip_prefix(&s.path("lib/")).unwrap();
    assert_eq!(s.version_of(rel).as_deref(), Some("published"));
}

/// `fetch --pdf` of an accepted manuscript after the published version was refused (a challenge):
/// its description gives the published link (`test_fetch_pdf_published_refused`).
#[test]
fn fetch_pdf_published_refused() {
    let s = sandbox();
    s.web.publisher(
        DOI,
        Some(PDF),
        Some(b"<html>Just a moment...</html>".to_vec()),
        &[],
        &[],
    );
    s.web.openalex(
        DOI,
        vec![location(
            "https://repo.example.edu/am.pdf",
            "acceptedVersion",
            None,
            true,
        )],
    );
    s.web.file(
        "https://repo.example.edu/am.pdf",
        File::Bytes(pdf_bytes("am")),
    );
    let mut search = Search::new(
        &s.fetcher,
        &OpenAlex::default(),
        Some(DOI),
        None,
        Least::Any,
    );
    let found = fetch_pdf(&mut search, None).unwrap();
    assert_eq!(found.refused, [PDF]);
    assert_eq!(
        found.describe(),
        format!(
            "accepted manuscript, from repo.example.edu; the published version was refused: open {PDF} in a browser"
        )
    );
    freepdf::discard(&found.path);
}

// TestFetchPdfPorcelain: formats.md 7.2, the porcelain lines of `papers fetch --pdf`, one group
// per argument, in their order (the exit status: tests/fetch_extract.rs of papers-rs-cli)
// -----------------------------------------------------------------------------------------------

impl Sandbox {
    /// The porcelain lines of `papers fetch --pdf ARG... [-o FILE]`, and whether each argument
    /// gave a PDF.
    fn porcelain(&self, args: &[&str], output: Option<&str>) -> (String, Vec<bool>) {
        let mut out = String::new();
        let mut ok = Vec::new();
        for arg in args {
            let result =
                fetch_free_pdf(&self.fetcher, &OpenAlex::default(), arg, output, Least::Any);
            for line in pdf_porcelain_lines(arg, &result) {
                out.push_str(&line);
                out.push('\n');
            }
            ok.push(result.is_ok());
        }
        (out, ok)
    }
}

#[test]
fn porcelain_downloaded() {
    let s = sandbox();
    s.publisher();
    let target = s.path("paper.pdf");
    let (out, ok) = s.porcelain(&[DOI], Some(&target));
    assert_eq!(out, format!("downloaded {DOI} published {PDF} {target}\n"));
    assert_eq!(ok, [true]);
    assert_eq!(fs::read(&target).unwrap(), pdf_bytes(PDF));
}

/// `test_downloaded_into_the_current_directory`, with `-o DIR`: the name of the URL in it (the
/// current directory is that of the whole test process).
#[test]
fn porcelain_downloaded_into_a_directory() {
    let s = sandbox();
    s.web.file(
        "https://arxiv.org/pdf/2101.01234",
        File::Bytes(pdf_bytes("arxiv")),
    );
    let dir = s.path("lib");
    let (out, _) = s.porcelain(&["arXiv:2101.01234"], Some(&dir));
    assert_eq!(
        out,
        format!(
            "downloaded arXiv:2101.01234 submitted https://arxiv.org/pdf/2101.01234 {dir}/2101.01234.pdf\n"
        )
    );
}

#[test]
fn porcelain_refused_published() {
    let s = sandbox();
    s.publisher();
    s.web.file(PDF, File::Status(403));
    s.web.openalex(
        DOI,
        vec![location(
            "https://repo.example.edu/am.pdf",
            "acceptedVersion",
            None,
            true,
        )],
    );
    s.web.file(
        "https://repo.example.edu/am.pdf",
        File::Bytes(pdf_bytes("am")),
    );
    let target = s.path("paper.pdf");
    let (out, _) = s.porcelain(&[DOI], Some(&target));
    assert_eq!(
        out,
        format!(
            "downloaded {DOI} accepted https://repo.example.edu/am.pdf {target}\nrefused-published {DOI} {PDF}\n"
        )
    );
}

#[test]
fn porcelain_refused() {
    let s = sandbox();
    s.web.publisher(
        DOI,
        Some(PDF),
        Some(b"<html>a challenge</html>".to_vec()),
        &[],
        &[],
    );
    s.web.openalex(DOI, vec![]);
    let (out, ok) = s.porcelain(&[DOI], Some(&s.path("paper.pdf")));
    assert_eq!(
        out,
        format!("refused {DOI} {PDF}\nrefused {DOI} https://doi.org/{DOI}\n")
    );
    assert_eq!(ok, [false]);
}

#[test]
fn porcelain_failed() {
    let s = sandbox();
    assert_eq!(
        s.porcelain(&["nothing"], None),
        ("failed nothing\n".to_string(), vec![false])
    );
}

/// An existing `-o FILE` is found before any request: nothing is looked up nor downloaded
/// (`test_exists_output_checked_before_the_download`).
#[test]
fn porcelain_exists_output_checked_before_the_download() {
    let s = sandbox();
    s.publisher();
    s.web.file(PDF, File::Status(403));
    s.web.openalex(
        DOI,
        vec![location(
            "https://repo.example.edu/am.pdf",
            "acceptedVersion",
            None,
            true,
        )],
    );
    s.web.file(
        "https://repo.example.edu/am.pdf",
        File::Bytes(pdf_bytes("am")),
    );
    let target = s.write("paper.pdf", "x");
    assert_eq!(
        s.porcelain(&[DOI], Some(&target)),
        (format!("exists {DOI} {target}\n"), vec![false])
    );
    assert!(s.web.0.lock().unwrap().requests.is_empty());
    assert_eq!(fs::read_to_string(&target).unwrap(), "x");
    let error = fetch_free_pdf(
        &s.fetcher,
        &OpenAlex::default(),
        DOI,
        Some(&target),
        Least::Any,
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        format!("{target} exists: not replaced (-o FILE writes another file)")
    );
}

/// The name made from the link is known once its PDF is found: it is not replaced, and no other
/// link is tried (`test_exists_name_of_the_link`, with `-o DIR`: the current directory is that of
/// the whole test process).
#[test]
fn porcelain_exists_name_of_the_link() {
    let s = sandbox();
    s.publisher();
    s.web.openalex(
        DOI,
        vec![location(
            "https://repo.example.edu/am.pdf",
            "acceptedVersion",
            None,
            true,
        )],
    );
    let existing = s.write("lib/10.1000_ocean.pdf", "x");
    let (out, ok) = s.porcelain(&[DOI], Some(&s.path("lib")));
    assert_eq!(out, format!("exists {DOI} {existing}\n"));
    assert_eq!(ok, [false]);
    assert_eq!(s.web.downloads(), [PDF]);
    assert_eq!(fs::read_to_string(&existing).unwrap(), "x");
}

#[test]
fn porcelain_several_in_order() {
    let s = sandbox();
    s.publisher();
    s.web.publisher("10.1000/other", None, None, &[], &[]);
    s.web.openalex("10.1000/other", vec![]);
    s.web.file(
        "https://arxiv.org/pdf/2101.01234",
        File::Bytes(pdf_bytes("arxiv")),
    );
    let dir = s.path("lib");
    let doi = format!("doi:{DOI}");
    let (out, ok) = s.porcelain(&["10.1000/other", &doi, "2101.01234"], Some(&dir));
    assert_eq!(
        out,
        format!(
            "refused 10.1000/other https://doi.org/10.1000/other\n\
             downloaded doi:{DOI} published {PDF} {dir}/10.1000_ocean.pdf\n\
             downloaded 2101.01234 submitted https://arxiv.org/pdf/2101.01234 {dir}/2101.01234.pdf\n"
        )
    );
    assert_eq!(ok, [false, true, true]);
    assert!(Path::new(&format!("{dir}/10.1000_ocean.pdf")).exists());
}

// TestPageLinks
// -------------

#[test]
fn the_openalex_request() {
    let s = sandbox();
    s.web.openalex(
        DOI,
        vec![location(
            "https://x.example.org/a.pdf",
            "publishedVersion",
            None,
            true,
        )],
    );
    let access = OpenAlex {
        api_key: Some("secret".into()),
        mailto: None,
    };
    let locations = s.fetcher.work_locations(DOI, &access).unwrap();
    assert_eq!(
        locations[0].pdf_url.as_deref(),
        Some("https://x.example.org/a.pdf")
    );
    let url = s.web.requests("openalex").pop().unwrap();
    let (base, query) = url.split_once('?').unwrap();
    assert_eq!(base, "https://api.openalex.org/works/doi:10.1000/ocean");
    assert!(
        query.contains(&format!("select={}", WORK_SELECT.replace(',', "%2C"))),
        "{query}"
    );
    assert!(query.contains("api_key=secret"));
}

#[test]
fn fetch_pdf_of_a_doi() {
    let s = sandbox();
    s.publisher();
    let mut search = Search::new(
        &s.fetcher,
        &OpenAlex::default(),
        Some(DOI),
        None,
        Least::Any,
    );
    let found = fetch_pdf(&mut search, None).unwrap();
    assert_eq!(fs::read(&found.path).unwrap(), pdf_bytes(PDF));
    assert_eq!(
        found.link.describe(),
        "published version, from journals.example.org"
    );
    freepdf::discard(&found.path);
    assert!(!Path::new(&found.path).exists());
}

// the file of a broken link in the library, its pool and the other libraries of the machine
// (tests/test_file_other_libraries.py)
// ------------------------------------------------------------------------------------------

/// The files under a folder, with their content and time (to see that nothing changed).
fn tree(top: &str) -> Vec<(String, Vec<u8>, std::time::SystemTime)> {
    fn walk(dir: &Path, found: &mut Vec<(String, Vec<u8>, std::time::SystemTime)>) {
        for e in fs::read_dir(dir).unwrap().flatten() {
            let path = e.path();
            if path.is_dir() {
                walk(&path, found);
            } else {
                let modified = fs::metadata(&path).unwrap().modified().unwrap();
                found.push((
                    path.display().to_string(),
                    fs::read(&path).unwrap(),
                    modified,
                ));
            }
        }
    }
    let mut found = Vec::new();
    walk(Path::new(top), &mut found);
    found.sort();
    found
}

fn author_entry(key: &str, file: &str, doi: &str, author: &str, title: &str) -> String {
    format!(
        "@article{{{key},\n author = {{{author}}},\n doi = {{{doi}}},\n file = {{:{file}:pdf}},\n title = {{{title}}},\n year = {{2000}}\n}}\n\n"
    )
}

/// lib/ (files: copy), and the library other/, which the history of the machine knows: its entry
/// doe2020 (DOI 10.1000/doe) links to other/files/doe.pdf, whose checksum its files.json has.
struct Others {
    s: Sandbox,
    other_pdf: String,
    before: Vec<(String, Vec<u8>, std::time::SystemTime)>,
}

fn others() -> Others {
    let s = sandbox();
    s.files_setting("copy");
    s.write(
        "other/.papers/config.json",
        "{\n  \"bibtex\": \"papers.bib\",\n  \"filesdir\": \"files\",\n  \"pool\": \"none\",\n  \"version\": 3\n}",
    );
    let other_pdf = s.write("other/files/doe.pdf", "pdf doe");
    s.write(
        "other/papers.bib",
        &author_entry(
            "doe2020",
            "files/doe.pdf",
            "10.1000/doe",
            "Doe, Jane",
            "Ice",
        ),
    );
    // a step: other/ is a place of the history
    s.checksum_in("other");
    let before = tree(&s.path("other"));
    Others {
        s,
        other_pdf,
        before,
    }
}

impl Sandbox {
    /// `papers file checksum` in the library of the folder `dir`.
    fn checksum_in(&self, dir: &str) {
        let mut session = Session::open(self.config_of(dir), Options::default()).unwrap();
        let keys: Vec<String> = session.library.entries().map(|e| e.key.clone()).collect();
        checksums(&mut session, &keys, false).unwrap();
        session.finish();
    }

    /// `papers file check` (`fix`: `papers file fix`) of the whole library: its lines.
    fn check(&self, fix: bool, dry_run: bool) -> Vec<String> {
        let mut session = self.session(!fix || dry_run);
        if fix && !dry_run {
            session.use_pool_files().unwrap();
        }
        let keys: Vec<String> = session.library.entries().map(|e| e.key.clone()).collect();
        let options = CheckOptions {
            fix_command: "papers file fix".to_string(),
            ..CheckOptions::default()
        };
        check_entries_files(&mut session, &options, fix, &keys, &Queries::default()).unwrap();
        session.finish().out
    }
}

impl Others {
    fn assert_other_unchanged(&self) {
        assert!(tree(&self.s.path("other")) == self.before, "other/ changed");
    }

    fn found_in_other(&self) -> String {
        format!("(found in {})", self.s.path("other"))
    }

    /// An entry of lib/ whose file lib/files/<key>.pdf had `content`, its checksum recorded, then
    /// deleted.
    fn lost(&self, key: &str, content: &str, doi: Option<&str>) -> String {
        let path = self.s.write(&format!("lib/files/{key}.pdf"), content);
        self.s
            .bib(&[entry(key, &[&format!("files/{key}.pdf")], doi)]);
        self.s.checksum_in("lib");
        fs::remove_file(&path).unwrap();
        path
    }
}

#[test]
fn own_files_directory_by_checksum() {
    // renamed by hand in the files directory: found by its checksum, renamed in place (files: copy)
    let o = others();
    let old = o.lost("one2000", "pdf one", None);
    o.s.write("lib/files/renamed.pdf", "pdf one");
    let out = o.s.check(true, false);
    assert!(
        joined(&out).contains("(same content, in the library)"),
        "{out:?}"
    );
    let files = o.s.files("one2000");
    assert_eq!(files.len(), 1);
    assert_eq!(fs::read_to_string(&files[0]).unwrap(), "pdf one");
    let name = Path::new(&files[0])
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert_eq!(o.s.libfiles(), [name]);
    assert_ne!(files[0], old);
}

#[test]
fn own_file_of_another_entry_by_checksum() {
    let o = others();
    let shared = o.s.write("lib/files/shared.pdf", "pdf one");
    let old = o.s.write("lib/files/one.pdf", "pdf one");
    o.s.bib(&[
        entry("one2000", &["files/one.pdf"], None),
        author_entry(
            "two2000",
            "files/shared.pdf",
            "10.1000/two",
            "Some One",
            "Other",
        ),
    ]);
    o.s.checksum_in("lib");
    fs::remove_file(&old).unwrap();
    let out = o.s.check(true, false);
    assert!(joined(&out).contains("one2000: relinked to"), "{out:?}");
    assert!(joined(&out).contains("(same content as the file of two2000)"));
    // linked where it is
    assert_eq!(o.s.files("one2000"), [shared]);
}

#[test]
fn other_library_by_checksum() {
    let o = others();
    let old = o.lost("one2000", "pdf doe", None); // another title, no DOI: by checksum
    let out = o.s.check(false, false);
    assert!(
        out.contains(&format!(
            "one2000: broken link: {old} -> {} {}",
            o.other_pdf,
            o.found_in_other()
        )),
        "{out:?}"
    );
    let out = o.s.check(true, false);
    assert!(joined(&out).contains(&o.found_in_other()), "{out:?}");
    let file = o.s.files("one2000").remove(0);
    assert_eq!(
        Path::new(&file).parent().unwrap(),
        Path::new(&o.s.path("lib/files"))
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "pdf doe");
    o.assert_other_unchanged();
}

#[test]
fn same_paper_without_checksum_not_taken() {
    // the same DOI as doe2020 of other/, no checksum recorded for the link: other/ is not searched
    let o = others();
    let gone = o.s.path("gone/ice.pdf");
    o.s.bib(&[entry("doe2020a", &[&gone], Some("10.1000/doe"))]);
    let out = o.s.check(true, false);
    assert!(
        out.contains(
            &"0 links relinked, 1 still broken (papers file detach --broken removes them)"
                .to_string()
        ),
        "{out:?}"
    );
}

#[test]
fn other_content_not_taken() {
    let o = others();
    o.lost("one2000", "pdf other", None);
    assert!(joined(&o.s.check(true, false)).contains("0 links relinked, 1 still broken"));
}

#[test]
fn move_setting_copies() {
    let o = others();
    o.s.files_setting("move");
    o.lost("one2000", "pdf doe", None);
    o.s.check(true, false);
    let file = o.s.files("one2000").remove(0);
    assert_eq!(
        Path::new(&file).parent().unwrap(),
        Path::new(&o.s.path("lib/files"))
    );
    assert!(Path::new(&o.other_pdf).exists());
    o.assert_other_unchanged();
}

#[test]
fn gone_library_skipped() {
    let o = others();
    fs::rename(o.s.path("other"), o.s.path("moved")).unwrap();
    o.lost("one2000", "pdf doe", None);
    assert!(joined(&o.s.check(true, false)).contains("0 links relinked, 1 still broken"));
}

#[test]
fn dry_run_of_a_fix_from_another_library() {
    let o = others();
    let old = o.lost("one2000", "pdf doe", None);
    let before = o.s.bibtext();
    let out = o.s.check(true, true);
    assert!(
        out.contains(&format!(
            "DRYRUN: one2000: relink {old} -> {} {}",
            o.other_pdf,
            o.found_in_other()
        )),
        "{out:?}"
    );
    assert_eq!(o.s.bibtext(), before);
}

/// lib/ is a shallow library of other/: other/ is its pool, searched once (as the pool), not again
/// as another library.
fn pool_of_others() -> Others {
    let o = others();
    let path = o.s.path("lib/.papers/config.json");
    let text = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        text.replace("\"pool\": \"none\"", "\"pool\": \"../other\""),
    )
    .unwrap();
    o
}

#[test]
fn the_pool_by_the_same_paper() {
    let o = pool_of_others();
    let gone = o.s.path("gone/ice.pdf");
    o.s.bib(&[entry("doe2020a", &[&gone], Some("10.1000/doe"))]);
    let out = o.s.check(true, false);
    assert!(
        out.contains(&format!(
            "doe2020a: relinked to {} (from the pool)",
            o.other_pdf
        )),
        "{out:?}"
    );
}

#[test]
fn the_pool_by_checksum() {
    let o = pool_of_others();
    let old = o.lost("one2000", "pdf doe", None);
    let out = o.s.check(false, false);
    assert!(
        out.contains(&format!(
            "one2000: broken link: {old} -> {} (from the pool, same content)",
            o.other_pdf
        )),
        "{out:?}"
    );
}

#[test]
fn missing_without_pdf_link_downloads() {
    // the same paper in other/ with a PDF: not looked for, downloaded
    let o = others();
    o.s.web.publisher("10.1000/doe", Some(PDF), None, &[], &[]);
    o.s.bib(&[entry("doe2020a", &[], Some("10.1000/doe"))]);
    o.s.fetch(&[], true, PdfOptions::default(), false);
    assert_eq!(o.s.web.downloads().len(), 1);
    let file = o.s.files("doe2020a").remove(0);
    assert!(fs::read(&file).unwrap().starts_with(b"%PDF"));
    o.assert_other_unchanged();
}

#[test]
fn missing_broken_link_by_checksum() {
    let o = others();
    o.s.web.publisher("10.1000/doe", Some(PDF), None, &[], &[]);
    o.lost("doe2020a", "pdf doe", Some("10.1000/doe"));
    let (out, _) = o.s.fetch(&[], true, PdfOptions::default(), false);
    assert!(joined(&out).contains("not downloaded"), "{out:?}");
    assert!(o.s.web.downloads().is_empty());
    assert_eq!(
        fs::read_to_string(&o.s.files("doe2020a")[0]).unwrap(),
        "pdf doe"
    );
    o.assert_other_unchanged();
}

#[test]
fn a_key_still_downloads() {
    let o = others();
    o.s.web.publisher("10.1000/doe", Some(PDF), None, &[], &[]);
    o.s.bib(&[entry("doe2020a", &[], Some("10.1000/doe"))]);
    o.s.out(&["doe2020a"]);
    assert_eq!(o.s.web.downloads().len(), 1);
    let file = o.s.files("doe2020a").remove(0);
    assert!(fs::read(&file).unwrap().starts_with(b"%PDF"));
}

/// A new file in other/, its entry new2020.
fn new_in_other(o: &Others) -> String {
    let new = o.s.write("other/files/new.pdf", "pdf new");
    o.s.write(
        "other/papers.bib",
        &(author_entry(
            "doe2020",
            "files/doe.pdf",
            "10.1000/doe",
            "Doe, Jane",
            "Ice",
        ) + &entry("new2020", &["files/new.pdf"], None)),
    );
    new
}

#[test]
fn a_new_head_is_read() {
    let o = others();
    o.lost("one2000", "pdf doe", None);
    o.s.check(false, false);
    // a new file in other/, recorded by a command there: found at the next search
    let new = new_in_other(&o);
    o.s.checksum_in("other");
    o.lost("two2000", "pdf new", None);
    let out = o.s.check(false, false);
    assert!(
        joined(&out).contains(&format!("-> {new} {}", o.found_in_other())),
        "{out:?}"
    );
}

#[test]
fn an_edit_outside_papers_is_seen_after_a_command() {
    let o = others();
    let new = new_in_other(&o);
    o.lost("two2000", "pdf new", None);
    // not in the head state
    assert!(joined(&o.s.check(true, true)).contains("0 links relinked, 1 still broken"));
    o.s.checksum_in("other");
    let out = o.s.check(false, false);
    assert!(
        joined(&out).contains(&format!("-> {new} {}", o.found_in_other())),
        "{out:?}"
    );
}

#[test]
fn a_missing_file_is_skipped() {
    let o = others();
    o.lost("one2000", "pdf doe", None);
    o.s.check(false, false);
    fs::remove_file(&o.other_pdf).unwrap();
    assert!(joined(&o.s.check(true, false)).contains("0 links relinked, 1 still broken"));
}

#[test]
fn a_changed_content_is_skipped() {
    let o = others();
    o.lost("one2000", "pdf doe", None);
    o.s.check(false, false);
    fs::write(&o.other_pdf, "pdf changed").unwrap();
    assert!(joined(&o.s.check(true, false)).contains("0 links relinked, 1 still broken"));
}

#[test]
fn a_gone_place_is_skipped() {
    let o = others();
    o.lost("one2000", "pdf doe", None);
    o.s.check(false, false);
    fs::rename(o.s.path("other"), o.s.path("moved")).unwrap();
    assert!(joined(&o.s.check(true, false)).contains("0 links relinked, 1 still broken"));
}

// the version of the PDFs that papers file fetch attaches (.papers/files.json), papers file fetch
// --upgrade (history.md 4, 6.4, 10; library.md "The free PDF of an entry";
// tests/test_pdf_versions.py; its TestList: tests/formats_cmds.rs of papers-rs-cli)
// -------------------------------------------------------------------------------------------------

const AM: &str = "https://repo.example.edu/am.pdf";

/// smith2020 gets an accepted manuscript: the publisher refuses its PDF (`VersionsTest`).
fn versions() -> Sandbox {
    let s = sandbox();
    s.bib(&[entry("smith2020", &[], Some(DOI))]);
    s.publisher();
    s.web.file(PDF, File::Status(403));
    s.web
        .openalex(DOI, vec![location(AM, "acceptedVersion", None, true)]);
    s.web.file(AM, File::Bytes(pdf_bytes("am")));
    s
}

impl Sandbox {
    fn files_json(&self) -> Option<Value> {
        fs::read_to_string(self.path("lib/.papers/files.json"))
            .ok()
            .map(|text| serde_json::from_str(&text).unwrap())
    }

    /// The version of the file at `rel` (relative to the library).
    fn version_of(&self, rel: &str) -> Option<String> {
        let data = self.files_json()?;
        let digest = data["files"][rel].as_str()?;
        data["versions"][digest].as_str().map(str::to_string)
    }

    /// `papers file fetch smith2020`: its file, absolute and relative to the library.
    fn fetch_accepted(&self) -> (String, String) {
        self.out(&["smith2020"]);
        let files = self.files("smith2020");
        assert_eq!(files.len(), 1);
        let rel = files[0]
            .strip_prefix(&self.path("lib/"))
            .unwrap()
            .to_string();
        (files[0].clone(), rel)
    }

    /// The publisher gives its PDF now.
    fn publish(&self) {
        self.web.file(PDF, File::Bytes(pdf_bytes("vor")));
    }

    /// `papers file fetch --upgrade [KEY...]`: the lines and the summary.
    fn upgrade(&self, keys: &[&str], dry_run: bool) -> (Vec<String>, Option<Vec<String>>) {
        let mut session = self.session(dry_run);
        let request = FileUpgrade {
            keys: (!keys.is_empty()).then(|| keys.iter().map(|k| k.to_string()).collect()),
            ..FileUpgrade::default()
        };
        let upgraded = file_upgrade(&mut session, &self.fetcher, &request).unwrap();
        (session.finish().out, upgraded.summary())
    }

    fn history_step(&self, command: &str, undo: bool) {
        let mut config = self.config();
        let mut history = History::require(&mut config, false).unwrap();
        let mut resolver = Resolver::new(OnDuplicate::Skip);
        if undo {
            history
                .undo(1, false, None, command, &mut resolver)
                .unwrap();
        } else {
            history
                .redo(1, false, None, command, &mut resolver)
                .unwrap();
        }
    }

    fn head(&self) -> String {
        let mut config = self.config();
        let history = History::open(&mut config, false, false).unwrap().unwrap();
        history.head().unwrap().state.to_string()
    }
}

fn digest(path: &str) -> String {
    file_digest(path).unwrap()
}

// TestFilesJson

#[test]
fn versions_written_by_fetch() {
    let s = versions();
    let (file, rel) = s.fetch_accepted();
    let d = digest(&file);
    assert_eq!(
        s.files_json().unwrap(),
        json!({"files": {&rel: &d}, "versions": {&d: "accepted"}})
    );
    // written as json.dumps(indent=1, sort_keys=True)
    assert_eq!(
        fs::read_to_string(s.path("lib/.papers/files.json")).unwrap(),
        format!(
            "{{\n \"files\": {{\n  \"{rel}\": \"{d}\"\n }},\n \"versions\": {{\n  \"{d}\": \"accepted\"\n }}\n}}\n"
        )
    );
}

#[test]
fn versions_none_for_a_file_attached_by_hand() {
    let s = versions();
    let x = s.write("downloads/x.pdf", "pdf x");
    let mut session = s.session(false);
    attach_files(&mut session, "smith2020", &[x], &Attach::new(Mode::Copy)).unwrap();
    session.commit().unwrap();
    session.finish();
    assert!(s.files_json().unwrap().get("versions").is_none());
}

#[test]
fn versions_supplements_have_none() {
    let s = versions();
    let suppl = "https://journals.example.org/suppl.pdf";
    s.web
        .publisher(DOI, Some(PDF), None, &[(suppl, "Supplement")], &[]);
    s.web.file(suppl, File::Bytes(pdf_bytes("suppl")));
    s.fetch(
        &["smith2020"],
        false,
        options(|o| o.supplements = true),
        false,
    );
    assert_eq!(s.files("smith2020").len(), 2);
    let data = s.files_json().unwrap();
    let versions: Vec<&Value> = data["versions"].as_object().unwrap().values().collect();
    assert_eq!(versions, [&json!("published")]);
}

#[test]
fn versions_with_the_checksums_setting_off() {
    let s = versions();
    let path = s.path("lib/.papers/config.json");
    let text = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        text.replace("\"filesdir\"", "\"checksums\": false,\n  \"filesdir\""),
    )
    .unwrap();
    let (_, rel) = s.fetch_accepted();
    assert_eq!(s.version_of(&rel).as_deref(), Some("accepted"));
}

#[test]
fn versions_kept_when_the_file_moves() {
    let s = versions();
    let (file, _) = s.fetch_accepted();
    let mut session = s.session(false);
    move_files(
        &mut session,
        std::slice::from_ref(&file),
        &s.path("lib/files/moved.pdf"),
        &MvOptions::default(),
    )
    .unwrap();
    session.finish();
    let new = s.files("smith2020")[0].clone();
    assert_ne!(new, file);
    assert_eq!(s.version_of("files/moved.pdf").as_deref(), Some("accepted"));
}

#[test]
fn versions_dropped_with_the_checksum() {
    let s = versions();
    let (file, rel) = s.fetch_accepted();
    let x = s.write("downloads/x.pdf", "pdf x");
    let mut session = s.session(false);
    attach_files(&mut session, "smith2020", &[x], &Attach::new(Mode::Copy)).unwrap();
    session.commit().unwrap();
    session.finish();
    let mut session = s.session(false);
    rm_files(&mut session, None, &[file], false).unwrap();
    session.finish();
    let data = s.files_json().unwrap();
    assert!(data["files"].get(&rel).is_none());
    assert!(data.get("versions").is_none());
}

#[test]
fn versions_of_an_old_file_without_them() {
    let s = versions();
    let (file, rel) = s.fetch_accepted();
    let d = digest(&file);
    fs::write(
        s.path("lib/.papers/files.json"),
        format!("{{\n \"files\": {{\n  \"{rel}\": \"{d}\"\n }}\n}}\n"),
    )
    .unwrap();
    let config = s.config();
    let files_json = FilesJson::of(&config, &s.path("lib")).unwrap();
    assert!(files_json.file_versions().is_empty()); // unknown
    let mut session = s.session(false);
    checksums(&mut session, &["smith2020".to_string()], false).unwrap();
    session.finish();
    assert_eq!(s.files_json().unwrap(), json!({"files": {&rel: &d}}));
}

#[test]
fn versions_restored_by_undo() {
    let s = versions();
    let (_, rel) = s.fetch_accepted();
    s.history_step("papers undo", true);
    assert_eq!(s.files_json(), None);
    s.history_step("papers redo", false);
    assert_eq!(s.version_of(&rel).as_deref(), Some("accepted"));
}

// TestUpgrade

#[test]
fn upgrade_replaces_under_the_same_name() {
    let s = versions();
    let (file, rel) = s.fetch_accepted();
    s.publish();
    let (out, _) = s.upgrade(&[], false);
    assert!(
        out.contains(&format!(
            "smith2020: {file}: accepted manuscript replaced by the published version, from journals.example.org"
        )),
        "{out:?}"
    );
    assert_eq!(s.files("smith2020"), std::slice::from_ref(&file));
    assert_eq!(fs::read(&file).unwrap(), pdf_bytes("vor"));
    let d = digest(&file);
    assert_eq!(
        s.files_json().unwrap(),
        json!({"files": {&rel: &d}, "versions": {&d: "published"}})
    );
    assert_eq!(
        s.libfiles(),
        [Path::new(&file)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned()]
    );
}

#[test]
fn upgrade_undo_brings_the_old_file_back() {
    let s = versions();
    let (file, rel) = s.fetch_accepted();
    s.publish();
    s.upgrade(&["smith2020"], false);
    s.history_step("papers undo", true);
    assert_eq!(fs::read(&file).unwrap(), pdf_bytes("am"));
    assert_eq!(s.version_of(&rel).as_deref(), Some("accepted"));
    assert_eq!(s.libfiles().len(), 1);
    s.history_step("papers redo", false);
    assert_eq!(fs::read(&file).unwrap(), pdf_bytes("vor"));
    assert_eq!(s.version_of(&rel).as_deref(), Some("published"));
    s.history_step("papers undo", true);
    assert_eq!(fs::read(&file).unwrap(), pdf_bytes("am"));
}

#[test]
fn upgrade_undo_leaves_a_file_changed_since() {
    let s = versions();
    let (file, _) = s.fetch_accepted();
    s.publish();
    s.upgrade(&["smith2020"], false);
    fs::write(&file, b"%PDF annotated").unwrap();
    s.history_step("papers undo", true);
    assert_eq!(fs::read(&file).unwrap(), b"%PDF annotated");
}

#[test]
fn upgrade_operations_of_the_step() {
    let s = versions();
    let (file, rel) = s.fetch_accepted();
    let old = digest(&file);
    s.publish();
    s.upgrade(&["smith2020"], false);
    let mut config = s.config();
    let history = History::open(&mut config, false, false).unwrap().unwrap();
    let head = history.head().unwrap().state;
    let ops = history.project.state(&head).unwrap().ops();
    let new = digest(&file);
    let got: Vec<(OpKind, bool, Option<String>)> = ops
        .iter()
        .map(|op| {
            (
                op.kind,
                op.dst.as_deref() == Some(rel.as_str()),
                op.checksum.clone(),
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            (OpKind::Trashed, false, Some(old)),
            (OpKind::Copied, true, Some(new))
        ]
    );
    assert_eq!(ops[0].src, rel);
}

#[test]
fn upgrade_only_the_published_version() {
    let s = versions();
    s.fetch_accepted();
    s.web.clear();
    s.upgrade(&[], false);
    assert_eq!(s.web.downloads(), [PDF]); // not the accepted manuscript again
}

#[test]
fn upgrade_without_published_version_changes_nothing() {
    let s = versions();
    let (file, _) = s.fetch_accepted();
    let (before, head) = (s.files_json(), s.head());
    let (out, _) = s.upgrade(&["smith2020"], false);
    let out = joined(&out);
    assert!(
        out.contains(&format!(
            "smith2020: {file} (accepted manuscript) kept: no PDF could be downloaded"
        )),
        "{out}"
    );
    assert!(out.contains(&format!("open in a browser: {PDF}")), "{out}");
    assert_eq!(fs::read(&file).unwrap(), pdf_bytes("am"));
    assert_eq!(s.files_json(), before);
    assert_eq!(s.head(), head);
}

#[test]
fn upgrade_dry_run() {
    let s = versions();
    let (file, rel) = s.fetch_accepted();
    s.publish();
    let (out, _) = s.upgrade(&[], true);
    assert!(
        out.contains(&format!(
            "DRYRUN: smith2020: would download {PDF} (published version, from journals.example.org), in place of {file} (accepted manuscript)"
        )),
        "{out:?}"
    );
    assert_eq!(fs::read(&file).unwrap(), pdf_bytes("am"));
    assert_eq!(s.version_of(&rel).as_deref(), Some("accepted"));
}

#[test]
fn upgrade_entries_without_lower_version() {
    let s = versions();
    s.write("lib/files/a.pdf", "pdf a");
    s.bib(&[
        entry("smith2020", &[], Some(DOI)),
        entry("hand2000", &["files/a.pdf"], Some("10.1000/hand")),
    ]);
    let (file, _) = s.fetch_accepted();
    s.publish();
    let (out, _) = s.upgrade(&["hand2000"], false);
    assert!(
        out.contains(
            &"hand2000: no accepted manuscript or submitted version to replace".to_string()
        ),
        "{out:?}"
    );
    assert_eq!(s.web.downloads(), [PDF, AM]); // the first fetch only
    // all entries: those with a lower version
    let (out, _) = s.upgrade(&[], false);
    let out = joined(&out);
    assert!(!out.contains("hand2000"), "{out}");
    assert!(out.contains(&format!("smith2020: {file}: ")), "{out}");
    assert!(out.contains("replaced by the published version"), "{out}");
}

#[test]
fn upgrade_nothing_to_upgrade() {
    let s = versions();
    let mut session = s.session(false);
    let upgraded = file_upgrade(&mut session, &s.fetcher, &FileUpgrade::default()).unwrap();
    assert!(upgraded.keys.is_empty() && upgraded.looked.is_empty());
    assert!(s.web.0.lock().unwrap().requests.is_empty());
}

#[test]
fn upgrade_summary_of_several_entries() {
    let s = versions();
    s.bib(&[
        entry("smith2020", &[], Some(DOI)),
        entry("ice2020", &[], Some("10.1000/ice")),
    ]);
    let ice = "https://journals.example.org/ice.pdf";
    s.web.publisher("10.1000/ice", Some(ice), None, &[], &[]);
    s.web.file(ice, File::Status(403));
    s.web.openalex(
        "10.1000/ice",
        vec![location(
            "https://repo.example.edu/ice.pdf",
            "submittedVersion",
            None,
            true,
        )],
    );
    s.web.file(
        "https://repo.example.edu/ice.pdf",
        File::Bytes(pdf_bytes("ice")),
    );
    s.out(&["smith2020", "ice2020"]);
    s.publish();
    let (_, summary) = s.upgrade(&[], false);
    let summary = summary.unwrap();
    assert_eq!(
        summary[..4],
        [
            "summary of 2 entries:",
            "  1 replaced by the published version",
            "  1 without published version (kept as they are)",
            "open in a browser:",
        ]
    );
    assert!(
        summary[4].starts_with(&format!("  ice2020: {ice}")),
        "{summary:?}"
    );
}

// TestMoreSources: library.md "The free PDF of an entry", the landing pages of OpenAlex
// (repositories) and the retry after HTTP 429
// -------------------------------------------------------------------------------------------------

const REPO: &str = "https://repo.example.edu/handle/1";
const REPO_PDF: &str = "https://repo.example.edu/bitstream/1/paper.pdf";

/// An open-access location of OpenAlex without PDF link: its landing page.
fn landing_location(version: &str, landing: &str) -> Value {
    json!({"pdf_url": null, "landing_page_url": landing, "version": version, "is_oa": true,
           "source": {"display_name": "somewhere"}})
}

impl Web {
    /// A page of the web, answered from `last` (after the redirections).
    fn page(&self, url: &str, last: &str, html: String) {
        self.0
            .lock()
            .unwrap()
            .pages
            .insert(url.to_string(), (last.to_string(), html));
    }
}

/// smith2020; the publisher gives a check page for its PDF.
fn more_sources() -> Sandbox {
    let s = sandbox();
    s.bib(&[entry("smith2020", &[], Some(DOI))]);
    s.web.publisher(
        DOI,
        Some(PDF),
        Some(b"<html>a check page</html>".to_vec()),
        &[],
        &[],
    );
    s
}

/// The message of the search of the DOI (the error of `file fetch` of one entry).
fn no_pdf_of(s: &Sandbox, least: Least) -> String {
    let mut search = Search::new(&s.fetcher, &OpenAlex::default(), Some(DOI), None, least);
    fetch_pdf(&mut search, None).unwrap_err().message
}

#[test]
fn landing_page_citation_pdf_url() {
    let s = more_sources();
    s.web.openalex(
        DOI,
        vec![
            landing_location("acceptedVersion", REPO),
            location(
                "https://preprints.example.org/s.pdf",
                "submittedVersion",
                None,
                true,
            ),
        ],
    );
    s.web
        .page(REPO, REPO, page(Some("/bitstream/1/paper.pdf"), &[], &[]));
    s.web.file(REPO_PDF, File::Bytes(pdf_bytes("am")));
    let out = joined(&s.out(&["smith2020"]));
    assert!(
        out.contains(
            "(accepted manuscript, from repo.example.edu; the published version was refused"
        ),
        "{out}"
    );
    assert_eq!(fs::read(&s.files("smith2020")[0]).unwrap(), pdf_bytes("am"));
    assert_eq!(s.web.downloads(), [PDF, REPO_PDF]);
}

#[test]
fn landing_page_of_a_repository_doi_follows_the_redirection() {
    let s = more_sources();
    let landing = "https://doi.org/10.34657/351";
    s.web
        .openalex(DOI, vec![landing_location("publishedVersion", landing)]);
    s.web.page(
        landing,
        REPO,
        page(Some("/bitstream/1/paper.pdf"), &[], &[]),
    );
    s.web.file(REPO_PDF, File::Bytes(pdf_bytes("vor")));
    let out = joined(&s.out(&["smith2020"]));
    assert!(
        out.contains("(published version, from repo.example.edu)"),
        "{out}"
    );
}

#[test]
fn landing_page_without_the_tag_is_to_open() {
    let s = more_sources();
    s.web
        .openalex(DOI, vec![landing_location("acceptedVersion", REPO)]);
    s.web.page(
        REPO,
        REPO,
        page(None, &[("/bitstream/1/paper.pdf", "PDF")], &[]),
    );
    let (_, failed) = s.fetch(&["smith2020"], false, PdfOptions::default(), false);
    assert_eq!(failed, 1);
    assert_eq!(s.web.downloads(), [PDF]); // no other link of the page
    let message = no_pdf_of(&s, Least::Any);
    assert!(
        message.contains("repo.example.edu: no PDF link on the landing page"),
        "{message}"
    );
    assert!(
        message.contains(&format!(
            "open in a browser: {PDF}, https://doi.org/{DOI}, {REPO}"
        )),
        "{message}"
    );
}

#[test]
fn landing_page_of_the_publisher_is_not_read_again() {
    let s = more_sources();
    s.web.openalex(
        DOI,
        vec![landing_location(
            "publishedVersion",
            &format!("https://doi.org/{DOI}"),
        )],
    );
    let (_, failed) = s.fetch(&["smith2020"], false, PdfOptions::default(), false);
    assert_eq!(failed, 1);
    assert_eq!(s.web.requests("page").len(), 1);
}

#[test]
fn http_429_then_the_pdf() {
    let s = more_sources();
    let am = "https://repo.example.edu/am.pdf";
    s.web
        .openalex(DOI, vec![location(am, "acceptedVersion", None, true)]);
    s.web.file(
        am,
        File::Answers(vec![
            File::TooMany(Some("120")),
            File::Bytes(pdf_bytes("am")),
        ]),
    );
    let out = joined(&s.out(&["smith2020"]));
    assert!(
        out.contains("(accepted manuscript, from repo.example.edu"),
        "{out}"
    );
    assert_eq!(slept(), [Duration::from_secs(30)]); // at most 30 s
    assert_eq!(freepdf::retry_after(Some("120")), 30.0);
    assert_eq!(freepdf::retry_after(Some("2.5")), 2.5);
    assert_eq!(
        freepdf::retry_after(Some("Wed, 21 Oct 2015 07:28:00 GMT")),
        5.0
    );
}

#[test]
fn http_429_twice_moves_on() {
    let s = more_sources();
    let (am, preprint) = (
        "https://repo.example.edu/am.pdf",
        "https://preprints.example.org/s.pdf",
    );
    s.web.openalex(
        DOI,
        vec![
            location(am, "acceptedVersion", None, true),
            location(preprint, "submittedVersion", None, true),
        ],
    );
    s.web.file(am, File::TooMany(None));
    s.web.file(preprint, File::Bytes(pdf_bytes("s")));
    let out = joined(&s.out(&["smith2020"]));
    assert!(
        out.contains("(submitted version, from preprints.example.org"),
        "{out}"
    );
    assert_eq!(slept(), [Duration::from_secs(5)]); // without Retry-After
    assert_eq!(s.web.downloads(), [PDF, am, am, preprint]);
}

#[test]
fn http_429_of_a_page() {
    let s = sandbox();
    s.bib(&[entry("smith2020", &[], Some(DOI))]);
    s.web
        .publisher(DOI, Some(PDF), Some(pdf_bytes("vor")), &[], &[]);
    s.web
        .0
        .lock()
        .unwrap()
        .busy
        .insert(format!("https://doi.org/{DOI}"), Some("2"));
    let out = joined(&s.out(&["smith2020"]));
    assert!(out.contains("(published version"), "{out}");
    assert_eq!(
        s.web.requests("page"),
        [
            format!("https://doi.org/{DOI}"),
            format!("https://doi.org/{DOI}")
        ]
    );
    assert_eq!(slept(), [Duration::from_secs(2)]);
}

// TestNoPdfMessages: library.md "The free PDF of an entry", one entry without PDF is an error, of
// several a line each on the standard error (`one_line`; the line of an entry without DOI:
// tests/formats_cmds.rs of papers-rs-cli)
// -------------------------------------------------------------------------------------------------

#[test]
fn no_pdf_a_line_each() {
    let s = sandbox();
    s.bib(&[
        entry("smith2020", &[], Some(DOI)),
        entry("nodoi2000", &[], None),
    ]);
    s.web.publisher(DOI, None, None, &[], &[]);
    s.web.openalex(DOI, vec![]);
    let (_, failed) = s.fetch(
        &["smith2020", "nodoi2000"],
        false,
        PdfOptions::default(),
        false,
    );
    assert_eq!(failed, 2);
    assert_eq!(s.summary().unwrap()[0], "summary of 2 entries:");
    let message = no_pdf_of(&s, Least::Any);
    assert!(message.starts_with("no free PDF found\n"), "{message}");
    assert_eq!(
        one_line(&message),
        format!(
            "journals.example.org: no PDF link on the article page; OpenAlex: no open-access PDF; open https://doi.org/{DOI} in a browser"
        )
    );
    assert_eq!(
        one_line("no DOI or arXiv id to look for a PDF"),
        "no DOI or arXiv id to look for a PDF"
    );
}

// the versions of the PDFs found through a landing page (tests/test_pdf_versions.py)
// ----------------------------------------------------------------------------------

#[test]
fn versions_written_for_a_landing_page() {
    let s = versions();
    let landing = "https://repo.example.edu/handle/1";
    s.web
        .openalex(DOI, vec![landing_location("acceptedVersion", landing)]);
    s.web.page(landing, landing, page(Some(AM), &[], &[]));
    let (_, rel) = s.fetch_accepted();
    assert_eq!(s.version_of(&rel).as_deref(), Some("accepted"));
}

#[test]
fn upgrade_several_entries_a_line_each() {
    let s = versions();
    s.bib(&[
        entry("smith2020", &[], Some(DOI)),
        entry("ice2020", &[], Some("10.1000/ice")),
    ]);
    let ice = "https://journals.example.org/ice.pdf";
    s.web.publisher("10.1000/ice", Some(ice), None, &[], &[]);
    s.web.file(ice, File::Status(403));
    s.web.openalex(
        "10.1000/ice",
        vec![location(
            "https://repo.example.edu/ice.pdf",
            "submittedVersion",
            None,
            true,
        )],
    );
    s.web.file(
        "https://repo.example.edu/ice.pdf",
        File::Bytes(pdf_bytes("ice")),
    );
    s.out(&["smith2020", "ice2020"]);
    let file = s.files("ice2020")[0].clone();
    s.publish();
    let (out, _) = s.upgrade(&[], false);
    assert!(
        out.contains(&format!(
            "ice2020: {file} (submitted version) kept (no published version: {ice} (published version): HTTP 403; OpenAlex: no open-access PDF of the versions asked; open {ice}, https://doi.org/10.1000/ice in a browser)"
        )),
        "{out:?}"
    );
}

#[test]
fn upgrade_published_landing_page() {
    let s = versions();
    let (_, rel) = s.fetch_accepted();
    let landing = "https://repo.example.edu/handle/2";
    s.web.openalex(
        DOI,
        vec![
            location(AM, "acceptedVersion", None, true),
            landing_location("publishedVersion", landing),
        ],
    );
    // the locations changed
    fs::remove_dir_all(s.path("cache/openalex-work")).unwrap();
    s.web.page(
        landing,
        landing,
        page(Some("https://repo.example.edu/vor.pdf"), &[], &[]),
    );
    s.web.file(
        "https://repo.example.edu/vor.pdf",
        File::Bytes(pdf_bytes("vor")),
    );
    let (out, _) = s.upgrade(&["smith2020"], false);
    assert!(
        joined(&out).contains(
            "accepted manuscript replaced by the published version, from repo.example.edu"
        ),
        "{out:?}"
    );
    assert_eq!(s.version_of(&rel).as_deref(), Some("published"));
}
