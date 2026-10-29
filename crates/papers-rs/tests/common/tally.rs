//! What a differential test counts: the cases compared with what papers gave.

#![allow(dead_code)]

#[derive(Default)]
pub struct Tally {
    pub cases: usize,
}

impl Tally {
    /// Check one case: `got` must be `expected` (`text` is what the case was made of).
    pub fn check<T: PartialEq + std::fmt::Debug>(
        &mut self,
        what: &str,
        text: &str,
        got: T,
        expected: T,
    ) {
        self.cases += 1;
        if got != expected {
            panic!("{what} of {text:?}: got {got:?}, papers gave {expected:?}");
        }
    }

    pub fn report(&self, name: &str) {
        eprintln!("{name}: {} cases", self.cases);
    }
}
