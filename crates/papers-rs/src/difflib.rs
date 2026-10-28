//! Python's `difflib.ndiff` (3.14), which `papers check` shows the changes of an entry with
//! (`papers.bib.entry_diff`): `SequenceMatcher` (with the junk of `ndiff`, and the popular
//! elements of its `autojunk`) and the `Differ` that puts the lines marked `?` under the
//! changed ones. The same algorithm, element by element, so that the same text comes out.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;

use crate::bib::pystr::is_space;

/// What to do with a block of the first sequence to get the second.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tag {
    Replace,
    Delete,
    Insert,
    Equal,
}

/// `(tag, i1, i2, j1, j2)`: `a[i1..i2]` becomes `b[j1..j2]`.
pub type Opcode = (Tag, usize, usize, usize, usize);

/// `difflib.SequenceMatcher`: the matching blocks of two sequences, by the longest
/// contiguous match without junk, then the same on both sides of it.
pub struct SequenceMatcher<T> {
    isjunk: Option<fn(&T) -> bool>,
    a: Vec<T>,
    b: Vec<T>,
    b2j: HashMap<T, Vec<usize>>,
    bjunk: HashSet<T>,
    matching_blocks: Option<Vec<(usize, usize, usize)>>,
    opcodes: Option<Vec<Opcode>>,
    fullbcount: Option<HashMap<T, usize>>,
}

fn calculate_ratio(matches: usize, length: usize) -> f64 {
    if length > 0 {
        2.0 * matches as f64 / length as f64
    } else {
        1.0
    }
}

impl<T: Clone + Eq + Hash> SequenceMatcher<T> {
    /// A matcher of two sequences; `isjunk` tells the elements that are not worth matching on
    /// (`SequenceMatcher(isjunk, a, b)`, with `autojunk` on).
    pub fn new(isjunk: Option<fn(&T) -> bool>, a: Vec<T>, b: Vec<T>) -> SequenceMatcher<T> {
        let mut matcher = SequenceMatcher {
            isjunk,
            a,
            b: Vec::new(),
            b2j: HashMap::new(),
            bjunk: HashSet::new(),
            matching_blocks: None,
            opcodes: None,
            fullbcount: None,
        };
        matcher.set_seq2(b);
        matcher
    }

    pub fn set_seqs(&mut self, a: Vec<T>, b: Vec<T>) {
        self.set_seq1(a);
        self.set_seq2(b);
    }

    pub fn set_seq1(&mut self, a: Vec<T>) {
        self.a = a;
        self.matching_blocks = None;
        self.opcodes = None;
    }

    pub fn set_seq2(&mut self, b: Vec<T>) {
        self.b = b;
        self.matching_blocks = None;
        self.opcodes = None;
        self.fullbcount = None;
        self.chain_b();
    }

    fn chain_b(&mut self) {
        let mut b2j: HashMap<T, Vec<usize>> = HashMap::new();
        for (i, elt) in self.b.iter().enumerate() {
            b2j.entry(elt.clone()).or_default().push(i);
        }
        // purge the junk elements
        let mut junk: HashSet<T> = HashSet::new();
        if let Some(isjunk) = self.isjunk {
            for elt in b2j.keys() {
                if isjunk(elt) {
                    junk.insert(elt.clone());
                }
            }
            for elt in &junk {
                b2j.remove(elt);
            }
        }
        // purge the popular elements that are not junk
        let n = self.b.len();
        if n >= 200 {
            let ntest = n / 100 + 1;
            let popular: Vec<T> = b2j
                .iter()
                .filter(|(_, idxs)| idxs.len() > ntest)
                .map(|(elt, _)| elt.clone())
                .collect();
            for elt in &popular {
                b2j.remove(elt);
            }
        }
        self.b2j = b2j;
        self.bjunk = junk;
    }

    fn is_bjunk(&self, j: usize) -> bool {
        self.bjunk.contains(&self.b[j])
    }

    /// The longest matching block of `a[alo..ahi]` and `b[blo..bhi]`: `(i, j, size)`; of the
    /// longest, the one that starts first in `a`, then in `b`.
    pub fn find_longest_match(
        &self,
        alo: usize,
        ahi: usize,
        blo: usize,
        bhi: usize,
    ) -> (usize, usize, usize) {
        let (mut besti, mut bestj, mut bestsize) = (alo, blo, 0usize);
        // j2len[j]: the length of the longest junk-free match ending with a[i-1] and b[j]
        let mut j2len: HashMap<usize, usize> = HashMap::new();
        for i in alo..ahi {
            let mut newj2len: HashMap<usize, usize> = HashMap::new();
            if let Some(js) = self.b2j.get(&self.a[i]) {
                for &j in js {
                    if j < blo {
                        continue;
                    }
                    if j >= bhi {
                        break;
                    }
                    let k = if j > 0 {
                        j2len.get(&(j - 1)).copied().unwrap_or(0)
                    } else {
                        0
                    } + 1;
                    newj2len.insert(j, k);
                    if k > bestsize {
                        besti = i + 1 - k;
                        bestj = j + 1 - k;
                        bestsize = k;
                    }
                }
            }
            j2len = newj2len;
        }
        // extend the best by the elements that are not junk, on each end
        while besti > alo
            && bestj > blo
            && !self.is_bjunk(bestj - 1)
            && self.a[besti - 1] == self.b[bestj - 1]
        {
            besti -= 1;
            bestj -= 1;
            bestsize += 1;
        }
        while besti + bestsize < ahi
            && bestj + bestsize < bhi
            && !self.is_bjunk(bestj + bestsize)
            && self.a[besti + bestsize] == self.b[bestj + bestsize]
        {
            bestsize += 1;
        }
        // and by the junk that matches
        while besti > alo
            && bestj > blo
            && self.is_bjunk(bestj - 1)
            && self.a[besti - 1] == self.b[bestj - 1]
        {
            besti -= 1;
            bestj -= 1;
            bestsize += 1;
        }
        while besti + bestsize < ahi
            && bestj + bestsize < bhi
            && self.is_bjunk(bestj + bestsize)
            && self.a[besti + bestsize] == self.b[bestj + bestsize]
        {
            bestsize += 1;
        }
        (besti, bestj, bestsize)
    }

    /// The blocks `(i, j, n)` with `a[i..i+n] == b[j..j+n]`, in order, adjacent ones joined, and
    /// the sentinel `(len(a), len(b), 0)` last.
    pub fn get_matching_blocks(&mut self) -> Vec<(usize, usize, usize)> {
        if let Some(blocks) = &self.matching_blocks {
            return blocks.clone();
        }
        let (la, lb) = (self.a.len(), self.b.len());
        let mut queue = vec![(0, la, 0, lb)];
        let mut matching: Vec<(usize, usize, usize)> = Vec::new();
        while let Some((alo, ahi, blo, bhi)) = queue.pop() {
            let (i, j, k) = self.find_longest_match(alo, ahi, blo, bhi);
            if k > 0 {
                matching.push((i, j, k));
                if alo < i && blo < j {
                    queue.push((alo, i, blo, j));
                }
                if i + k < ahi && j + k < bhi {
                    queue.push((i + k, ahi, j + k, bhi));
                }
            }
        }
        matching.sort_unstable();
        let (mut i1, mut j1, mut k1) = (0, 0, 0);
        let mut non_adjacent: Vec<(usize, usize, usize)> = Vec::new();
        for (i2, j2, k2) in matching {
            if i1 + k1 == i2 && j1 + k1 == j2 {
                k1 += k2;
            } else {
                if k1 > 0 {
                    non_adjacent.push((i1, j1, k1));
                }
                (i1, j1, k1) = (i2, j2, k2);
            }
        }
        if k1 > 0 {
            non_adjacent.push((i1, j1, k1));
        }
        non_adjacent.push((la, lb, 0));
        self.matching_blocks = Some(non_adjacent.clone());
        non_adjacent
    }

    /// How to turn `a` into `b`.
    pub fn get_opcodes(&mut self) -> Vec<Opcode> {
        if let Some(codes) = &self.opcodes {
            return codes.clone();
        }
        let (mut i, mut j) = (0, 0);
        let mut answer = Vec::new();
        for (ai, bj, size) in self.get_matching_blocks() {
            let tag = if i < ai && j < bj {
                Some(Tag::Replace)
            } else if i < ai {
                Some(Tag::Delete)
            } else if j < bj {
                Some(Tag::Insert)
            } else {
                None
            };
            if let Some(tag) = tag {
                answer.push((tag, i, ai, j, bj));
            }
            (i, j) = (ai + size, bj + size);
            if size > 0 {
                answer.push((Tag::Equal, ai, i, bj, j));
            }
        }
        self.opcodes = Some(answer.clone());
        answer
    }

    pub fn ratio(&mut self) -> f64 {
        let matches: usize = self.get_matching_blocks().iter().map(|t| t.2).sum();
        calculate_ratio(matches, self.a.len() + self.b.len())
    }

    /// An upper bound of [`SequenceMatcher::ratio`]: the elements in common, whatever their order.
    pub fn quick_ratio(&mut self) -> f64 {
        if self.fullbcount.is_none() {
            let mut full: HashMap<T, usize> = HashMap::new();
            for elt in &self.b {
                *full.entry(elt.clone()).or_default() += 1;
            }
            self.fullbcount = Some(full);
        }
        let full = self.fullbcount.as_ref().expect("counted");
        let mut avail: HashMap<&T, i64> = HashMap::new();
        let mut matches = 0usize;
        for elt in &self.a {
            let numb = match avail.get(elt) {
                Some(n) => *n,
                None => full.get(elt).copied().unwrap_or(0) as i64,
            };
            avail.insert(elt, numb - 1);
            if numb > 0 {
                matches += 1;
            }
        }
        calculate_ratio(matches, self.a.len() + self.b.len())
    }

    /// An upper bound of [`SequenceMatcher::quick_ratio`]: the length of the shorter.
    pub fn real_quick_ratio(&self) -> f64 {
        let (la, lb) = (self.a.len(), self.b.len());
        calculate_ratio(la.min(lb), la + lb)
    }
}

/// `IS_CHARACTER_JUNK`: a space or a tab.
fn is_character_junk(c: &char) -> bool {
    matches!(c, ' ' | '\t')
}

/// `str.splitlines(True)`: the lines with their ends, cut at the line boundaries of Python.
pub fn splitlines_keepends(s: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = s.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if matches!(
            c,
            '\n' | '\r' | '\u{0b}' | '\u{0c}' | '\u{1c}'
                ..='\u{1e}' | '\u{85}' | '\u{2028}' | '\u{2029}'
        ) {
            let mut end = i + c.len_utf8();
            if c == '\r' && matches!(chars.peek(), Some((_, '\n'))) {
                chars.next();
                end += 1;
            }
            lines.push(s[start..end].to_string());
            start = end;
        }
    }
    if start < s.len() {
        lines.push(s[start..].to_string());
    }
    lines
}

fn dump(out: &mut Vec<String>, tag: char, x: &[String], lo: usize, hi: usize) {
    for line in &x[lo..hi] {
        out.push(format!("{tag} {line}"));
    }
}

fn plain_replace(
    out: &mut Vec<String>,
    a: &[String],
    alo: usize,
    ahi: usize,
    b: &[String],
    blo: usize,
    bhi: usize,
) {
    // the shorter block first
    if bhi - blo < ahi - alo {
        dump(out, '+', b, blo, bhi);
        dump(out, '-', a, alo, ahi);
    } else {
        dump(out, '-', a, alo, ahi);
        dump(out, '+', b, blo, bhi);
    }
}

fn fancy_helper(
    out: &mut Vec<String>,
    a: &[String],
    alo: usize,
    ahi: usize,
    b: &[String],
    blo: usize,
    bhi: usize,
) {
    if alo < ahi {
        if blo < bhi {
            plain_replace(out, a, alo, ahi, b, blo, bhi);
        } else {
            dump(out, '-', a, alo, ahi);
        }
    } else if blo < bhi {
        dump(out, '+', b, blo, bhi);
    }
}

/// `_keep_original_ws`: the tags, but the white space of the line where it has no mark.
fn keep_original_ws(line: &str, tags: &str) -> String {
    line.chars()
        .zip(tags.chars())
        .map(|(c, t)| if t == ' ' && is_space(c) { c } else { t })
        .collect()
}

fn rstrip(s: &str) -> &str {
    s.trim_end_matches(is_space)
}

fn qformat(out: &mut Vec<String>, aline: &str, bline: &str, atags: &str, btags: &str) {
    let atags = keep_original_ws(aline, atags);
    let atags = rstrip(&atags);
    let btags = keep_original_ws(bline, btags);
    let btags = rstrip(&btags);
    out.push(format!("- {aline}"));
    if !atags.is_empty() {
        out.push(format!("? {atags}\n"));
    }
    out.push(format!("+ {bline}"));
    if !btags.is_empty() {
        out.push(format!("? {btags}\n"));
    }
}

fn chars(s: &str) -> Vec<char> {
    s.chars().collect()
}

/// `Differ._fancy_replace`: for a block of lines replaced by another, the similar lines are
/// paired and marked within the line, the others are dumped.
fn fancy_replace(
    out: &mut Vec<String>,
    a: &[String],
    alo: usize,
    ahi: usize,
    b: &[String],
    blo: usize,
    bhi: usize,
) {
    const CUTOFF: f64 = 0.74999;
    const WINDOW: usize = 10;
    let mut cruncher: SequenceMatcher<char> =
        SequenceMatcher::new(Some(is_character_junk), Vec::new(), Vec::new());
    let mut best: Option<(usize, usize)> = None;
    let (mut dump_i, mut dump_j) = (alo, blo);
    for j in blo..bhi {
        cruncher.set_seq2(chars(&b[j]));
        let aequiv = alo + (j - blo);
        let start = aequiv.saturating_sub(WINDOW).max(dump_i);
        let end = (aequiv + WINDOW + 1).min(ahi);
        if start >= end {
            break;
        }
        let mut best_ratio = CUTOFF;
        for (i, line) in a.iter().enumerate().take(end).skip(start) {
            cruncher.set_seq1(chars(line));
            if cruncher.real_quick_ratio() > best_ratio
                && cruncher.quick_ratio() > best_ratio
                && cruncher.ratio() > best_ratio
            {
                best = Some((i, j));
                best_ratio = cruncher.ratio();
            }
        }
        let Some((best_i, best_j)) = best.take() else {
            continue;
        };
        fancy_helper(out, a, dump_i, best_i, b, dump_j, best_j);
        let (aelt, belt) = (&a[best_i], &b[best_j]);
        if aelt != belt {
            let (mut atags, mut btags) = (String::new(), String::new());
            cruncher.set_seqs(chars(aelt), chars(belt));
            for (tag, ai1, ai2, bj1, bj2) in cruncher.get_opcodes() {
                let (la, lb) = (ai2 - ai1, bj2 - bj1);
                match tag {
                    Tag::Replace => {
                        atags.push_str(&"^".repeat(la));
                        btags.push_str(&"^".repeat(lb));
                    }
                    Tag::Delete => atags.push_str(&"-".repeat(la)),
                    Tag::Insert => btags.push_str(&"+".repeat(lb)),
                    Tag::Equal => {
                        atags.push_str(&" ".repeat(la));
                        btags.push_str(&" ".repeat(lb));
                    }
                }
            }
            qformat(out, aelt, belt, &atags, &btags);
        } else {
            out.push(format!("  {aelt}"));
        }
        (dump_i, dump_j) = (best_i + 1, best_j + 1);
    }
    fancy_helper(out, a, dump_i, ahi, b, dump_j, bhi);
}

/// `difflib.ndiff(a, b)`: the lines of the delta (each starting with `- `, `+ `, `  ` or `? `),
/// the lines of `a` and `b` with their ends.
pub fn ndiff(a: &[String], b: &[String]) -> Vec<String> {
    let mut cruncher = SequenceMatcher::new(None, a.to_vec(), b.to_vec());
    let mut out = Vec::new();
    for (tag, alo, ahi, blo, bhi) in cruncher.get_opcodes() {
        match tag {
            Tag::Replace => fancy_replace(&mut out, a, alo, ahi, b, blo, bhi),
            Tag::Delete => dump(&mut out, '-', a, alo, ahi),
            Tag::Insert => dump(&mut out, '+', b, blo, bhi),
            Tag::Equal => dump(&mut out, ' ', a, alo, ahi),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        splitlines_keepends(text)
    }

    #[test]
    fn the_example_of_the_documentation() {
        let a = lines("one\ntwo\nthree\n");
        let b = lines("ore\ntree\nemu\n");
        let delta: String = ndiff(&a, &b).concat();
        assert_eq!(
            delta,
            "- one\n?  ^\n+ ore\n?  ^\n- two\n- three\n?  -\n+ tree\n+ emu\n"
        );
    }

    #[test]
    fn similar_lines_are_marked_when_they_are_alike_enough() {
        let a = lines(
            "  1. Beautiful is better than ugly.\n  2. Explicit is better than implicit.\n  3. Simple is better than complex.\n  4. Complex is better than complicated.\n",
        );
        let b = lines(
            "  1. Beautiful is better than ugly.\n  3.   Simple is better than complex.\n  4. Complicated is better than complex.\n  5. Flat is better than nested.\n",
        );
        let delta: String = ndiff(&a, &b).concat();
        assert_eq!(
            delta,
            "    1. Beautiful is better than ugly.\n-   2. Explicit is better than implicit.\n-   3. Simple is better than complex.\n+   3.   Simple is better than complex.\n?     ++\n-   4. Complex is better than complicated.\n+   4. Complicated is better than complex.\n+   5. Flat is better than nested.\n"
        );
    }

    #[test]
    fn splitting_keeps_the_ends() {
        assert_eq!(
            lines("a\r\nb\rc\u{2028}d"),
            ["a\r\n", "b\r", "c\u{2028}", "d"]
        );
        assert!(lines("").is_empty());
    }

    #[test]
    fn matching_blocks() {
        let mut s = SequenceMatcher::new(None, "abxcd".chars().collect(), "abcd".chars().collect());
        assert_eq!(s.get_matching_blocks(), [(0, 0, 2), (3, 2, 2), (5, 4, 0)]);
        let s = SequenceMatcher::new(
            Some(is_character_junk),
            " abcd".chars().collect(),
            "abcd abcd".chars().collect(),
        );
        assert_eq!(s.find_longest_match(0, 5, 0, 9), (1, 0, 4));
    }
}
