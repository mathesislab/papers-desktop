//! `rapidfuzz.fuzz.token_set_ratio` (with no processor and no cutoff), which papers uses
//! to choose among the results of a full-text search.

use std::collections::BTreeSet;

use crate::extract::doi::py_split;

/// The Indel distance: the lengths of both, minus twice their longest common subsequence.
fn indel_distance(a: &[char], b: &[char]) -> usize {
    if a.is_empty() || b.is_empty() {
        return a.len() + b.len();
    }
    // the longest common subsequence, one row at a time
    let mut row = vec![0usize; b.len() + 1];
    for &x in a {
        let mut diagonal = 0;
        for (j, &y) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = if x == y {
                diagonal + 1
            } else {
                row[j + 1].max(row[j])
            };
            diagonal = above;
        }
    }
    a.len() + b.len() - 2 * row[b.len()]
}

fn norm_distance(dist: usize, lensum: usize) -> f64 {
    if lensum == 0 {
        100.0
    } else {
        100.0 - 100.0 * dist as f64 / lensum as f64
    }
}

/// How alike two texts are, by their words (0 to 100): 100 when the words of one are among the
/// words of the other.
pub fn token_set_ratio(s1: &str, s2: &str) -> f64 {
    let tokens_a: BTreeSet<&str> = py_split(s1).into_iter().collect();
    let tokens_b: BTreeSet<&str> = py_split(s2).into_iter().collect();
    if tokens_a.is_empty() || tokens_b.is_empty() {
        return 0.0;
    }
    let intersect: Vec<&str> = tokens_a.intersection(&tokens_b).copied().collect();
    let diff_ab: Vec<&str> = tokens_a.difference(&tokens_b).copied().collect();
    let diff_ba: Vec<&str> = tokens_b.difference(&tokens_a).copied().collect();
    // one text is part of the other one
    if !intersect.is_empty() && (diff_ab.is_empty() || diff_ba.is_empty()) {
        return 100.0;
    }
    let ab: Vec<char> = diff_ab.join(" ").chars().collect();
    let ba: Vec<char> = diff_ba.join(" ").chars().collect();
    let sect_len = intersect.join(" ").chars().count();
    // the lengths of sect+ab and sect+ba
    let sep = usize::from(sect_len != 0);
    let sect_ab_len = sect_len + sep + ab.len();
    let sect_ba_len = sect_len + sep + ba.len();
    let result = norm_distance(indel_distance(&ab, &ba), sect_ab_len + sect_ba_len);
    if sect_len == 0 {
        return result;
    }
    // only sect is alike in sect+ab and sect: the distance is the difference of the lengths
    let sect_ab_ratio = norm_distance(sep + ab.len(), sect_len + sect_ab_len);
    let sect_ba_ratio = norm_distance(sep + ba.len(), sect_len + sect_ba_len);
    result.max(sect_ab_ratio).max(sect_ba_ratio)
}
