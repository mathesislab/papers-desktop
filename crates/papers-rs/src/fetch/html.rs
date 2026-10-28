//! The links to a `.bib` file of an HTML page (`find_bibtex_links`: BeautifulSoup with
//! `html.parser`, the `<a>` elements with an `href`). A scan of the tags as `html.parser`
//! reads them: comments and the text of `<script>` and `<style>` are skipped, the names of
//! tags and attributes are in lower case, the values of the attributes are unescaped, the
//! last of a repeated attribute wins.
//!
//! An extension of papers-rs (papers has no such function): the `<meta>` tags that journals put
//! in their article pages for Google Scholar and the readers of citations ([`meta_content`]),
//! and the DOI of the article they give ([`page_doi`]).

fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c')
}

/// `html.unescape` of the common references.
fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i + 1..];
        let end = tail.find(';').filter(|&e| e <= 10);
        let decoded = end.and_then(|e| {
            let name = &tail[..e];
            let c = match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ => name.strip_prefix('#').and_then(|n| {
                    let code = match n.strip_prefix(['x', 'X']) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok(),
                        None => n.parse().ok(),
                    };
                    code.and_then(char::from_u32)
                }),
            };
            c.map(|c| (c, e))
        });
        match decoded {
            Some((c, e)) => {
                out.push(c);
                rest = &tail[e + 1..];
            }
            None => {
                out.push('&');
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The attributes of a start tag whose name ends at `i` in `t`, and the index after the tag;
/// none if the text ends inside the tag (then it is text, not a tag).
fn attributes(t: &[char], mut i: usize) -> Option<(Vec<(String, String)>, usize)> {
    let mut attrs: Vec<(String, String)> = Vec::new();
    loop {
        while i < t.len() && (is_space(t[i]) || t[i] == '/') {
            i += 1;
        }
        if i >= t.len() {
            return None;
        }
        if t[i] == '>' {
            return Some((attrs, i + 1));
        }
        let start = i;
        while i < t.len() && !is_space(t[i]) && !matches!(t[i], '=' | '>' | '/') {
            i += 1;
        }
        if i == start {
            i += 1; // a stray character
            continue;
        }
        let name: String = t[start..i].iter().collect::<String>().to_lowercase();
        let mut j = i;
        while j < t.len() && is_space(t[j]) {
            j += 1;
        }
        let mut value = String::new();
        if t.get(j) == Some(&'=') {
            j += 1;
            while j < t.len() && is_space(t[j]) {
                j += 1;
            }
            match t.get(j) {
                Some(&q) if q == '"' || q == '\'' => {
                    let from = j + 1;
                    let to = t[from..]
                        .iter()
                        .position(|&c| c == q)
                        .map_or(t.len(), |p| from + p);
                    value = unescape(&t[from..to].iter().collect::<String>());
                    j = (to + 1).min(t.len());
                }
                _ => {
                    let from = j;
                    while j < t.len() && !is_space(t[j]) && t[j] != '>' {
                        j += 1;
                    }
                    value = unescape(&t[from..j].iter().collect::<String>());
                }
            }
            i = j;
        }
        match attrs.iter_mut().find(|(n, _)| *n == name) {
            Some(a) => a.1 = value,
            None => attrs.push((name, value)),
        }
    }
}

/// The `href` of the `<a>` elements of the page, in order.
pub fn anchor_links(html: &str) -> Vec<String> {
    start_tags(html)
        .into_iter()
        .filter(|(name, _)| name == "a")
        .filter_map(|(_, attrs)| attrs.into_iter().find(|(n, _)| n == "href").map(|(_, v)| v))
        .collect()
}

/// The `content` of the `<meta>` elements of the page that have the attribute `name` (or
/// `property`, as Open Graph and RDFa write it) as given, in lower case, in order: what the
/// pages of journals say of the article for the readers of citations (`citation_doi`,
/// `citation_pdf_url`, `dc.identifier`, `prism.doi`).
pub fn meta_content(html: &str, name: &str) -> Vec<String> {
    let name = name.to_lowercase();
    start_tags(html)
        .into_iter()
        .filter(|(tag, _)| tag == "meta")
        .filter(|(_, attrs)| {
            attrs
                .iter()
                .any(|(n, v)| (n == "name" || n == "property") && v.trim().to_lowercase() == name)
        })
        .filter_map(|(_, attrs)| {
            attrs
                .into_iter()
                .find(|(n, _)| n == "content")
                .map(|(_, v)| v)
        })
        .collect()
}

/// The DOI of the article that a page of a journal is: the first valid one of the tags
/// `citation_doi`, `prism.doi`, `dc.identifier.doi` and `dc.identifier` (Dublin Core, which
/// often says `doi:10.…`), in that order; in its case, without `doi:` or `doi.org/`.
pub fn page_doi(html: &str) -> Option<String> {
    [
        "citation_doi",
        "prism.doi",
        "dc.identifier.doi",
        "dc.identifier",
    ]
    .iter()
    .flat_map(|name| meta_content(html, name))
    .find_map(|content| crate::identity::strip_doi(&content))
}

/// The start tags of the page, in order, with their attributes: the scan of `html.parser`, with
/// the comments and the text of `<script>` and `<style>` skipped.
fn start_tags(html: &str) -> Vec<(String, Vec<(String, String)>)> {
    let t: Vec<char> = html.chars().collect();
    let mut tags = Vec::new();
    let mut i = 0;
    while i < t.len() {
        if t[i] != '<' {
            i += 1;
            continue;
        }
        let rest: String = t[i..(i + 4).min(t.len())].iter().collect();
        if rest == "<!--" {
            // a comment
            let tail: String = t[i + 4..].iter().collect();
            i = match tail.find("-->") {
                Some(p) => i + 4 + tail[..p].chars().count() + 3,
                None => t.len(),
            };
            continue;
        }
        let Some(&first) = t.get(i + 1) else { break };
        if first == '/' || first == '!' || first == '?' {
            // an end tag, a declaration: up to the next `>`
            i = t[i..]
                .iter()
                .position(|&c| c == '>')
                .map_or(t.len(), |p| i + p + 1);
            continue;
        }
        if !first.is_ascii_alphabetic() {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < t.len() && !is_space(t[j]) && !matches!(t[j], '>' | '/') {
            j += 1;
        }
        let name: String = t[i + 1..j].iter().collect::<String>().to_lowercase();
        let Some((attrs, next)) = attributes(&t, j) else {
            // the text ends inside the tag
            i += 1;
            continue;
        };
        i = next;
        let script = name == "script" || name == "style";
        tags.push((name.clone(), attrs));
        if script {
            // the text up to the end tag is not markup
            let tail: String = t[i..].iter().collect::<String>().to_lowercase();
            i = match tail.find(&format!("</{name}")) {
                Some(p) => i + tail[..p].chars().count(),
                None => t.len(),
            };
        }
    }
    tags
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = r#"<!-- <meta name="citation_doi" content="10.0/comment"> -->
<html><head>
<META NAME="Citation_DOI" content="10.5194/tc-11-1647-2017">
<meta property="og:title" content="A &amp; B">
<meta name="dc.identifier" content="doi:10.5194/tc-11-1647-2017"/>
<meta name="citation_pdf_url" content="https://tc.copernicus.org/articles/11/1647/2017/tc-11-1647-2017.pdf">
<script>var s = '<meta name="citation_doi" content="10.0/script">';</script>
<a href="/x.bib">bib</a></head></html>"#;

    #[test]
    fn meta_tags_by_name_or_property() {
        assert_eq!(
            meta_content(PAGE, "citation_doi"),
            ["10.5194/tc-11-1647-2017"]
        );
        assert_eq!(
            meta_content(PAGE, "DC.identifier"),
            ["doi:10.5194/tc-11-1647-2017"]
        );
        assert_eq!(meta_content(PAGE, "og:title"), ["A & B"]);
        assert!(meta_content(PAGE, "prism.doi").is_empty());
        assert_eq!(anchor_links(PAGE), ["/x.bib"]);
    }

    #[test]
    fn the_doi_of_a_page() {
        assert_eq!(page_doi(PAGE).as_deref(), Some("10.5194/tc-11-1647-2017"));
        let dublin = r#"<meta name="DC.Identifier" content="https://doi.org/10.1000/Abc">"#;
        assert_eq!(page_doi(dublin).as_deref(), Some("10.1000/Abc"));
        // an identifier that is not a DOI (an ISSN, a handle) is not one
        let other = r#"<meta name="dc.identifier" content="ISSN 1234-5678"><meta name="prism.doi" content="x">"#;
        assert_eq!(page_doi(other), None);
        assert_eq!(page_doi("<html></html>"), None);
    }
}
