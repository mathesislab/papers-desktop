//! The score of an item, 0–100: what the rows show, the sort by score and
//! `score:` take. It is of two parts, each 0–100: its keywords, the best of
//! its interests' scores scaled (the interest's `min_score` is 50, twice
//! that 100), and its library, how like the library it is scaled (as like as
//! the library's typical paper is 100: core's `Like::scaled`). The settings
//! say how the parts make the score ([`ScoreMode`]): the higher of both, or
//! one alone. An exclude of any interest vetoes the item: its score and its
//! parts are 0.

use papers_core::Like;
use serde::{Deserialize, Serialize};

use crate::interest::Score;
use crate::model::words;

/// How the parts of an item make its score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScoreMode {
    /// the higher of both: without interests, the library; without the
    /// index of the library, the keywords
    #[default]
    Higher,
    Keywords,
    Library,
}

words!(ScoreMode {
    Higher => "higher",
    Keywords => "keywords",
    Library => "library",
});

/// The score of an item and its parts.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Rating {
    /// the parts as the mode takes them, 0–100; 0 when vetoed, or without
    /// the part the mode takes
    pub score: f64,
    /// the best of its interests' scaled scores; none without interests
    pub keywords: Option<f64>,
    /// how like the library it is, scaled; none without the index of the
    /// library
    pub library: Option<f64>,
    /// the exclude that vetoed it, of the first interest whose exclude
    /// matched: its term or pattern
    pub excluded: Option<String>,
}

impl Rating {
    /// The score of an item of these scores in the interests and this
    /// likeness to the library.
    pub(crate) fn of(scores: &[Score], like: Option<&Like>, mode: ScoreMode) -> Rating {
        let excluded = scores.iter().find_map(|s| s.excluded.clone());
        let keywords = (scores.iter()).map(|s| s.scaled).reduce(f64::max);
        let library = like.map(|l| l.scaled);
        let (keywords, library) = match excluded {
            Some(_) => (keywords.map(|_| 0.0), library.map(|_| 0.0)),
            None => (keywords, library),
        };
        let score = match mode {
            ScoreMode::Higher => keywords.into_iter().chain(library).reduce(f64::max),
            ScoreMode::Keywords => keywords,
            ScoreMode::Library => library,
        };
        Rating {
            score: score.unwrap_or(0.0),
            keywords,
            library,
            excluded,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn score(interest: &str, scaled: f64, excluded: Option<&str>) -> Score {
        Score {
            interest: interest.to_string(),
            score: scaled / 10.0,
            scaled,
            terms: Vec::new(),
            excluded: excluded.map(str::to_string),
            reached: excluded.is_none() && scaled >= 50.0,
        }
    }

    fn like(scaled: f64) -> Like {
        Like {
            score: scaled / 3.0,
            scaled,
            nearest: Vec::new(),
        }
    }

    #[test]
    fn the_keywords_are_the_best_interest() {
        let scores = [score("AMOC", 30.0, None), score("Firn", 72.0, None)];
        let rating = Rating::of(&scores, None, ScoreMode::Higher);
        assert_eq!(rating.keywords, Some(72.0));
        assert_eq!((rating.score, rating.library), (72.0, None));
    }

    #[test]
    fn the_mode_takes_the_higher_part_or_one() {
        let scores = [score("AMOC", 72.0, None)];
        let of = |mode| Rating::of(&scores, Some(&like(41.0)), mode).score;
        assert_eq!(of(ScoreMode::Higher), 72.0);
        assert_eq!(of(ScoreMode::Keywords), 72.0);
        assert_eq!(of(ScoreMode::Library), 41.0);
        let rating = Rating::of(
            &[score("AMOC", 10.0, None)],
            Some(&like(41.0)),
            ScoreMode::Higher,
        );
        assert_eq!(rating.score, 41.0);
        // without interests, the library; without the index, the keywords
        let none = Rating::of(&[], Some(&like(41.0)), ScoreMode::Higher);
        assert_eq!((none.score, none.keywords), (41.0, None));
        assert_eq!(
            Rating::of(&[], Some(&like(41.0)), ScoreMode::Keywords).score,
            0.0
        );
        assert_eq!(Rating::of(&scores, None, ScoreMode::Library).score, 0.0);
        assert_eq!(Rating::of(&[], None, ScoreMode::Higher).score, 0.0);
    }

    #[test]
    fn an_exclude_of_any_interest_vetoes_the_whole_score() {
        let scores = [
            score("AMOC", 90.0, None),
            score("Firn", 0.0, Some("Georgia")),
        ];
        for mode in [ScoreMode::Higher, ScoreMode::Keywords, ScoreMode::Library] {
            let rating = Rating::of(&scores, Some(&like(80.0)), mode);
            assert_eq!(
                rating,
                Rating {
                    score: 0.0,
                    keywords: Some(0.0),
                    library: Some(0.0),
                    excluded: Some("Georgia".to_string()),
                }
            );
        }
        let rating = Rating::of(&scores, None, ScoreMode::Higher);
        assert_eq!((rating.score, rating.library), (0.0, None));
    }

    #[test]
    fn the_mode_in_its_words() {
        assert_eq!(ScoreMode::default(), ScoreMode::Higher);
        let json = serde_json::to_string(&ScoreMode::Keywords).unwrap();
        assert_eq!(json, "\"keywords\"");
        assert_eq!("library".parse(), Ok(ScoreMode::Library));
    }
}
