//! When the entries came into a library, as its history tells: not part of papers, which has no
//! use for it; for a program that lists the papers added last (the app's *Recently added*).

use std::collections::HashMap;

use super::project::State;
use super::recorder::History;
use crate::bib::Bib;

impl History {
    /// When the entries of the library came into it: for each key that the states of the line of
    /// the head have had, the time of the oldest of these states whose bibtex has it. The keys of
    /// the oldest state of the line are left out: they were there before it (the library before
    /// its first recorded command, or before the oldest state the history keeps), from a time it
    /// does not tell. Empty without a head.
    ///
    /// The states are walked once; a piece of text is read once, for the oldest state that has
    /// it (the states share their pieces: a command changes one or two), and a piece that cannot
    /// be read is skipped.
    pub fn entry_times(&self) -> HashMap<String, f64> {
        let Some(head) = self.head() else {
            return HashMap::new();
        };
        let line = self.project.line(&head.state);
        let states: Vec<&State> = line
            .iter()
            .rev()
            .filter_map(|n| self.project.state(n))
            .collect();
        // the oldest state of each piece, by its place on the line (0: the oldest)
        let mut first: HashMap<String, usize> = HashMap::new();
        for (i, state) in states.iter().enumerate() {
            for piece in state.bibtex_pieces() {
                first.entry(piece).or_insert(i);
            }
        }
        let mut oldest: HashMap<String, usize> = HashMap::new();
        for (piece, i) in first {
            let Ok(text) = self.store.text(std::slice::from_ref(&piece)) else {
                continue;
            };
            for entry in Bib::parse(&text).entries() {
                oldest
                    .entry(entry.key.clone())
                    .and_modify(|j| *j = (*j).min(i))
                    .or_insert(i);
            }
        }
        oldest
            .into_iter()
            .filter(|&(_, i)| i > 0)
            .map(|(key, i)| (key, states[i].time()))
            .collect()
    }
}
