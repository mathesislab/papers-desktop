//! The subscriptions offered to start with, for a researcher of the climate
//! and the Earth's surface, in groups. Each feed answered a browser's GET
//! with a feed (October 2026); the journals whose feeds refuse programs
//! (Cambridge's) are Crossref sweeps by ISSN. PNAS's feed refused (403), and
//! ESSOAr has neither a feed nor an ISSN, so they are not offered.

use serde::Serialize;

use crate::journals::journal_key;
use crate::model::SourceKind::{Blog, Journal, News, Preprints};
use crate::model::{Feed, SourceKind};
use crate::normalize::canonical_issn;

/// A subscription offered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Suggested {
    pub title: String,
    pub kind: SourceKind,
    pub feed: Feed,
    /// its group in the list: "Copernicus", "Nature", "AGU", "News", ...
    pub group: String,
}

/// What a suggestion follows.
enum Of {
    Rss(&'static str),
    Copernicus(&'static str),
    Nature(&'static str),
    /// a feed of Wiley's AGU journals, by the electronic ISSN without its hyphen
    Wiley(&'static str),
    Issn(&'static str),
}

/// The list: group, title, kind, feed.
#[rustfmt::skip]
const LIST: &[(&str, &str, SourceKind, Of)] = &[
    ("Copernicus", "The Cryosphere", Journal, Of::Copernicus("tc")),
    ("Copernicus", "Climate of the Past", Journal, Of::Copernicus("cp")),
    ("Copernicus", "Earth System Dynamics", Journal, Of::Copernicus("esd")),
    ("Copernicus", "Geoscientific Model Development", Journal, Of::Copernicus("gmd")),
    ("Copernicus", "Biogeosciences", Journal, Of::Copernicus("bg")),
    ("Copernicus", "Atmospheric Chemistry and Physics", Journal, Of::Copernicus("acp")),
    ("Copernicus", "Ocean Science", Journal, Of::Copernicus("os")),
    ("Copernicus", "Natural Hazards and Earth System Sciences", Journal, Of::Copernicus("nhess")),
    ("Copernicus", "Earth System Science Data", Journal, Of::Copernicus("essd")),
    ("Copernicus", "EGUsphere", Preprints, Of::Copernicus("egusphere")),
    ("Nature", "Nature", Journal, Of::Nature("nature")),
    ("Nature", "Nature Geoscience", Journal, Of::Nature("ngeo")),
    ("Nature", "Nature Climate Change", Journal, Of::Nature("nclimate")),
    ("Nature", "Nature Communications", Journal, Of::Nature("ncomms")),
    ("Science", "Science", Journal, Of::Rss("https://www.science.org/action/showFeed?type=etoc&feed=rss&jc=science")),
    ("Science", "Science Advances", Journal, Of::Rss("https://www.science.org/action/showFeed?type=etoc&feed=rss&jc=sciadv")),
    ("AGU", "Geophysical Research Letters", Journal, Of::Wiley("19448007")),
    ("AGU", "JGR: Oceans", Journal, Of::Wiley("21699291")),
    ("AGU", "JGR: Atmospheres", Journal, Of::Wiley("21698996")),
    ("AGU", "JGR: Earth Surface", Journal, Of::Wiley("21699011")),
    ("AGU", "AGU Advances", Journal, Of::Wiley("2576604x")),
    ("AGU", "Earth's Future", Journal, Of::Wiley("23284277")),
    ("AGU", "Reviews of Geophysics", Journal, Of::Wiley("19449208")),
    ("Glaciology", "Journal of Glaciology", Journal, Of::Issn("0022-1430")),
    ("Glaciology", "Annals of Glaciology", Journal, Of::Issn("0260-3055")),
    ("AMS", "Journal of Climate", Journal, Of::Rss("https://journals.ametsoc.org/journalissuetocrss/journals/clim/clim-overview.xml")),
    ("Preprints", "arXiv: Atmospheric and Oceanic Physics", Preprints, Of::Rss("https://rss.arxiv.org/atom/physics.ao-ph")),
    ("Preprints", "arXiv: Geophysics", Preprints, Of::Rss("https://rss.arxiv.org/atom/physics.geo-ph")),
    ("News", "Futurity", News, Of::Rss("https://www.futurity.org/feed/")),
    ("News", "ScienceDaily: Climate", News, Of::Rss("https://www.sciencedaily.com/rss/earth_climate/climate.xml")),
    ("News", "ScienceDaily: Earth & Climate", News, Of::Rss("https://www.sciencedaily.com/rss/earth_climate.xml")),
    ("News", "Eos", News, Of::Rss("https://eos.org/feed")),
    ("News", "Carbon Brief", News, Of::Rss("https://www.carbonbrief.org/feed/")),
    ("Blogs", "RealClimate", Blog, Of::Rss("https://www.realclimate.org/index.php/feed/")),
];

impl Of {
    fn feed(&self) -> Feed {
        let rss = |url: String| Feed::Rss { url };
        match self {
            Of::Rss(url) => rss(url.to_string()),
            Of::Copernicus(j) => rss(format!("https://{j}.copernicus.org/xml/rss2_0.xml")),
            Of::Nature(j) => rss(format!("https://www.nature.com/{j}.rss")),
            Of::Wiley(issn) => rss(format!(
                "https://agupubs.onlinelibrary.wiley.com/feed/{issn}/most-recent"
            )),
            Of::Issn(issn) => Feed::Crossref {
                issn: issn.to_string(),
            },
        }
    }

    /// The ISSN it shows: a sweep's, the electronic one of Wiley's feeds.
    fn issn(&self) -> Option<String> {
        match self {
            Of::Issn(issn) | Of::Wiley(issn) => canonical_issn(issn),
            _ => None,
        }
    }
}

/// The suggestion of a journal: the same title (folded as [`journal_key`]),
/// or one of its ISSNs (canonical).
pub fn suggested_journal(title: &str, issns: &[String]) -> Option<Suggested> {
    let key = journal_key(title);
    let found = LIST.iter().find(|(_, title, _, of)| {
        journal_key(title) == key || of.issn().is_some_and(|issn| issns.contains(&issn))
    })?;
    let (group, title, kind, of) = found;
    Some(suggestion(group, title, *kind, of))
}

/// A suggestion of the list.
fn suggestion(group: &str, title: &str, kind: SourceKind, of: &Of) -> Suggested {
    Suggested {
        title: title.to_string(),
        kind,
        feed: of.feed(),
        group: group.to_string(),
    }
}

/// The subscriptions offered, in their groups.
pub fn suggested() -> Vec<Suggested> {
    LIST.iter()
        .map(|(group, title, kind, of)| suggestion(group, title, *kind, of))
        .collect()
}
