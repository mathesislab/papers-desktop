//! A reference as a paper or a web page writes it, to find in the library:
//! "Robinson, A., Calov, R., and Ganopolski, A.: Multistability and critical
//! thresholds of the Greenland ice sheet, Nat. Clim. Change, 2, 429–432,
//! 2012." Its DOI finds its entry; else the entries most like it are the
//! closest, word by word (see `Library::closest`).

use crate::text::{fold, words};

/// Small words, and the words of links and page numbers, that tell no paper.
const COMMON: &[&str] = &[
    "and", "the", "for", "with", "from", "into", "onto", "over", "under", "its", "their", "this",
    "that", "these", "are", "was", "were", "has", "have", "been", "not", "but", "can", "all",
    "our", "via", "between", "during", "using", "doi", "http", "https", "www", "org", "vol", "edn",
    "eds",
];

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Reference {
    pub doi: Option<String>,
    /// the words that tell a paper, folded: no initials, numbers or small words
    pub words: Vec<String>,
    pub years: Vec<i32>,
}

impl Reference {
    pub fn parse(text: &str) -> Reference {
        let folded = fold(text);
        let mut reference = Reference {
            doi: doi(&folded),
            words: Vec::new(),
            years: Vec::new(),
        };
        for w in words(&folded) {
            match w.parse::<i32>() {
                Ok(year) if w.len() == 4 && (1800..=2100).contains(&year) => {
                    add(&mut reference.years, year)
                }
                // a volume or a page
                Ok(_) => {}
                Err(_) if w.len() >= 3 && !COMMON.contains(&w.as_str()) => {
                    add(&mut reference.words, w)
                }
                Err(_) => {}
            }
        }
        reference
    }
}

fn add<T: PartialEq>(list: &mut Vec<T>, x: T) {
    if !list.contains(&x) {
        list.push(x);
    }
}

/// The first DOI of a folded text: "10.", digits, "/" and what follows up to
/// a space, without the punctuation that ends a sentence.
fn doi(folded: &str) -> Option<String> {
    folded.match_indices("10.").find_map(|(at, _)| {
        let doi = folded[at..].split(char::is_whitespace).next()?;
        let (prefix, suffix) = doi.split_once('/')?;
        let registrant = &prefix[3..];
        let valid = (4..=9).contains(&registrant.len())
            && registrant.bytes().all(|b| b.is_ascii_digit())
            && !suffix.is_empty();
        let doi = doi.trim_end_matches(['.', ',', ';', ':', ')', ']', '>', '"', '\'']);
        valid.then(|| doi.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copernicus() {
        let r = Reference::parse(
            "Robinson, A., Calov, R., and Ganopolski, A.: Multistability and critical \
             thresholds of the Greenland ice sheet, Nat. Clim. Change, 2, 429–432, \
             https://doi.org/10.1038/nclimate1449, 2012.",
        );
        assert_eq!(r.doi.as_deref(), Some("10.1038/nclimate1449"));
        assert_eq!(r.years, [2012]);
        assert_eq!(
            r.words,
            [
                "robinson",
                "calov",
                "ganopolski",
                "multistability",
                "critical",
                "thresholds",
                "greenland",
                "ice",
                "sheet",
                "nat",
                "clim",
                "change",
                "nclimate1449",
            ]
        );
    }

    #[test]
    fn names_and_dois() {
        let r = Reference::parse("van den Broeke, M. et al. (2009) Partitioning … Müller");
        assert_eq!(r.words, ["van", "den", "broeke", "partitioning", "muller"]);
        assert_eq!(r.years, [2009]);
        assert_eq!(r.doi, None);
        let doi = |text| Reference::parse(text).doi;
        assert_eq!(
            doi("(doi:10.5194/tc-11-1913-2017)."),
            Some("10.5194/tc-11-1913-2017".into())
        );
        assert_eq!(doi("10.1002/XYZ.123"), Some("10.1002/xyz.123".into()));
        assert_eq!(doi("version 10.2 / 3"), None);
        assert_eq!(doi("10.123/x"), None);
    }
}
