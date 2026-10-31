//! The free copy of a paper, as OpenAlex knows it: its best open-access
//! location and its others (`works/doi:…`, one request), the published or
//! accepted version first, else a preprint's; arXiv's PDF for an item that
//! has an arXiv id and nothing better. Told, not downloaded: the Add sheet
//! downloads.

use serde::{Deserialize, Serialize};

use crate::http::{Access, Http, doi_path, get_json, with_query};
use crate::model::{Item, ItemKind};

const WORKS: &str = "https://api.openalex.org/works";

/// The fields asked for.
const SELECT: &str = "id,best_oa_location,locations";

/// How free a paper is to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OpenAccess {
    /// a free copy of this version (the published or accepted one, or a preprint's own)
    Open,
    /// a free copy of another version: the preprint of a paper
    Preprint,
    /// none known
    Closed,
    /// OpenAlex does not know the paper, or it has no DOI
    Unknown,
}

/// The free copy of a paper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Pdf {
    pub access: OpenAccess,
    /// its PDF, else its page
    pub url: Option<String>,
    /// `cc-by`, `cc-by-nc`, …
    pub licence: Option<String>,
    /// the host of `url`
    pub host: Option<String>,
}

impl Pdf {
    fn without(access: OpenAccess) -> Pdf {
        Pdf {
            access,
            url: None,
            licence: None,
            host: None,
        }
    }

    fn at(access: OpenAccess, url: String, licence: Option<String>) -> Pdf {
        let host = url::Url::parse(&url).ok().and_then(|u| {
            u.host_str()
                .map(|h| h.trim_start_matches("www.").to_string())
        });
        Pdf {
            access,
            url: Some(url),
            licence,
            host,
        }
    }
}

/// The open-access locations of a work.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Work {
    best_oa_location: Option<Location>,
    locations: Vec<Location>,
}

/// A place where a work can be read.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct Location {
    is_oa: bool,
    pdf_url: Option<String>,
    landing_page_url: Option<String>,
    /// `publishedVersion`, `acceptedVersion`, `submittedVersion`
    version: Option<String>,
    license: Option<String>,
}

/// The URL of a work's locations.
fn work_url(doi: &str, access: &Access) -> String {
    let mut pairs = vec![("select", SELECT)];
    if let Some(key) = &access.openalex_key {
        pairs.push(("api_key", key));
    }
    if let Some(mailto) = &access.mailto {
        pairs.push(("mailto", mailto));
    }
    with_query(&format!("{WORKS}/doi:{}", doi_path(doi)), &pairs)
}

/// The free copy among a work's locations, the best first: a PDF of this
/// version, else its page, else a preprint's (for a preprint, its own).
fn best(work: Work, preprint: bool) -> Pdf {
    let locations: Vec<Location> = work
        .best_oa_location
        .into_iter()
        .chain(work.locations)
        .filter(|l| l.is_oa && (l.pdf_url.is_some() || l.landing_page_url.is_some()))
        .collect();
    let submitted = |l: &Location| l.version.as_deref() == Some("submittedVersion");
    let this = |l: &&Location| preprint || !submitted(l);
    let found = (locations.iter().find(|l| this(l) && l.pdf_url.is_some()))
        .or_else(|| locations.iter().find(this))
        .map(|l| (OpenAccess::Open, l))
        .or_else(|| {
            let other = locations.iter().find(|l| l.pdf_url.is_some());
            other
                .or(locations.first())
                .map(|l| (OpenAccess::Preprint, l))
        });
    match found {
        Some((access, l)) => {
            let url = l.pdf_url.clone().or(l.landing_page_url.clone());
            Pdf::at(
                access,
                url.expect("a location with a URL"),
                l.license.clone(),
            )
        }
        None => Pdf::without(OpenAccess::Closed),
    }
}

/// The free copy of an item: by its DOI in OpenAlex; arXiv's PDF if it has
/// an arXiv id and OpenAlex gives nothing better. An error for no answer.
pub fn pdf(item: &Item, http: &dyn Http, access: &Access) -> Result<Pdf, String> {
    let preprint = item.kind == ItemKind::Preprint;
    let arxiv = item.arxiv.as_ref().map(|id| {
        let url = format!("https://arxiv.org/pdf/{id}");
        let access = if preprint {
            OpenAccess::Open
        } else {
            OpenAccess::Preprint
        };
        Pdf::at(access, url, None)
    });
    let Some(doi) = &item.doi else {
        return Ok(arxiv.unwrap_or(Pdf::without(OpenAccess::Unknown)));
    };
    let found = match get_json::<Work>(http, &work_url(doi, access))? {
        Some(work) => best(work, preprint),
        None => Pdf::without(OpenAccess::Unknown),
    };
    Ok(match (found.access, arxiv) {
        (OpenAccess::Open, _) | (_, None) => found,
        (_, Some(arxiv)) => arxiv,
    })
}
