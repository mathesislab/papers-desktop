//! The extension of papers-rs, `--openalex` (where papers has `--scholar`; not in papers), as the
//! tests that compare with papers see it: the usage and the help of `add`, `fetch` and `extract`
//! have the option, and nothing else differs.

#![allow(dead_code)]

/// The words of a text, as one line (the wrapping is argparse's).
pub fn words(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The words of a usage or help of papers-rs, without the option `--openalex` (its place in the
/// usage, and its entry in the help); `None` if the text has none.
pub fn without_openalex(text: &str) -> Option<String> {
    let one_line = words(text);
    if !one_line.contains("[--openalex]") && !one_line.contains(" --openalex ") {
        return None;
    }
    let one_line = one_line.replace(" [--openalex]", "");
    let Some(start) = one_line.find(" --openalex (papers-rs) use OpenAlex instead of ") else {
        return Some(one_line); // a usage: no entry
    };
    let last = "(environment: PAPERS_MAILTO, OPENALEX_API_KEY)";
    let end = start + one_line[start..].find(last)? + last.len();
    Some(format!("{}{}", &one_line[..start], &one_line[end..]))
}

/// The text papers printed, or the same with the option of papers-rs.
pub fn is_as_papers(got: &str, papers: &str) -> bool {
    got == papers || without_openalex(got).is_some_and(|got| got == words(papers))
}
