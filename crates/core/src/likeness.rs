//! How like the library a text is: a TF-IDF index of the library's papers
//! (title, abstract, keywords) and the cosine of a text with each of them.
//!
//! The words are the folded words of the search, without the stopwords,
//! the numbers and the words of fewer than three letters; a plural's `s`
//! goes (`sheets` is `sheet`). A word weighs `(1 + ln tf) · idf`, a word of
//! the title or a keyword counting [`TITLE`] times; the vectors are of
//! length 1. An inverted index (each word's papers and weights) adds up the
//! cosines of the papers that share a word with the text, so that scoring
//! thousands of items against thousands of papers is quick.

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;

use crate::Library;
use crate::fields::field;
use crate::text::{fold, words};

/// How many times a word of a title or a keyword counts.
pub const TITLE: f32 = 2.0;
/// The papers whose cosines make the score.
pub const TOP: usize = 5;
/// The papers listed as nearest.
pub const NEAREST: usize = 3;

/// The words that say nothing of what a paper is about.
const STOPWORDS: &[&str] = &[
    "about", "above", "across", "after", "again", "against", "all", "also", "among", "and", "any",
    "are", "around", "based", "been", "before", "being", "below", "between", "both", "but", "can",
    "could", "did", "does", "doing", "down", "due", "during", "each", "either", "few", "for",
    "from", "further", "had", "has", "have", "having", "here", "how", "however", "into", "its",
    "itself", "may", "might", "more", "most", "much", "must", "new", "nor", "not", "now", "off",
    "once", "one", "only", "other", "our", "ours", "out", "over", "own", "same", "several",
    "should", "show", "shown", "shows", "since", "some", "such", "than", "that", "the", "their",
    "them", "then", "there", "these", "they", "this", "those", "through", "thus", "too", "two",
    "under", "until", "upon", "use", "used", "using", "very", "via", "was", "well", "were", "what",
    "when", "where", "whether", "which", "while", "who", "whom", "why", "will", "with", "within",
    "without", "would", "yet", "you", "your", "study", "paper", "results", "result", "find",
    "found", "first", "suggest", "suggests", "present", "presents",
];

/// A paper of the library, as the index takes it.
#[derive(Debug, Clone, Default)]
pub struct Doc {
    pub id: String,
    pub title: String,
    pub r#abstract: String,
    pub keywords: Vec<String>,
}

/// How like the library a text is.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Like {
    /// the mean of the best [`TOP`] cosines, times 100 (0–100)
    pub score: f64,
    /// the ids of the [`NEAREST`] papers and their cosines, the nearest first
    pub nearest: Vec<(String, f64)>,
}

/// The TF-IDF index of the library's papers.
#[derive(Debug, Default)]
pub struct Likeness {
    ids: Vec<String>,
    /// the number of each word
    vocabulary: HashMap<String, u32>,
    /// by word
    idf: Vec<f32>,
    /// by word: the papers that have it, and its weight in their vector
    postings: Vec<Vec<(u32, f32)>>,
    /// the idf of a word no paper has
    unknown: f32,
}

impl Likeness {
    /// The index of these papers.
    pub fn build(docs: impl IntoIterator<Item = Doc>) -> Likeness {
        let mut index = Likeness::default();
        let mut counts: Vec<Vec<(u32, f32)>> = Vec::new();
        for doc in docs {
            let mut tf: BTreeMap<u32, f32> = BTreeMap::new();
            let keywords = doc.keywords.join(" ");
            for (text, weight) in [
                (&doc.title, TITLE),
                (&keywords, TITLE),
                (&doc.r#abstract, 1.0),
            ] {
                for word in terms(text) {
                    let next = index.vocabulary.len() as u32;
                    let id = *index.vocabulary.entry(word).or_insert(next);
                    *tf.entry(id).or_default() += weight;
                }
            }
            index.ids.push(doc.id);
            counts.push(tf.into_iter().collect());
        }
        let mut df = vec![0u32; index.vocabulary.len()];
        counts
            .iter()
            .flatten()
            .for_each(|&(w, _)| df[w as usize] += 1);
        let n = index.ids.len() as f32;
        // smoothed, as scikit-learn: a word of every paper still counts a little
        let idf = |df: u32| ((1.0 + n) / (1.0 + df as f32)).ln() + 1.0;
        index.idf = df.into_iter().map(idf).collect();
        index.unknown = idf(0);
        index.postings = vec![Vec::new(); index.vocabulary.len()];
        for (d, tf) in counts.into_iter().enumerate() {
            let vector: Vec<(u32, f32)> = (tf.into_iter())
                .map(|(w, tf)| (w, (1.0 + tf.ln()) * index.idf[w as usize]))
                .collect();
            let norm = vector.iter().map(|(_, x)| x * x).sum::<f32>().sqrt();
            for (w, x) in vector {
                index.postings[w as usize].push((d as u32, x / norm));
            }
        }
        index
    }

    /// The index of a library's papers.
    pub fn of(library: &Library) -> Likeness {
        Likeness::build(library.items().iter().map(|item| {
            let entry = library.entry(&item.id);
            let r#abstract = entry.and_then(|e| field(&e.entry, &["abstract"]));
            Doc {
                id: item.id.clone(),
                title: item.title.clone(),
                r#abstract: r#abstract.unwrap_or_default(),
                keywords: item.keywords.clone(),
            }
        }))
    }

    /// The number of papers.
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// How like the library a title and an abstract are: 0 without a word
    /// or a paper.
    pub fn like(&self, title: &str, r#abstract: &str) -> Like {
        // in order: the same text sums to the same score
        let mut tf: BTreeMap<&str, f32> = BTreeMap::new();
        let (title, r#abstract) = (terms(title), terms(r#abstract));
        let weighted =
            (title.iter().map(|w| (w, TITLE))).chain(r#abstract.iter().map(|w| (w, 1.0)));
        for (word, weight) in weighted {
            *tf.entry(word.as_str()).or_default() += weight;
        }
        // the words no paper has count in the length of the text
        let vector: Vec<(Option<u32>, f32)> = (tf.into_iter())
            .map(|(word, tf)| {
                let id = self.vocabulary.get(word).copied();
                let idf = id.map_or(self.unknown, |w| self.idf[w as usize]);
                (id, (1.0 + tf.ln()) * idf)
            })
            .collect();
        let norm = vector.iter().map(|(_, x)| x * x).sum::<f32>().sqrt();
        // a cosine per paper, those of the papers that share no word left at 0
        let mut dot = vec![0f32; self.ids.len()];
        for (w, x) in vector.into_iter().filter_map(|(w, x)| Some((w?, x))) {
            for &(d, y) in &self.postings[w as usize] {
                dot[d as usize] += x * y;
            }
        }
        let shared = dot.into_iter().enumerate().filter(|&(_, x)| x > 0.0);
        let mut cosines: Vec<(u32, f32)> = shared.map(|(d, x)| (d as u32, x / norm)).collect();
        let top = TOP.min(self.ids.len());
        if cosines.len() > top {
            cosines.select_nth_unstable_by(top, |a, b| b.1.total_cmp(&a.1));
            cosines.truncate(top);
        }
        cosines.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        // from 0, not the -0 of an empty sum
        let sum = (cosines.iter()).fold(0.0, |sum, &(_, c)| sum + f64::from(c));
        Like {
            score: if top == 0 {
                0.0
            } else {
                100.0 * sum / top as f64
            },
            nearest: (cosines.iter().take(NEAREST))
                .map(|&(d, c)| (self.ids[d as usize].clone(), f64::from(c)))
                .collect(),
        }
    }
}

/// The words of a text the index counts.
fn terms(text: &str) -> Vec<String> {
    let words = words(&fold(text)).into_iter();
    words
        .filter(|w| {
            w.len() >= 3
                && !w.bytes().all(|b| b.is_ascii_digit())
                && !STOPWORDS.contains(&w.as_str())
        })
        .map(stem)
        .collect()
}

/// A plural without its `s`: `sheets` is `sheet`; not `mass`, `status`, `analysis`.
fn stem(word: String) -> String {
    let plural = word.len() > 4
        && word.ends_with('s')
        && !["ss", "us", "is"].iter().any(|e| word.ends_with(e));
    match plural {
        true => word[..word.len() - 1].to_string(),
        false => word,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(id: &str, title: &str, r#abstract: &str) -> Doc {
        Doc {
            id: id.to_string(),
            title: title.to_string(),
            r#abstract: r#abstract.to_string(),
            keywords: Vec::new(),
        }
    }

    fn library() -> Likeness {
        Likeness::build([
            doc(
                "ice",
                "Greenland ice sheet melt",
                "Surface melt of the Greenland ice sheet under warming.",
            ),
            doc(
                "amoc",
                "AMOC weakening",
                "The Atlantic meridional overturning circulation weakens.",
            ),
            doc(
                "sea",
                "Sea level rise from Antarctica",
                "Ice sheet loss raises the sea level.",
            ),
            doc(
                "bird",
                "Migration of songbirds",
                "Songbirds migrate south in the autumn.",
            ),
        ])
    }

    #[test]
    fn terms_drop_stopwords_numbers_and_short_words() {
        assert_eq!(
            terms("The Ice-Sheets of 2012 in Müller's GIA analysis"),
            ["ice", "sheet", "muller", "gia", "analysis"]
        );
    }

    #[test]
    fn nearest_are_the_papers_of_the_same_words() {
        let index = library();
        let like = index.like("Melt of the Greenland ice sheet", "");
        let ids: Vec<&str> = like.nearest.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids, ["ice", "sea"]);
        assert!(like.nearest[0].1 > like.nearest[1].1);
        assert!(like.nearest[0].1 <= 1.0 + 1e-6);
    }

    #[test]
    fn score_is_the_mean_of_the_best_cosines() {
        let index = library();
        let like = index.like(
            "Greenland ice sheet melt",
            "Surface melt of the Greenland ice sheet under warming.",
        );
        // the same text as a paper: a cosine of 1, the mean over the four papers
        let (id, cosine) = &like.nearest[0];
        assert_eq!(id, "ice");
        assert!((cosine - 1.0).abs() < 1e-5);
        let sum: f64 = like.nearest.iter().map(|(_, c)| c).sum();
        assert!((like.score - 100.0 * sum / 4.0).abs() < 1e-3);
        let unlike = index.like("Quantum chromodynamics on the lattice", "");
        assert_eq!(
            unlike,
            Like {
                score: 0.0,
                nearest: Vec::new()
            }
        );
        assert!(index.like("Songbird migration", "").score > 10.0);
    }

    #[test]
    fn words_no_paper_has_lower_the_score() {
        let index = library();
        let plain = index.like("AMOC weakening", "").score;
        let diluted = index
            .like("AMOC weakening", "zebrafish genome telomere")
            .score;
        assert!(diluted < plain);
    }

    #[test]
    fn empty_index_or_text_scores_zero() {
        let empty = Likeness::build([]);
        assert!(empty.is_empty());
        assert_eq!(empty.like("Greenland ice sheet", "").score, 0.0);
        assert_eq!(library().like("", "").score, 0.0);
    }

    #[test]
    fn of_a_library_reads_the_abstracts() {
        let dir = tempfile::tempdir().unwrap();
        let bib = dir.path().join("lib.bib");
        let text = "@article{a, title = {Ocean heat}, abstract = {Antarctic shelf warming}}\n\
                    @article{b, title = {Songbirds}, keywords = {migration, autumn}}\n";
        std::fs::write(&bib, text).unwrap();
        let index = Likeness::of(&Library::load(&[bib]).unwrap());
        assert_eq!(index.len(), 2);
        assert_eq!(index.like("Antarctic warming", "").nearest[0].0, "a");
        assert_eq!(index.like("Autumn migration", "").nearest[0].0, "b");
    }
}
