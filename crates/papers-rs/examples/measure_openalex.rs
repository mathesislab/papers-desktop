//! What the search of OpenAlex (`--openalex`) adds to the default chain of papers (crossref),
//! measured on a set of PDFs (read only: nothing is written next to them). For each PDF of a
//! random sample, in this order:
//!
//! 1. papers' chain: the DOI of the PDF metadata or of the text, and else crossref's full-text
//!    search (`extract_pdf_metadata` as `papers extract`): found, or not (no hit, or a hit whose
//!    title is not in the text).
//! 2. For those where it is not found (and, with `--agree N`, N of those where it is found), the
//!    same chain with OpenAlex in place of crossref's search: found, or not; the number of works
//!    that the search has with their title in the text, and the place of the first in the
//!    semantic order of OpenAlex (what the number of works asked for must reach).
//!
//! The text sent to crossref and OpenAlex is the query that papers makes of the PDF (the first
//! 200 words). A line of JSON for each PDF goes to the standard output, a summary to the
//! standard error.
//!
//!     CARGO_PROFILE_RELEASE_PANIC=unwind cargo run --release --example measure_openalex -- \
//!         DIR --sample 150 --seed 1 --cache CACHE_DIR --budget 90 [--agree 10] > measure.jsonl
//!
//! `--cache` is a folder of the cache of the queries, not that of papers (the answers of crossref
//! and OpenAlex are kept there, so that a second run asks for nothing). `--budget` is the most
//! searches of OpenAlex (a keyless use has about 100 a day; a PDF asked costs two: the search of
//! 50 works, then that of the chain, of 10).

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use papers_rs::bib::Bib;
use papers_rs::extract::doi::{parse_dois, query_text, title_in_text_strict};
use papers_rs::extract::{Options, PdfFile, extract_pdf_metadata};
use papers_rs::fetch::Fetcher;
use papers_rs::fetch::http::{Network, Response, Transport};
use papers_rs::fetch::openalex::{self, OpenAlex, work_title};
use serde_json::json;

/// The network, counting the requests to OpenAlex (and refusing those over the budget), and
/// keeping a distance between the others.
struct Counted {
    network: Network,
    openalex: Arc<AtomicUsize>,
    budget: usize,
    last: Mutex<Instant>,
}

impl Transport for Counted {
    fn get(&self, url: &str, accept: Option<&str>) -> Result<Response, String> {
        if url.starts_with(openalex::API) {
            if self.openalex.fetch_add(1, Ordering::SeqCst) >= self.budget {
                self.openalex.fetch_sub(1, Ordering::SeqCst);
                return Err("the budget of searches of OpenAlex is spent".to_string());
            }
        } else {
            let mut last = self.last.lock().unwrap();
            let wait = Duration::from_millis(250).saturating_sub(last.elapsed());
            std::thread::sleep(wait);
            *last = Instant::now();
        }
        self.network.get(url, accept)
    }
}

fn shuffled(mut files: Vec<String>, seed: u64) -> Vec<String> {
    // a Fisher-Yates shuffle with a small generator: the same order for the same seed
    let mut state = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    for i in (1..files.len()).rev() {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        files.swap(i, (state >> 33) as usize % (i + 1));
    }
    files
}

fn find_pdfs(dir: &Path, found: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("_archive") || name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            find_pdfs(&path, found);
        } else if name.to_lowercase().ends_with(".pdf") {
            found.push(path.to_string_lossy().into_owned());
        }
    }
}

fn bibtex_fields(bibtex: &str) -> (String, String) {
    let bib = Bib::parse(bibtex);
    let entry = bib.entries().next();
    let get = |k: &str| entry.and_then(|e| e.get(k)).unwrap_or("").to_string();
    (get("title"), get("doi"))
}

fn main() {
    // the PDFs that pdf-extract cannot read panic: that is a failed file here
    std::panic::set_hook(Box::new(|_| {}));
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let dir = args.first().expect("DIR").clone();
    let sample: usize = flag("--sample").map_or(100, |s| s.parse().unwrap());
    let seed: u64 = flag("--seed").map_or(1, |s| s.parse().unwrap());
    let budget: usize = flag("--budget").map_or(90, |s| s.parse().unwrap());
    let agree: usize = flag("--agree").map_or(0, |s| s.parse().unwrap());
    let cache = flag("--cache").expect("--cache DIR");
    let skip: usize = flag("--skip").map_or(0, |s| s.parse().unwrap());

    let mut files = Vec::new();
    find_pdfs(Path::new(&dir), &mut files);
    files.sort();
    let total = files.len();
    let files = shuffled(files, seed);

    let counter = Arc::new(AtomicUsize::new(0));
    let transport = Counted {
        network: Network::new().unwrap(),
        openalex: counter.clone(),
        budget,
        last: Mutex::new(Instant::now()),
    };
    let fetcher = Fetcher::with_transport(Box::new(transport), &cache);
    let access = OpenAlex::from_env();

    let (mut n, mut unreadable, mut found, mut missed) = (0, 0, 0, 0);
    let (mut gained, mut with_doi, mut agreed_asked, mut agreed) = (0, 0, 0, 0);
    let mut first_place: Vec<Option<usize>> = Vec::new();
    for path in files.iter().skip(skip).take(sample) {
        n += 1;
        let text = std::panic::catch_unwind(|| {
            let file = PdfFile::new(path);
            let head = file.head(10, 200, false)?;
            let tail = file.tail()?;
            let meta = file.metadata_doi()?;
            Ok::<_, papers_rs::fetch::FetchError>((head, tail, meta))
        });
        let Ok(Ok((head, tail, meta))) = text else {
            unreadable += 1;
            println!("{}", json!({"pdf": path, "unreadable": true}));
            continue;
        };
        let text_dois = parse_dois(&head, &tail).map(|d| d.len()).unwrap_or(0);
        let papers = extract_pdf_metadata(&fetcher, path, &Options::default());
        let mut record = json!({
            "pdf": path, "words": head.split_whitespace().count(), "meta_doi": meta, "text_dois": text_dois,
        });
        let papers_found = papers.is_ok();
        match &papers {
            Ok(b) => {
                let (title, doi) = bibtex_fields(b);
                record["papers"] = json!({"found": true, "title": title, "doi": doi});
                found += 1;
            }
            Err(e) => {
                record["papers"] = json!({"found": false, "error": e.message});
                missed += 1;
            }
        }
        let ask = !papers_found || agreed_asked < agree;
        if ask && counter.load(Ordering::SeqCst) < budget {
            if papers_found {
                agreed_asked += 1;
            }
            // the works of the search (as many as OpenAlex finds, to see where the title is)
            let query = query_text(&head, 200).unwrap_or_default();
            let works = if query.is_empty() {
                Err(papers_rs::fetch::FetchError::new(
                    papers_rs::fetch::ErrorKind::Value,
                    "no query",
                ))
            } else {
                fetcher.openalex_search(
                    openalex::truncate(&query, openalex::QUERY_LENGTH),
                    50,
                    &access,
                )
            };
            let mut oa = json!({});
            match &works {
                Ok(works) => {
                    let places: Vec<usize> = works
                        .iter()
                        .enumerate()
                        .filter(|(_, w)| {
                            let title = work_title(w);
                            !title.is_empty() && title_in_text_strict(title, &head)
                        })
                        .map(|(i, _)| i)
                        .collect();
                    oa["works"] = json!(works.len());
                    oa["title_in_text_at"] = json!(places);
                    if !papers_found {
                        first_place.push(places.first().copied());
                    }
                }
                Err(e) => oa["search_error"] = json!(e.message),
            }
            if works.is_ok() {
                // where papers finds the paper, OpenAlex alone is asked (the DOI of the text would be found again)
                let options = Options {
                    search_doi: !papers_found,
                    openalex: Some(access.clone()),
                    ..Options::default()
                };
                match extract_pdf_metadata(&fetcher, path, &options) {
                    Ok(b) => {
                        let (title, doi) = bibtex_fields(&b);
                        oa["found"] = json!(true);
                        oa["title"] = json!(title);
                        oa["doi"] = json!(doi);
                        if !papers_found {
                            gained += 1;
                            if !doi.is_empty() {
                                with_doi += 1;
                            }
                        } else {
                            let same = record["papers"]["doi"]
                                .as_str()
                                .is_some_and(|d| !d.is_empty() && d.eq_ignore_ascii_case(&doi));
                            oa["same_doi_as_papers"] = json!(same);
                            agreed += usize::from(same);
                        }
                    }
                    Err(e) => {
                        oa["found"] = json!(false);
                        oa["error"] = json!(e.message);
                    }
                }
            }
            record["openalex"] = oa;
            if !papers_found {
                let first: String = head
                    .split_whitespace()
                    .take(60)
                    .collect::<Vec<_>>()
                    .join(" ");
                record["head"] = json!(first);
            }
        }
        println!("{record}");
    }
    let place = |limit: usize| {
        first_place
            .iter()
            .filter(|p| p.is_some_and(|p| p < limit))
            .count()
    };
    eprintln!(
        "PDFs in the folder: {total}; sampled: {n} (skipping {skip}); unreadable: {unreadable}"
    );
    eprintln!("papers' chain finds: {found}; does not: {missed}");
    eprintln!(
        "of those not found, asked OpenAlex: {}; OpenAlex finds (title in the text): {gained} ({with_doi} with a DOI, {} without)",
        first_place.len(),
        gained - with_doi
    );
    eprintln!(
        "first work with its title in the text, in the order of OpenAlex: within 5: {}, 10: {}, 20: {}, 50: {}, none: {}",
        place(5),
        place(10),
        place(20),
        place(50),
        first_place.iter().filter(|p| p.is_none()).count()
    );
    eprintln!(
        "of those that papers finds, asked OpenAlex: {agreed_asked}; with the same DOI: {agreed}"
    );
    eprintln!("searches of OpenAlex: {}", counter.load(Ordering::SeqCst));
}
