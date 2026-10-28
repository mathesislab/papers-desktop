//! `difflib.ndiff` of Python 3.14: the line diff of two texts with the `?` lines that mark what
//! changed inside a line. A port of `SequenceMatcher` (with the junk of the characters and
//! `autojunk`) and of `Differ`, so that the lines are the same as papers prints.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;

/// `SequenceMatcher(isjunk, a, b)`: the matching blocks of two sequences.
struct Matcher<'a, T> {
    a: &'a [T],
    b: &'a [T],
    b2j: HashMap<&'a T, Vec<usize>>,
    bjunk: HashSet<&'a T>,
}

impl<'a, T: Eq + Hash> Matcher<'a, T> {
    fn new(isjunk: Option<fn(&T) -> bool>, a: &'a [T], b: &'a [T]) -> Matcher<'a, T> {
        let mut b2j: HashMap<&T, Vec<usize>> = HashMap::new();
        for (i, elt) in b.iter().enumerate() {
            b2j.entry(elt).or_default().push(i);
        }
        let mut bjunk = HashSet::new();
        if let Some(isjunk) = isjunk {
            for elt in b2j.keys().copied().collect::<Vec<_>>() {
                if isjunk(elt) {
                    bjunk.insert(elt);
                    b2j.remove(elt);
                }
            }
        }
        // autojunk: the elements that are more than 1% of a long sequence
        if b.len() >= 200 {
            let ntest = b.len() / 100 + 1;
            b2j.retain(|_, idxs| idxs.len() <= ntest);
        }
        Matcher { a, b, b2j, bjunk }
    }

    fn longest_match(
        &self,
        alo: usize,
        ahi: usize,
        blo: usize,
        bhi: usize,
    ) -> (usize, usize, usize) {
        let (a, b) = (self.a, self.b);
        let (mut besti, mut bestj, mut bestsize) = (alo, blo, 0usize);
        let mut j2len: HashMap<usize, usize> = HashMap::new();
        for (i, item) in a.iter().enumerate().take(ahi).skip(alo) {
            let mut newj2len: HashMap<usize, usize> = HashMap::new();
            if let Some(js) = self.b2j.get(item) {
                for &j in js {
                    if j < blo {
                        continue;
                    }
                    if j >= bhi {
                        break;
                    }
                    let k = j2len.get(&j.wrapping_sub(1)).copied().unwrap_or(0) + 1;
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
        let junk = |x: &T| self.bjunk.contains(x);
        while besti > alo && bestj > blo && !junk(&b[bestj - 1]) && a[besti - 1] == b[bestj - 1] {
            besti -= 1;
            bestj -= 1;
            bestsize += 1;
        }
        while besti + bestsize < ahi
            && bestj + bestsize < bhi
            && !junk(&b[bestj + bestsize])
            && a[besti + bestsize] == b[bestj + bestsize]
        {
            bestsize += 1;
        }
        while besti > alo && bestj > blo && junk(&b[bestj - 1]) && a[besti - 1] == b[bestj - 1] {
            besti -= 1;
            bestj -= 1;
            bestsize += 1;
        }
        while besti + bestsize < ahi
            && bestj + bestsize < bhi
            && junk(&b[bestj + bestsize])
            && a[besti + bestsize] == b[bestj + bestsize]
        {
            bestsize += 1;
        }
        (besti, bestj, bestsize)
    }

    fn matching_blocks(&self) -> Vec<(usize, usize, usize)> {
        let (la, lb) = (self.a.len(), self.b.len());
        let mut queue = vec![(0, la, 0, lb)];
        let mut blocks: Vec<(usize, usize, usize)> = Vec::new();
        while let Some((alo, ahi, blo, bhi)) = queue.pop() {
            let (i, j, k) = self.longest_match(alo, ahi, blo, bhi);
            if k > 0 {
                blocks.push((i, j, k));
                if alo < i && blo < j {
                    queue.push((alo, i, blo, j));
                }
                if i + k < ahi && j + k < bhi {
                    queue.push((i + k, ahi, j + k, bhi));
                }
            }
        }
        blocks.sort();
        let mut merged: Vec<(usize, usize, usize)> = Vec::new();
        let (mut i1, mut j1, mut k1) = (0, 0, 0);
        for (i2, j2, k2) in blocks {
            if i1 + k1 == i2 && j1 + k1 == j2 {
                k1 += k2;
            } else {
                if k1 > 0 {
                    merged.push((i1, j1, k1));
                }
                (i1, j1, k1) = (i2, j2, k2);
            }
        }
        if k1 > 0 {
            merged.push((i1, j1, k1));
        }
        merged.push((la, lb, 0));
        merged
    }

    fn opcodes(&self) -> Vec<(Tag, usize, usize, usize, usize)> {
        let (mut i, mut j) = (0, 0);
        let mut codes = Vec::new();
        for (ai, bj, size) in self.matching_blocks() {
            let tag = match (i < ai, j < bj) {
                (true, true) => Some(Tag::Replace),
                (true, false) => Some(Tag::Delete),
                (false, true) => Some(Tag::Insert),
                (false, false) => None,
            };
            if let Some(tag) = tag {
                codes.push((tag, i, ai, j, bj));
            }
            (i, j) = (ai + size, bj + size);
            if size > 0 {
                codes.push((Tag::Equal, ai, i, bj, j));
            }
        }
        codes
    }

    fn ratio(&self) -> f64 {
        let length = self.a.len() + self.b.len();
        if length == 0 {
            return 1.0;
        }
        let matches: usize = self.matching_blocks().iter().map(|m| m.2).sum();
        2.0 * matches as f64 / length as f64
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tag {
    Replace,
    Delete,
    Insert,
    Equal,
}

/// `IS_CHARACTER_JUNK`: a space or a tab.
fn is_character_junk(c: &char) -> bool {
    matches!(c, ' ' | '\t')
}

fn dump(out: &mut Vec<String>, tag: char, lines: &[String], lo: usize, hi: usize) {
    for line in &lines[lo..hi] {
        out.push(format!("{tag} {line}"));
    }
}

fn plain_replace(
    out: &mut Vec<String>,
    a: &[String],
    (alo, ahi): (usize, usize),
    b: &[String],
    (blo, bhi): (usize, usize),
) {
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
    (alo, ahi): (usize, usize),
    b: &[String],
    (blo, bhi): (usize, usize),
) {
    if alo < ahi {
        if blo < bhi {
            plain_replace(out, a, (alo, ahi), b, (blo, bhi));
        } else {
            dump(out, '-', a, alo, ahi);
        }
    } else if blo < bhi {
        dump(out, '+', b, blo, bhi);
    }
}

/// The tags of a line, where the spaces and tabs of the line stay as they are (`_keep_original_ws`).
fn keep_original_ws(line: &str, tags: &str) -> String {
    line.chars()
        .zip(tags.chars())
        .map(|(c, t)| if t == ' ' && c.is_whitespace() { c } else { t })
        .collect()
}

fn qformat(out: &mut Vec<String>, aline: &str, bline: &str, atags: &str, btags: &str) {
    let atags = keep_original_ws(aline, atags);
    let btags = keep_original_ws(bline, btags);
    let (atags, btags) = (atags.trim_end(), btags.trim_end());
    out.push(format!("- {aline}"));
    if !atags.is_empty() {
        out.push(format!("? {atags}\n"));
    }
    out.push(format!("+ {bline}"));
    if !btags.is_empty() {
        out.push(format!("? {btags}\n"));
    }
}

fn fancy_replace(
    out: &mut Vec<String>,
    a: &[String],
    (alo, ahi): (usize, usize),
    b: &[String],
    (blo, bhi): (usize, usize),
) {
    const CUTOFF: f64 = 0.74999;
    const WINDOW: usize = 10;
    let chars = |s: &str| s.chars().collect::<Vec<char>>();
    let mut best: Option<(usize, usize)> = None;
    let (mut dump_i, mut dump_j) = (alo, blo);
    for j in blo..bhi {
        let bj = chars(&b[j]);
        let aequiv = alo + (j - blo);
        let lo = aequiv.saturating_sub(WINDOW).max(dump_i);
        let hi = (aequiv + WINDOW + 1).min(ahi);
        if lo >= hi {
            break;
        }
        let mut best_ratio = CUTOFF;
        for (i, line) in a.iter().enumerate().take(hi).skip(lo) {
            let ai = chars(line);
            let ratio = Matcher::new(Some(is_character_junk), &ai, &bj).ratio();
            if ratio > best_ratio {
                best = Some((i, j));
                best_ratio = ratio;
            }
        }
        let Some((best_i, best_j)) = best else {
            continue;
        };
        fancy_helper(out, a, (dump_i, best_i), b, (dump_j, best_j));
        let (aelt, belt) = (&a[best_i], &b[best_j]);
        if aelt != belt {
            let (ac, bc) = (chars(aelt), chars(belt));
            let matcher = Matcher::new(Some(is_character_junk), &ac, &bc);
            let (mut atags, mut btags) = (String::new(), String::new());
            for (tag, ai1, ai2, bj1, bj2) in matcher.opcodes() {
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
        best = None;
    }
    fancy_helper(out, a, (dump_i, ahi), b, (dump_j, bhi));
}

/// `''.join(difflib.ndiff(a.splitlines(True), b.splitlines(True)))`: the lines of the diff.
pub fn ndiff(a: &str, b: &str) -> String {
    let (a, b) = (split_keep_ends(a), split_keep_ends(b));
    let matcher = Matcher::new(None, &a, &b);
    let mut out: Vec<String> = Vec::new();
    for (tag, alo, ahi, blo, bhi) in matcher.opcodes() {
        match tag {
            Tag::Replace => fancy_replace(&mut out, &a, (alo, ahi), &b, (blo, bhi)),
            Tag::Delete => dump(&mut out, '-', &a, alo, ahi),
            Tag::Insert => dump(&mut out, '+', &b, blo, bhi),
            Tag::Equal => dump(&mut out, ' ', &a, alo, ahi),
        }
    }
    out.concat()
}

/// `str.splitlines(True)`, for the line ends that bibtex text has (`\n`, `\r\n`, `\r`).
fn split_keep_ends(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        line.push(c);
        let end = match c {
            '\n' => true,
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    line.push(chars.next().expect("peeked"));
                }
                true
            }
            _ => false,
        };
        if end {
            lines.push(std::mem::take(&mut line));
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_what_changed_in_a_line() {
        let diff = ndiff("abcDefghiJkl\n", "abcdefGhijkl\n");
        assert_eq!(
            diff,
            "- abcDefghiJkl\n?    ^  ^  ^\n+ abcdefGhijkl\n?    ^  ^  ^\n"
        );
    }

    #[test]
    fn plain_lines() {
        assert_eq!(ndiff("a\nb\n", "a\nc\n"), "  a\n- b\n+ c\n");
        assert_eq!(ndiff("a\n", "a\nb\n"), "  a\n+ b\n");
    }
}
