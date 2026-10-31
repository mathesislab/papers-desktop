//! The free PDF of a paper, from its DOI or arXiv id (`papers.freepdf`: `papers file fetch`,
//! `papers add --fetch-pdf`, `papers fetch --pdf`). Only open-access copies.
//!
//! The links are tried in this order, until one gives a PDF (the first 1024 bytes contain
//! `%PDF`): a published version, else an accepted manuscript, else a submitted version
//! (preprint):
//!
//! 1. the publisher's page (`https://doi.org/<DOI>`, then the article page): its
//!    `citation_pdf_url` tag, a published version;
//! 2. OpenAlex (`GET /works/doi:<DOI>`, [`Fetcher::work_locations`]), version by version: the
//!    `pdf_url` of its open-access locations, the best location first, then the `citation_pdf_url`
//!    tag of the landing pages of those without `pdf_url` (repositories; not the page of the DOI or
//!    the page it led to);
//! 3. arXiv: `https://arxiv.org/pdf/<id>`, a submitted version.
//!
//! The publisher's page is read first, OpenAlex only when the publisher gives no PDF, and a
//! landing page only when the links before it gave none. An HTTP 429 (too many requests) is
//! asked again once, after the wait it asks (`Retry-After`, at most [`RETRY_MAX`] seconds;
//! [`RETRY_DEFAULT`] without it). A download that is refused (an HTTP error, a network error, not a
//! PDF) moves to the next link; when none is left, the error gives the links to open in a browser. When an accepted or submitted
//! version is found after the published version was refused, the published links are given too
//! ([`Found::refused`]). The supplements are the PDFs that the article page links with "suppl" in
//! the URL or the text of the link, and its `citation_supplementary_material` tags.
//!
//! The checksum (sha256) and size of each PDF downloaded are kept in the cache of the machine, by
//! URL ([`Fetcher::known_download`]): before a URL is downloaded again, the caller looks for a file
//! of that checksum (`local` of [`fetch_pdf`]), already linked by the entry or in the library.
//!
//! The requests wait a second between two requests to the same site ([`set_interval`]). The user
//! agent is that of the transport (papers': free).

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::Fetcher;
use super::download::{accept, connection_failed, save_download};
use super::http::Response;
use super::openalex::OpenAlex;
use crate::config::paths::dirname;
use crate::identity::{arxiv_of_doi, normalize_arxiv};
use crate::log;

/// A version of a paper, in the order of preference; a supplement is none of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Version {
    Published,
    Accepted,
    Submitted,
    Supplement,
}

impl Version {
    /// `VERSION_NAMES`
    pub fn name(self) -> &'static str {
        match self {
            Version::Published => "published version",
            Version::Accepted => "accepted manuscript",
            Version::Submitted => "submitted version",
            Version::Supplement => "supplement",
        }
    }

    /// The word of the version in the porcelain lines of `fetch --pdf` (`published`, `accepted`,
    /// `submitted`; formats.md 7.2).
    pub fn word(self) -> &'static str {
        match self {
            Version::Published => "published",
            Version::Accepted => "accepted",
            Version::Submitted => "submitted",
            Version::Supplement => "supplement",
        }
    }

    /// The version of a location of OpenAlex (`publishedVersion`...).
    fn of_openalex(version: Option<&str>) -> Option<Version> {
        match version? {
            "publishedVersion" => Some(Version::Published),
            "acceptedVersion" => Some(Version::Accepted),
            "submittedVersion" => Some(Version::Submitted),
            _ => None,
        }
    }
}

/// The versions of a paper, in the order of preference (`VERSIONS`).
const VERSIONS: [Version; 3] = [Version::Published, Version::Accepted, Version::Submitted];

/// The least final version wanted (`--version`: `published`, `accepted`, `any`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Least {
    Published,
    Accepted,
    #[default]
    Any,
}

impl Least {
    /// The choices of `--version` (`LEAST_VERSIONS`).
    pub const CHOICES: [&'static str; 3] = ["published", "accepted", "any"];

    pub fn parse(word: &str) -> Option<Least> {
        Some(match word {
            "published" => Least::Published,
            "accepted" => Least::Accepted,
            "any" => Least::Any,
            _ => return None,
        })
    }

    fn versions(self) -> &'static [Version] {
        match self {
            Least::Published => &VERSIONS[..1],
            Least::Accepted => &VERSIONS[..2],
            Least::Any => &VERSIONS,
        }
    }
}

/// A PDF to try: its URL and its version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub url: String,
    pub version: Version,
}

impl Link {
    fn new(url: impl Into<String>, version: Version) -> Link {
        Link {
            url: url.into(),
            version,
        }
    }

    pub fn host(&self) -> String {
        host(&self.url)
    }

    /// e.g. `published version, from journals.example.org`
    pub fn describe(&self) -> String {
        format!("{}, from {}", self.version.name(), self.host())
    }
}

/// The site of a URL (`urlparse(url).netloc`).
pub fn host(url: &str) -> String {
    let Some((_, rest)) = url.split_once("://") else {
        return String::new();
    };
    rest.split(['/', '?', '#']).next().unwrap_or("").to_string()
}

// the wait between two requests to a site
// ---------------------------------------

/// Milliseconds between two requests to a site (`INTERVAL`).
static INTERVAL_MS: AtomicU64 = AtomicU64::new(1000);
static LAST: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);

/// The time between two requests to the same site (a second; the tests set none).
pub fn set_interval(interval: Duration) {
    INTERVAL_MS.store(interval.as_millis() as u64, Ordering::Relaxed);
}

/// Seconds to wait after an HTTP 429 without `Retry-After` (`RETRY_DEFAULT`), and at most
/// (`RETRY_MAX`).
pub const RETRY_DEFAULT: f64 = 5.0;
pub const RETRY_MAX: f64 = 30.0;

/// How the wait after an HTTP 429 is done (`time.sleep`; the tests record it).
static SLEEP: Mutex<fn(Duration)> = Mutex::new(std::thread::sleep as fn(Duration));

/// The sleep of the wait after an HTTP 429 (the tests record it instead).
pub fn set_sleep(sleep: fn(Duration)) {
    *SLEEP
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = sleep;
}

/// The seconds to wait for a `Retry-After` header (`_retry_after`): seconds, at most
/// [`RETRY_MAX`]; a date or none gives [`RETRY_DEFAULT`].
pub fn retry_after(value: Option<&str>) -> f64 {
    match value.and_then(|v| v.trim().parse::<f64>().ok()) {
        Some(seconds) => seconds.clamp(0.0, RETRY_MAX),
        None => RETRY_DEFAULT,
    }
}

/// Seconds as Python's `{:g}` writes them here: `30`, `2.5`.
fn seconds_text(seconds: f64) -> String {
    if seconds.fract() == 0.0 {
        format!("{}", seconds as i64)
    } else {
        format!("{seconds}")
    }
}

/// Wait until the last request to the site of `url` is the interval ago (`_wait`).
fn wait(url: &str) {
    let interval = Duration::from_millis(INTERVAL_MS.load(Ordering::Relaxed));
    let mut last = LAST.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let last = last.get_or_insert_with(HashMap::new);
    let site = host(url);
    if let Some(when) = last.get(&site) {
        let since = when.elapsed();
        if since < interval {
            std::thread::sleep(interval - since);
        }
    }
    last.insert(site, Instant::now());
}

// the links of a paper
// --------------------

/// The PDF (`citation_pdf_url`) and the supplements (`citation_supplementary_material`, then the
/// links to a PDF with "suppl" in the URL or the text, case-insensitively) of an article page, the
/// links resolved against the URL of the page (`page_links`).
pub fn page_links(html: &str, base_url: &str) -> (Option<String>, Vec<String>) {
    let base = reqwest::Url::parse(base_url).ok();
    let join = |href: &str| match &base {
        Some(base) => base
            .join(href)
            .map(|u| u.to_string())
            .unwrap_or_else(|_| href.to_string()),
        None => href.to_string(),
    };
    let meta = |name: &str| -> Vec<String> {
        super::html::meta_named(html, name)
            .iter()
            .map(|c| join(c))
            .collect()
    };
    let pdf = meta("citation_pdf_url").into_iter().next();
    let mut supplements: Vec<String> = Vec::new();
    let add = |url: String, supplements: &mut Vec<String>| {
        if !supplements.contains(&url) && pdf.as_ref() != Some(&url) {
            supplements.push(url);
        }
    };
    for url in meta("citation_supplementary_material") {
        add(url, &mut supplements);
    }
    for (href, text) in super::html::anchors(html) {
        let url = join(href.trim());
        let lower = url.to_lowercase();
        if !lower.contains(".pdf") {
            continue;
        }
        if lower.contains("suppl") || text.to_lowercase().contains("suppl") {
            add(url, &mut supplements);
        }
    }
    (pdf, supplements)
}

/// The links of the PDF of a paper (a DOI and/or an arXiv id), in the order they are tried
/// (`Search`). `notes` says what each source gave, `browser` the pages to open in a browser.
pub struct Search<'a> {
    fetcher: &'a Fetcher,
    access: OpenAlex,
    pub doi: Option<String>,
    pub arxiv: Option<String>,
    versions: &'static [Version],
    pub notes: Vec<String>,
    pub browser: Vec<String>,
    /// The URL of the publisher's page, when it gave an HTTP or network error.
    pub page_refused: Option<String>,
    page: Option<(Option<String>, Vec<String>)>,
    /// The URL of the publisher's article page (after the redirections).
    page_final: Option<String>,
}

impl<'a> Search<'a> {
    /// The search of a DOI and an arXiv id (an arXiv DOI is the paper of arXiv: it is not looked
    /// up as a DOI), for the versions from the published one to `least`; `access`: how OpenAlex
    /// is asked.
    pub fn new(
        fetcher: &'a Fetcher,
        access: &OpenAlex,
        doi: Option<&str>,
        arxiv: Option<&str>,
        least: Least,
    ) -> Search<'a> {
        let mut doi = doi.filter(|d| !d.is_empty()).map(str::to_string);
        let mut arxiv = arxiv.filter(|a| !a.is_empty()).map(str::to_string);
        if let Some(of_doi) = doi.as_deref().and_then(arxiv_of_doi) {
            if arxiv.is_none() {
                arxiv = normalize_arxiv(&of_doi);
            }
            doi = None;
        }
        Search {
            fetcher,
            access: access.clone(),
            doi,
            arxiv,
            versions: least.versions(),
            notes: Vec::new(),
            browser: Vec::new(),
            page_refused: None,
            page: None,
            page_final: None,
        }
    }

    /// The PDF link and the supplements of the publisher's page, read once.
    fn page(&mut self) -> (Option<String>, Vec<String>) {
        if self.page.is_none() {
            self.page = Some((None, Vec::new()));
            if let Some(doi) = self.doi.clone() {
                let url = format!("https://doi.org/{doi}");
                self.browser.push(url.clone());
                match self.fetcher.fetch_page(&url) {
                    Ok(response) => {
                        if let Some(code) = response.status_error().map(|_| response.status) {
                            self.page_refused = Some(url.clone());
                            self.notes.push(format!("{url}: HTTP {code}"));
                        } else {
                            self.page_final = Some(response.url.clone());
                            let found = page_links(&response.text(), &response.url);
                            if found.0.is_none() {
                                self.notes.push(format!(
                                    "{}: no PDF link on the article page",
                                    host(&response.url)
                                ));
                            }
                            self.page = Some(found);
                        }
                    }
                    Err(error) => {
                        self.page_refused = Some(url.clone());
                        self.notes
                            .push(format!("{url}: {}", reason(&error.message)));
                    }
                }
            }
        }
        self.page.clone().unwrap_or_default()
    }

    /// The open-access locations of OpenAlex of the versions asked: their PDF links, and their
    /// landing pages without PDF link (as links of their version), by version (the best location
    /// first among those of a version); the open landing pages go to `browser`.
    fn openalex(&mut self) -> (Vec<Link>, Vec<Link>) {
        let Some(doi) = self.doi.clone() else {
            return (Vec::new(), Vec::new());
        };
        let locations = match self.fetcher.work_locations(&doi, &self.access) {
            Ok(locations) => locations,
            Err(error) if error.kind == super::ErrorKind::NotFound => {
                self.notes.push("OpenAlex: no work of this DOI".to_string());
                return (Vec::new(), Vec::new());
            }
            Err(error) => {
                self.notes.push(error.message);
                return (Vec::new(), Vec::new());
            }
        };
        let (mut links, mut landings): (Vec<Link>, Vec<Link>) = (Vec::new(), Vec::new());
        for x in locations {
            if !x.is_oa {
                continue;
            }
            let landing = x.landing_page_url.filter(|l| !l.is_empty());
            if let Some(landing) = &landing
                && !self.browser.contains(landing)
            {
                self.browser.push(landing.clone());
            }
            let Some(version) =
                Version::of_openalex(x.version.as_deref()).filter(|v| self.versions.contains(v))
            else {
                continue;
            };
            if let Some(pdf) = x.pdf_url.filter(|p| !p.is_empty()) {
                links.push(Link::new(pdf, version));
            } else if let Some(landing) = landing {
                landings.push(Link::new(landing, version));
            }
        }
        if links.is_empty() {
            self.notes.push(format!(
                "OpenAlex: no open-access PDF{}",
                if self.versions == VERSIONS {
                    ""
                } else {
                    " of the versions asked"
                }
            ));
        }
        // stable: the best location first
        links.sort_by_key(|link| link.version);
        landings.sort_by_key(|link| link.version);
        (links, landings)
    }

    /// The link of the `citation_pdf_url` tag of a landing page, of its version, or none (what
    /// the page gave goes to `notes`).
    fn landing(&mut self, link: &Link) -> Option<Link> {
        let response = match self.fetcher.fetch_page(&link.url) {
            Ok(response) => response,
            Err(error) => {
                self.notes
                    .push(format!("{}: {}", link.url, reason(&error.message)));
                return None;
            }
        };
        if let Some(code) = response.status_error().map(|_| response.status) {
            self.notes.push(format!("{}: HTTP {code}", link.url));
            return None;
        }
        let Some(pdf) = page_links(&response.text(), &response.url).0 else {
            self.notes.push(format!(
                "{}: no PDF link on the landing page",
                host(&response.url)
            ));
            return None;
        };
        Some(Link::new(pdf, link.version))
    }

    /// The links to try, in order; the sources are read as they are needed.
    pub fn links(&mut self) -> Links<'_, 'a> {
        Links {
            search: self,
            stage: 0,
            queue: Vec::new(),
            seen: HashSet::new(),
        }
    }

    /// The supplements of the article page.
    pub fn supplements(&mut self) -> Vec<Link> {
        if self.doi.is_none() {
            return Vec::new();
        }
        self.page()
            .1
            .into_iter()
            .map(|url| Link::new(url, Version::Supplement))
            .collect()
    }
}

/// A link to try, or a landing page whose `citation_pdf_url` is the link to try.
enum Step {
    Link(Link),
    Landing(Link),
}

/// The links of a [`Search`], read as they are needed (`Search.links`).
pub struct Links<'s, 'a> {
    search: &'s mut Search<'a>,
    stage: u8,
    queue: Vec<Step>,
    seen: HashSet<String>,
}

impl Iterator for Links<'_, '_> {
    type Item = Link;

    fn next(&mut self) -> Option<Link> {
        loop {
            if !self.queue.is_empty() {
                let link = match self.queue.remove(0) {
                    Step::Link(link) => link,
                    Step::Landing(page) => match self.search.landing(&page) {
                        Some(link) => link,
                        None => continue,
                    },
                };
                if self.seen.insert(link.url.clone()) {
                    return Some(link);
                }
                continue;
            }
            self.stage += 1;
            match self.stage {
                1 if self.search.doi.is_some() => {
                    if let Some(pdf) = self.search.page().0 {
                        self.queue
                            .push(Step::Link(Link::new(pdf, Version::Published)));
                    }
                }
                2 if self.search.doi.is_some() => {
                    // version by version: the PDF links, then the landing pages (not the
                    // publisher's page, read already)
                    let (links, landings) = self.search.openalex();
                    let doi = self.search.doi.clone().unwrap_or_default();
                    let publisher = [
                        format!("https://doi.org/{doi}").to_lowercase(),
                        self.search
                            .page_final
                            .clone()
                            .unwrap_or_default()
                            .to_lowercase(),
                    ];
                    for version in self.search.versions {
                        self.queue.extend(
                            links
                                .iter()
                                .filter(|link| link.version == *version)
                                .cloned()
                                .map(Step::Link),
                        );
                        self.queue.extend(
                            landings
                                .iter()
                                .filter(|page| {
                                    page.version == *version
                                        && !publisher.contains(&page.url.to_lowercase())
                                })
                                .cloned()
                                .map(Step::Landing),
                        );
                    }
                }
                3 => {
                    let Some(arxiv) = self.search.arxiv.clone() else {
                        continue;
                    };
                    if !self.search.versions.contains(&Version::Submitted) {
                        continue;
                    }
                    let id = arxiv.to_lowercase();
                    let known = self.seen.iter().any(|url| {
                        let url = url.to_lowercase();
                        url.contains(&format!("arxiv.org/pdf/{id}"))
                            || url.contains(&format!("arxiv.org/abs/{id}"))
                    });
                    if !known {
                        self.search
                            .browser
                            .push(format!("https://arxiv.org/abs/{arxiv}"));
                        self.queue.push(Step::Link(Link::new(
                            format!("https://arxiv.org/pdf/{arxiv}"),
                            Version::Submitted,
                        )));
                    }
                }
                1 | 2 => {}
                _ => return None,
            }
        }
    }
}

// the downloads of the machine: URL -> checksum
// =============================================

/// The checksum (sha256, in hexadecimal) and the size of the file that a URL gave the last time it
/// was downloaded on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Known {
    pub sha256: String,
    pub size: u64,
}

/// The key of a URL in the cache of the downloads (`downloads/`): the start of its sha256.
fn download_key(url: &str) -> String {
    let digest = Sha256::digest(url.as_bytes());
    digest
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()[..32]
        .to_string()
}

/// A PDF found for a link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum How {
    /// downloaded: the path is a temporary file
    Downloaded,
    /// a file of the entry has the content that the link gave
    Has,
    /// the file of another entry (its key) has it
    Linked(String),
    /// a file of the library that no entry links to (`.papers/files.json`) has it
    Known,
}

/// A PDF for a link (`Found`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub path: String,
    pub link: Link,
    pub how: How,
    /// The links of the published version that were refused before this one, an accepted or
    /// submitted version, was found (to open in a browser).
    pub refused: Vec<String>,
}

impl Found {
    pub fn new(path: impl Into<String>, link: Link, how: How) -> Found {
        Found {
            path: path.into(),
            link,
            how,
            refused: Vec::new(),
        }
    }

    /// e.g. `accepted manuscript, from repo.example.org; the published version was refused: open
    /// URL in a browser`
    pub fn describe(&self) -> String {
        if self.refused.is_empty() {
            return self.link.describe();
        }
        format!(
            "{}; the published version was refused: open {} in a browser",
            self.link.describe(),
            self.refused.join(", ")
        )
    }
}

/// No PDF could be had (`NoPDF`): the message ends with the links to open in a browser (`links`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoPdf {
    pub message: String,
    pub links: Vec<String>,
}

impl std::fmt::Display for NoPdf {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// What the caller has for a URL downloaded before (see [`fetch_pdf`]).
pub type Local<'l> = dyn FnMut(&Link, &Known) -> Option<Found> + 'l;

impl Fetcher {
    /// GET a URL after the wait for its site, and again once after the wait that an HTTP 429
    /// asks (`_request`).
    fn request(&self, url: &str, accept: Option<&str>) -> super::Result<Response> {
        wait(url);
        let response = self.get(url, accept)?;
        if response.status != 429 {
            return Ok(response);
        }
        let seconds = retry_after(response.retry_after.as_deref());
        log::debug(&format!(
            "{url}: HTTP 429, retried in {} s",
            seconds_text(seconds)
        ));
        let sleep = *SLEEP
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // (a NaN, which Python's sleep refuses, waits nothing)
        sleep(Duration::try_from_secs_f64(seconds).unwrap_or_default());
        wait(url);
        self.get(url, accept)
    }

    /// A page and its URL after the redirections, as papers asks for it (`_fetch_page`).
    fn fetch_page(&self, url: &str) -> super::Result<Response> {
        self.request(url, None)
    }

    /// What the URL gave the last time it was downloaded on this machine (`known_download`).
    pub fn known_download(&self, url: &str) -> Option<Known> {
        let data = self.cache.read("downloads", &download_key(url))?;
        if data.get("url").and_then(Value::as_str) != Some(url) {
            return None;
        }
        let sha256 = data
            .get("sha256")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())?;
        let size = data.get("size").and_then(Value::as_u64)?;
        Some(Known {
            sha256: sha256.to_string(),
            size,
        })
    }

    /// Remember what a URL gave (`remember_download`).
    fn remember_download(&self, url: &str, path: &str) {
        let (Ok(sha256), Ok(meta)) = (crate::merge::checksum_hex(path), std::fs::metadata(path))
        else {
            return;
        };
        self.cache.put(
            "downloads",
            &download_key(url),
            json!({"url": url, "sha256": sha256, "size": meta.len()}),
        );
    }

    /// Download a link into a temporary folder (`download`): the path, or the error (an HTTP
    /// error, no answer, an answer that is not a PDF). Its checksum is remembered.
    pub fn download_pdf(&self, link: &Link) -> Result<String, String> {
        let response = self
            .request(&link.url, Some(accept(true)))
            .map_err(|e| connection_failed(&link.url, "ConnectionError", &e.message))?;
        let path = save_download(
            &link.url,
            response.status,
            &response.content_type,
            false,
            response.body,
            true,
            None,
        )?;
        let head = std::fs::read(&path)
            .map(|b| b[..b.len().min(1024)].to_vec())
            .unwrap_or_default();
        if !head.windows(4).any(|w| w == b"%PDF") {
            discard(&path);
            return Err(format!("{}: not a PDF", link.url));
        }
        self.remember_download(&link.url, &path);
        Ok(path)
    }

    /// The PDF of a link: what `local` finds for a URL downloaded before, else the download (`get`).
    /// What `local` finds for a URL downloaded before ([`Fetcher::known_download`]), or none
    /// (`local_copy`).
    pub fn local_copy(&self, link: &Link, local: Option<&mut Local>) -> Option<Found> {
        let local = local?;
        let known = self.known_download(&link.url)?;
        local(link, &known)
    }

    fn get_link(&self, link: &Link, local: Option<&mut Local>) -> Result<Found, String> {
        if let Some(found) = self.local_copy(link, local) {
            return Ok(found);
        }
        Ok(Found::new(
            self.download_pdf(link)?,
            link.clone(),
            How::Downloaded,
        ))
    }
}

/// Remove a downloaded file and its temporary folder (`discard`).
pub fn discard(path: &str) {
    let _ = std::fs::remove_dir_all(dirname(path));
}

/// The PDF of the first link of the search that gives one (`fetch_pdf`): what `local` finds for a
/// URL downloaded before, else the download. The error, when none does, ends with the links to
/// open in a browser ([`no_pdf_message`], [`browser_links`]).
pub fn fetch_pdf(search: &mut Search, local: Option<&mut Local>) -> Result<Found, NoPdf> {
    first_pdf(search, local).map_err(|tried| NoPdf {
        message: no_pdf_message(search, &tried),
        links: browser_links(search, &tried),
    })
}

/// The PDF of the first link of the search that gives one, as [`fetch_pdf`] finds it (an accepted
/// or submitted version with the published links refused before it: the links of the published
/// version tried, then the publisher's page when it gave an error); else the links tried, each with
/// its error.
pub fn first_pdf(
    search: &mut Search,
    mut local: Option<&mut Local>,
) -> Result<Found, Vec<(Link, String)>> {
    let fetcher = search.fetcher;
    let mut tried: Vec<(Link, String)> = Vec::new();
    // the links are read one by one: the next source only when a link failed
    let mut found = None;
    for link in search.links() {
        match fetcher.get_link(&link, local.as_deref_mut()) {
            Ok(f) => {
                found = Some(f);
                break;
            }
            Err(error) => {
                log::info(&format!("{} ({}): {error}", link.url, link.version.name()));
                tried.push((link, error));
            }
        }
    }
    let Some(mut found) = found else {
        return Err(tried);
    };
    if found.link.version != Version::Published {
        found.refused = tried
            .iter()
            .filter(|(link, _)| link.version == Version::Published)
            .map(|(link, _)| link.url.clone())
            .collect();
        if let Some(page) = &search.page_refused
            && !found.refused.contains(page)
        {
            found.refused.push(page.clone());
        }
    }
    Ok(found)
}

/// The supplements that give a PDF (`fetch_supplements`); the others are left out with a warning.
pub fn fetch_supplements(search: &mut Search, mut local: Option<&mut Local>) -> Vec<Found> {
    let fetcher = search.fetcher;
    let mut found = Vec::new();
    for link in search.supplements() {
        match fetcher.get_link(&link, local.as_deref_mut()) {
            Ok(f) => found.push(f),
            Err(error) => log::warning(&format!(
                "supplement {}: {} (not attached)",
                link.url,
                reason(&error)
            )),
        }
    }
    found
}

/// The short reason of a refused download (`_reason`): `HTTP 403`, `not a PDF`, else the first
/// sentence of the error.
pub fn reason(error: &str) -> String {
    for (i, _) in error.match_indices("HTTP ") {
        let digits: String = error[i + 5..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        if !digits.is_empty() {
            return format!("HTTP {digits}");
        }
    }
    if error.contains("did not return a PDF") || error.contains("not a PDF") {
        return "not a PDF".to_string();
    }
    error.split(". ").next().unwrap_or("").to_string()
}

/// The error of a search that gave no PDF (`no_pdf_message`): the links tried with their reason,
/// what the sources said, and the links to open in a browser.
pub fn no_pdf_message(search: &Search, tried: &[(Link, String)]) -> String {
    if search.doi.is_none() && search.arxiv.is_none() {
        return "no DOI or arXiv id to look for a PDF".to_string();
    }
    let mut lines = vec![
        if tried.is_empty() {
            "no free PDF found"
        } else {
            "no PDF could be downloaded"
        }
        .to_string(),
    ];
    for (link, error) in tried {
        lines.push(format!(
            "  {} ({}): {}",
            link.url,
            link.version.name(),
            reason(error)
        ));
    }
    lines.extend(search.notes.iter().map(|note| format!("  {note}")));
    let browser = browser_links(search, tried);
    if !browser.is_empty() {
        lines.push(format!("open in a browser: {}", browser.join(", ")));
    }
    lines.join("\n")
}

/// The links to open in a browser after a search that gave no PDF: the links tried, then the
/// pages of the paper that were not tried (the publisher's, the open-access landing pages, arXiv).
pub fn browser_links(search: &Search, tried: &[(Link, String)]) -> Vec<String> {
    let tried: Vec<String> = tried.iter().map(|(link, _)| link.url.clone()).collect();
    let pages: Vec<String> = search
        .browser
        .iter()
        .filter(|url| !tried.contains(url))
        .cloned()
        .collect();
    [tried, pages].concat()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_links_of_a_page() {
        let html = r#"<html><head><meta name="citation_pdf_url" content="/a/b.pdf"></head><body>
<a href="suppl/x.PDF">S</a><a href="y.pdf">Supplementary <b>data</b></a><a href="z.pdf">PDF</a>
<a href="/a/b.pdf">Supplement</a></body></html>"#;
        let (pdf, supplements) = page_links(html, "https://j.example.org/a/page");
        assert_eq!(pdf.as_deref(), Some("https://j.example.org/a/b.pdf"));
        assert_eq!(
            supplements,
            [
                "https://j.example.org/a/suppl/x.PDF",
                "https://j.example.org/a/y.pdf"
            ]
        );
    }

    #[test]
    fn reasons() {
        assert_eq!(
            reason(
                "Failed to download from https://x.org/a.pdf: HTTP 403 (content-type: text/html)"
            ),
            "HTTP 403"
        );
        assert_eq!(reason("https://x.org/a: not a PDF"), "not a PDF");
        assert_eq!(
            reason(
                "URL did not return a PDF: https://x.org/a (content-type: text/html). The server"
            ),
            "not a PDF"
        );
        assert_eq!(
            reason("Failed: ConnectionError: refused. The connection"),
            "Failed: ConnectionError: refused"
        );
    }

    #[test]
    fn hosts() {
        assert_eq!(
            host("https://journals.example.org/a/b.pdf?x=1"),
            "journals.example.org"
        );
        assert_eq!(host("http://x.org:8080"), "x.org:8080");
        assert_eq!(host("x"), "");
    }
}
