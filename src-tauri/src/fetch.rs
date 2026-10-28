//! What the sheet that adds a paper reads from PDFs and from the network, all of it read-only,
//! whatever writes the library: the DOI of a PDF, the entry of a DOI, the DOI of a journal
//! article's page, a free PDF of an article, and the download of that PDF. The network is only
//! used when asked (Add…), and always through papers-rs: the same queries and cache as `papers
//! add`, with the app's user agent and the e-mail address and OpenAlex key of the settings, which
//! are sent when set, to the services that take them.

use std::path::{Path, PathBuf};

use papers_rs::bib::Bib;
use papers_rs::config::dirs::cache_dir;
use papers_rs::extract::doi::parse_dois;
use papers_rs::extract::{Options, PdfFile, confirm_doi, extract_txt_metadata};
use papers_rs::fetch::download::download_url;
use papers_rs::fetch::html::page_doi;
use papers_rs::fetch::http::{Network, Transport};
use papers_rs::fetch::openalex::OpenAlex;
use papers_rs::fetch::{ErrorKind, FetchError, Fetcher};
use serde::Serialize;
use tauri::State;

use crate::AppState;
use crate::settings::Settings;

/// The DOI of what was typed or pasted: `10.1000/abc`, a doi.org or
/// dx.doi.org link to it, or `doi:10.1000/abc`. None for anything else.
pub fn doi_of(input: &str) -> Option<String> {
    const PREFIXES: [&str; 5] = [
        "https://doi.org/",
        "http://doi.org/",
        "https://dx.doi.org/",
        "http://dx.doi.org/",
        "doi:",
    ];
    let input = input.trim();
    let rest = PREFIXES
        .iter()
        .find_map(|p| {
            let start = input.get(..p.len())?;
            start.eq_ignore_ascii_case(p).then(|| &input[p.len()..])
        })
        .unwrap_or(input)
        .trim();
    // 10.<registrant of 4 to 9 digits>/<suffix>, no white space
    let (registrant, suffix) = rest.strip_prefix("10.")?.split_once('/')?;
    let valid = (4..=9).contains(&registrant.len())
        && registrant.bytes().all(|b| b.is_ascii_digit())
        && !suffix.is_empty()
        && !rest.contains(char::is_whitespace);
    valid.then(|| rest.to_string())
}

/// How the app identifies itself to Crossref, doi.org and OpenAlex: with the e-mail address of
/// the settings when there is one, as they recommend.
fn user_agent(mailto: Option<&str>) -> String {
    let agent = format!("Papers Desktop/{}", env!("CARGO_PKG_VERSION"));
    match mailto {
        Some(email) => format!("{agent} (mailto:{email})"),
        None => agent,
    }
}

/// The queries of papers, with the user agent of the app, and papers' own cache.
fn fetcher(mailto: Option<&str>) -> Result<Fetcher, String> {
    let network = Network::with_user_agent(&user_agent(mailto))?;
    Ok(Fetcher::with_transport(Box::new(network), &cache_dir()))
}

/// What the services are told about the user.
struct Access {
    mailto: Option<String>,
    openalex: OpenAlex,
}

impl Access {
    fn of(state: &State<'_, AppState>) -> Result<Access, String> {
        let settings = Settings::load(&state.settings_path)?;
        Ok(Access {
            mailto: settings.mailto().map(str::to_string),
            openalex: settings.openalex(),
        })
    }
}

/// A failure of a query, as a message to show.
fn failed(error: &FetchError, doi: &str) -> String {
    match error.kind {
        ErrorKind::NotFound => format!("No paper with DOI {doi}"),
        ErrorKind::Other => format!("Not reachable: {}", error.message),
        _ => error.message.clone(),
    }
}

/// The BibTeX of a DOI as papers makes it (Crossref, doi.org for the other registration agencies,
/// arXiv for their DOIs), one field per line, as the library has its entries.
pub fn fetch_entry(fetcher: &Fetcher, doi: &str) -> Result<String, String> {
    let doi = doi_of(doi).ok_or_else(|| format!("{}: not a DOI", doi.trim()))?;
    let bibtex = fetcher
        .fetch_bibtex_by_doi(&doi)
        .map_err(|e| failed(&e, &doi))?;
    Ok(formatted(&bibtex))
}

/// BibTeX as papers writes it, one field per line: what the entry will look like in the library.
fn formatted(text: &str) -> String {
    Bib::parse(text).to_string().trim().to_string()
}

/// The BibTeX of a DOI, for the sheet.
#[tauri::command]
pub async fn fetch_doi(state: State<'_, AppState>, doi: String) -> Result<String, String> {
    let access = Access::of(&state)?;
    blocking(move || fetch_entry(&fetcher(access.mailto.as_deref())?, &doi)).await
}

/// Where a PDF's DOI was read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// in the metadata of the PDF
    Metadata,
    /// in its text, and the title of the DOI is in the text too
    Text,
    /// a search of Crossref with the text found the paper
    Search,
}

/// What `papers add FILE.pdf` finds of a PDF.
#[derive(Debug, Serialize)]
pub struct PdfRead {
    pub doi: Option<String>,
    pub source: Option<Source>,
    /// the entry of the DOI, fetched
    pub bibtex: Option<String>,
    /// why there is no DOI or no entry
    pub message: Option<String>,
}

impl PdfRead {
    fn nothing(message: impl Into<String>) -> PdfRead {
        PdfRead {
            doi: None,
            source: None,
            bibtex: None,
            message: Some(message.into()),
        }
    }
}

/// The DOI of a PDF and its entry, by the steps of `papers add FILE.pdf` (papers.extract): the
/// DOI in the metadata, trusted; else a DOI of the text, taken if its title is in the text; else
/// a search of Crossref by the text. A PDF that cannot be read, or a service that cannot be
/// reached, is an error; a PDF with no DOI is not.
pub fn read_pdf(fetcher: &Fetcher, path: &Path) -> Result<PdfRead, String> {
    let path = path.to_string_lossy();
    let file = PdfFile::new(&path);
    let message = |e: FetchError| e.message;
    if let Some(doi) = file.metadata_doi().map_err(message)? {
        let found = fetcher.fetch_bibtex_by_doi(&doi);
        return Ok(PdfRead {
            source: Some(Source::Metadata),
            bibtex: found.as_ref().ok().map(|b| formatted(b)),
            message: found.as_ref().err().map(|e| failed(e, &doi)),
            doi: Some(doi),
        });
    }
    let text = file.head(10, 200, false).map_err(message)?;
    let tail = file.tail().map_err(message)?;
    let dois = parse_dois(&text, &tail).unwrap_or_default();
    if let Some((doi, bibtex)) = confirm_doi(fetcher, &dois, &text).map_err(message)? {
        return Ok(PdfRead {
            doi: Some(doi),
            source: Some(Source::Text),
            bibtex: Some(formatted(&bibtex)),
            message: None,
        });
    }
    let search = Options {
        search_doi: false,
        ..Options::default()
    };
    match extract_txt_metadata(fetcher, &text, &search, &tail) {
        Ok(bibtex) => {
            let bibtex = formatted(&bibtex);
            let doi = Bib::parse(&bibtex)
                .entries()
                .next()
                .and_then(|e| e.get("doi"))
                .map(str::to_string);
            Ok(PdfRead {
                doi,
                source: Some(Source::Search),
                bibtex: Some(bibtex),
                message: None,
            })
        }
        Err(_) => Ok(PdfRead::nothing("No DOI found in the PDF")),
    }
}

/// The DOI of a PDF and its entry, for the sheet.
#[tauri::command]
pub async fn read_pdf_doi(state: State<'_, AppState>, path: PathBuf) -> Result<PdfRead, String> {
    let access = Access::of(&state)?;
    blocking(move || read_pdf(&fetcher(access.mailto.as_deref())?, &path)).await
}

/// What a link to a page says of its article.
#[derive(Debug, Serialize)]
pub struct Page {
    /// where the link led (after redirects)
    pub url: String,
    /// the DOI the page gives the article, if it does
    pub doi: Option<String>,
    /// the link is a PDF, not a page
    pub pdf: bool,
}

/// The DOI of the article of a journal's page: the tags `citation_doi`, `prism.doi` and
/// `dc.identifier` of the page.
pub fn read_page(transport: &dyn Transport, url: &str) -> Result<Page, String> {
    let response = transport
        .get(url, Some("text/html,application/xhtml+xml,*/*;q=0.5"))
        .map_err(|e| format!("Not reachable: {e}"))?;
    if let Some(error) = response.status_error() {
        return Err(format!("The page cannot be read: {error}"));
    }
    let pdf = response.content_type.to_lowercase().contains("pdf");
    Ok(Page {
        doi: if pdf {
            None
        } else {
            page_doi(&response.text())
        },
        url: response.url,
        pdf,
    })
}

/// The DOI of the article a page is, for the sheet. Publishers answer programs less than
/// browsers: the user agent says it is a program, and what it is.
#[tauri::command]
pub async fn journal_page(state: State<'_, AppState>, url: String) -> Result<Page, String> {
    let access = Access::of(&state)?;
    blocking(move || {
        let agent = format!(
            "Mozilla/5.0 (compatible; {})",
            user_agent(access.mailto.as_deref())
        );
        read_page(&Network::with_user_agent(&agent)?, &url)
    })
    .await
}

/// A link to a free PDF of the article with this DOI, if OpenAlex has the work and lists one.
#[tauri::command]
pub async fn free_pdf(state: State<'_, AppState>, doi: String) -> Result<Option<String>, String> {
    let access = Access::of(&state)?;
    blocking(move || {
        fetcher(access.mailto.as_deref())?
            .openalex_free_pdf(&doi, &access.openalex)
            .map_err(|e| failed(&e, &doi))
    })
    .await
}

/// Download a PDF into a folder of its own in the temporary folder (where papers knows a
/// download, which it always places in the library's folder of files); its path.
#[tauri::command]
pub async fn download_pdf(url: String) -> Result<PathBuf, String> {
    blocking(move || download_url(&url, true, None).map(PathBuf::from)).await
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use papers_rs::fetch::http::Response;

    use super::*;

    #[test]
    fn user_agent_with_and_without_email() {
        let version = env!("CARGO_PKG_VERSION");
        assert_eq!(user_agent(None), format!("Papers Desktop/{version}"));
        assert_eq!(
            user_agent(Some("me@example.org")),
            format!("Papers Desktop/{version} (mailto:me@example.org)")
        );
    }

    #[test]
    fn formatted_one_field_per_line() {
        let text = "@article{WATSON_1953, title={Molecular Structure}, DOI={10.1038/171737a0}, month=Apr, year={1953} }";
        assert_eq!(
            formatted(text),
            "@article{WATSON_1953,\n doi = {10.1038/171737a0},\n month = Apr,\n title = {Molecular Structure},\n year = {1953}\n}"
        );
    }

    #[test]
    fn doi_forms() {
        let doi = Some("10.1000/abc.def-1".to_string());
        for input in [
            "10.1000/abc.def-1",
            "  10.1000/abc.def-1\n",
            "doi:10.1000/abc.def-1",
            "DOI: 10.1000/abc.def-1",
            "https://doi.org/10.1000/abc.def-1",
            "HTTP://DX.DOI.ORG/10.1000/abc.def-1",
        ] {
            assert_eq!(doi_of(input), doi, "{input}");
        }
        // case is kept: doi.org does not mind, the library compares without it
        assert_eq!(doi_of("10.1000/ABC"), Some("10.1000/ABC".to_string()));
    }

    #[test]
    fn not_dois() {
        for input in [
            "",
            "10.1000",
            "10.1000/",
            "10.12/abc",
            "10.x1000/abc",
            "11.1000/abc",
            "10.1000/a b",
            "https://example.org/10.1000/abc",
            "@article{a, title = {B}}",
            "some words",
        ] {
            assert_eq!(doi_of(input), None, "{input}");
        }
    }

    /// The answers of a network that is not there: by the start of the URL, and the URLs asked.
    #[derive(Default)]
    pub struct Canned {
        answers: HashMap<String, (u16, &'static str, String)>,
        asked: Mutex<Vec<String>>,
    }

    impl Canned {
        pub fn with(
            mut self,
            url: &str,
            status: u16,
            content_type: &'static str,
            body: &str,
        ) -> Self {
            self.answers
                .insert(url.to_lowercase(), (status, content_type, body.to_string()));
            self
        }

        pub fn asked(&self) -> Vec<String> {
            self.asked.lock().unwrap().clone()
        }
    }

    impl Transport for Canned {
        fn get(&self, url: &str, _accept: Option<&str>) -> Result<Response, String> {
            self.asked.lock().unwrap().push(url.to_string());
            let lower = url.to_lowercase();
            let (status, content_type, body) = self
                .answers
                .iter()
                .find(|(start, _)| lower.starts_with(start.as_str()))
                .map(|(_, a)| a.clone())
                .unwrap_or((404, "text/plain", String::new()));
            Ok(Response {
                status,
                reason: if status == 200 { "OK" } else { "Not Found" }.into(),
                content_type: content_type.into(),
                body: body.into_bytes(),
                url: url.into(),
            })
        }
    }

    /// What Crossref says of the paper that the PDFs of the tests are about.
    const CROSSREF: &str = r#"{"status": "ok", "message": {"DOI": "10.1234/ice.2020.1",
        "type": "journal-article", "URL": "https://doi.org/10.1234/ice.2020.1",
        "title": ["Ice sheet response to warming in the early Holocene"],
        "author": [{"given": "Ada", "family": "Lovelace"}, {"given": "Charles", "family": "Babbage"}],
        "container-title": ["Journal of Things"], "volume": "7", "page": "10-20",
        "issued": {"date-parts": [[2020]]}}}"#;

    fn pdf(name: &str) -> PathBuf {
        let fixtures =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../crates/papers-rs/tests/fixtures/pdf");
        fixtures.join(name)
    }

    /// Queries over canned answers, with a cache folder of their own.
    fn queries(canned: Canned) -> (Fetcher, tempfile::TempDir) {
        let cache = tempfile::tempdir().unwrap();
        let fetcher = Fetcher::with_transport(Box::new(canned), cache.path().to_str().unwrap());
        (fetcher, cache)
    }

    #[test]
    fn the_entry_of_a_doi_as_papers_makes_it() {
        let canned = Canned::default().with(
            "https://api.crossref.org/works/10.1234/ice.2020.1",
            200,
            "application/json",
            CROSSREF,
        );
        let (fetcher, _cache) = queries(canned);
        let bibtex = fetch_entry(&fetcher, "https://doi.org/10.1234/ice.2020.1").unwrap();
        assert!(bibtex.starts_with("@article{"), "{bibtex}");
        assert!(
            bibtex.contains("title = {Ice sheet response to warming in the early Holocene}"),
            "{bibtex}"
        );
        assert!(bibtex.contains("doi = {10.1234/ice.2020.1}"), "{bibtex}");
        // what is not a DOI is not asked for, and a DOI nobody has is said so
        assert_eq!(
            fetch_entry(&fetcher, "hello").unwrap_err(),
            "hello: not a DOI"
        );
        let error = fetch_entry(&fetcher, "10.9999/nothing").unwrap_err();
        assert_eq!(error, "No paper with DOI 10.9999/nothing");
    }

    #[test]
    fn the_doi_of_a_pdf_from_its_metadata() {
        let canned = Canned::default().with(
            "https://api.crossref.org/works/10.1234/ice.2020.1",
            200,
            "application/json",
            CROSSREF,
        );
        let (fetcher, _cache) = queries(canned);
        let read = read_pdf(&fetcher, &pdf("xmp-doi.pdf")).unwrap();
        assert_eq!(read.doi.as_deref(), Some("10.1234/ICE.2020.1"));
        assert_eq!(read.source, Some(Source::Metadata));
        assert!(read.bibtex.unwrap().contains("Journal of Things"));
        assert_eq!(read.message, None);
    }

    #[test]
    fn the_doi_of_a_pdf_from_its_text_when_the_title_is_there() {
        let canned = Canned::default().with(
            "https://api.crossref.org/works/10.1234/ice.2020.1",
            200,
            "application/json",
            CROSSREF,
        );
        let (fetcher, _cache) = queries(canned);
        let read = read_pdf(&fetcher, &pdf("text-doi.pdf")).unwrap();
        assert_eq!(read.doi.as_deref(), Some("10.1234/ice.2020.1"));
        assert_eq!(read.source, Some(Source::Text));
        assert!(read.bibtex.is_some());
    }

    #[test]
    fn a_pdf_without_a_doi_says_so() {
        let (fetcher, _cache) = queries(Canned::default());
        let read = read_pdf(&fetcher, &pdf("no-doi.pdf")).unwrap();
        assert_eq!((read.doi, read.source, read.bibtex), (None, None, None));
        assert_eq!(read.message.as_deref(), Some("No DOI found in the PDF"));
        // what is not a PDF is an error
        let dir = tempfile::tempdir().unwrap();
        let text = dir.path().join("a.pdf");
        std::fs::write(&text, "not a pdf").unwrap();
        assert!(read_pdf(&fetcher, &text).is_err());
    }

    #[test]
    fn the_metadata_doi_is_kept_when_its_entry_cannot_be_fetched() {
        let (fetcher, _cache) = queries(Canned::default());
        let read = read_pdf(&fetcher, &pdf("xmp-doi.pdf")).unwrap();
        assert_eq!(read.doi.as_deref(), Some("10.1234/ICE.2020.1"));
        assert_eq!(read.bibtex, None);
        assert_eq!(
            read.message.as_deref(),
            Some("No paper with DOI 10.1234/ICE.2020.1")
        );
    }

    const PAGE: &str = r#"<html><head><title>Ice</title>
        <meta name="citation_title" content="Ice sheet response">
        <meta name="citation_doi" content="10.1234/ice.2020.1">
        <meta name="citation_pdf_url" content="https://example.org/ice.pdf"></head><body></body></html>"#;

    #[test]
    fn the_doi_of_a_journals_page() {
        let canned = Canned::default()
            .with(
                "https://example.org/article",
                200,
                "text/html; charset=utf-8",
                PAGE,
            )
            .with(
                "https://example.org/nometa",
                200,
                "text/html",
                "<html></html>",
            )
            .with(
                "https://example.org/ice.pdf",
                200,
                "application/pdf",
                "%PDF-1.4",
            )
            .with("https://example.org/closed", 403, "text/html", "");
        let page = |url| read_page(&canned, url);
        let article = page("https://example.org/article").unwrap();
        assert_eq!(article.doi.as_deref(), Some("10.1234/ice.2020.1"));
        assert!(!article.pdf);
        assert_eq!(page("https://example.org/nometa").unwrap().doi, None);
        let pdf = page("https://example.org/ice.pdf").unwrap();
        assert!(pdf.pdf && pdf.doi.is_none());
        let error = page("https://example.org/closed").unwrap_err();
        assert!(error.starts_with("The page cannot be read: 403"), "{error}");
    }

    #[test]
    fn a_free_pdf_from_openalex_with_the_address_and_the_key() {
        let work = r#"{"id": "https://openalex.org/W1", "best_oa_location": {"pdf_url": "https://example.org/free.pdf"}}"#;
        let canned = Canned::default().with(
            "https://api.openalex.org/works/doi:10.1234/ice.2020.1",
            200,
            "application/json",
            work,
        );
        let cache = tempfile::tempdir().unwrap();
        let canned = std::sync::Arc::new(canned);
        struct Shared(std::sync::Arc<Canned>);
        impl Transport for Shared {
            fn get(&self, url: &str, accept: Option<&str>) -> Result<Response, String> {
                self.0.get(url, accept)
            }
        }
        let fetcher = Fetcher::with_transport(
            Box::new(Shared(canned.clone())),
            cache.path().to_str().unwrap(),
        );
        let access = OpenAlex {
            mailto: Some("me@example.org".into()),
            api_key: Some("KEY".into()),
        };
        let found = fetcher
            .openalex_free_pdf("10.1234/ice.2020.1", &access)
            .unwrap();
        assert_eq!(found.as_deref(), Some("https://example.org/free.pdf"));
        let asked = canned.asked();
        assert_eq!(asked.len(), 1);
        assert!(
            asked[0].contains("mailto=me%40example.org") && asked[0].contains("api_key=KEY"),
            "{}",
            asked[0]
        );
        // a work OpenAlex does not have has no free PDF
        assert_eq!(
            fetcher
                .openalex_free_pdf("10.9999/none", &OpenAlex::default())
                .unwrap(),
            None
        );
    }

    #[test]
    #[ignore = "needs the network"]
    fn fetch() {
        let fetcher = fetcher(None).unwrap();
        let bibtex = fetch_entry(&fetcher, "10.1038/171737a0").unwrap();
        assert!(bibtex.starts_with('@'), "{bibtex}");
        let none = fetch_entry(&fetcher, "10.1000/no-such-paper-xyz");
        assert_eq!(
            none.unwrap_err(),
            "No paper with DOI 10.1000/no-such-paper-xyz"
        );
    }
}
