//! Where the transliteration of papers (Unidecode) and that of papers-rs (the `deunicode`
//! crate) differ, as `tests/gen_library_expected.py` recorded it (`expected/library/translit.json`).
//!
//! The differential tests compare papers-rs with what papers gave. When they differ and the text
//! has a character of this list (or one that its NFKD or LaTeX decoding has), the difference is a
//! known one of the transliteration, not a failure: [`Translit::explains`].

#![allow(dead_code)]

use std::collections::HashSet;
use std::sync::OnceLock;

use papers_rs::bib::latex_table::latex_to_unicode;
use unicode_normalization::UnicodeNormalization;

use super::{dir, json_file};

pub struct Translit {
    below: HashSet<u32>,
    ranges: Vec<(u32, u32)>,
    pub count: usize,
}

pub fn translit() -> &'static Translit {
    static T: OnceLock<Translit> = OnceLock::new();
    T.get_or_init(|| {
        let j = json_file(&dir("expected").join("library").join("translit.json"));
        Translit {
            below: j["differences_below_3000"]
                .as_array()
                .unwrap()
                .iter()
                .map(|d| d[0].as_u64().unwrap() as u32)
                .collect(),
            ranges: j["difference_ranges_from_3000"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| (r[0].as_u64().unwrap() as u32, r[1].as_u64().unwrap() as u32))
                .collect(),
            count: j["difference_count"].as_u64().unwrap() as usize,
        }
    })
}

impl Translit {
    /// The two tables differ for this character.
    pub fn differs(&self, c: char) -> bool {
        let cp = c as u32;
        if cp < 0x80 {
            return false;
        }
        if cp < 0x3000 {
            return self.below.contains(&cp);
        }
        self.ranges.iter().any(|&(a, b)| a <= cp && cp <= b)
    }

    /// The text has a character that the tables do not agree on, or its NFKD or its LaTeX
    /// decoding has one.
    pub fn explains(&self, text: &str) -> bool {
        text.chars().any(|c| self.differs(c))
            || text.nfkd().any(|c| self.differs(c))
            || latex_to_unicode(text).chars().any(|c| self.differs(c))
    }
}

/// What a differential test counts: the cases, and those whose difference is a known one of the
/// transliteration.
#[derive(Default)]
pub struct Tally {
    pub cases: usize,
    pub known: usize,
}

impl Tally {
    /// Check one case: `got` must be `expected`, unless `text` explains the difference.
    pub fn check<T: PartialEq + std::fmt::Debug>(
        &mut self,
        what: &str,
        text: &str,
        got: T,
        expected: T,
    ) {
        self.cases += 1;
        if got != expected {
            if translit().explains(text) {
                self.known += 1;
            } else {
                panic!("{what} of {text:?}: got {got:?}, papers gave {expected:?}");
            }
        }
    }

    pub fn report(&self, name: &str) {
        eprintln!(
            "{name}: {} cases, {} known differences of the transliteration",
            self.cases, self.known
        );
    }
}
