//! Small helpers of papers' command line.

/// How the full-text search of `--openalex` (papers-rs, not in papers) identifies itself to
/// OpenAlex: the e-mail address of `PAPERS_MAILTO` (the polite pool) and the key of
/// `OPENALEX_API_KEY`, when they are set.
pub fn openalex_access() -> papers_rs::fetch::openalex::OpenAlex {
    let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
    papers_rs::fetch::openalex::OpenAlex {
        mailto: var("PAPERS_MAILTO"),
        api_key: var("OPENALEX_API_KEY"),
    }
}

/// The local path of a `file://` URI, or the argument unchanged otherwise
/// (desktop entries launch viewers with `%U`, i.e. with file:// URIs).
pub fn file_uri_to_path(arg: &str) -> String {
    let Some(rest) = arg.strip_prefix("file://") else { return arg.to_string() };
    // urlparse: authority up to the next `/`, then the path up to `?` or `#`
    let path = match rest.find('/') {
        Some(i) => &rest[i..],
        None => "",
    };
    let path = path.split(['#', '?']).next().unwrap_or("");
    unquote(path)
}

/// `urllib.parse.unquote`: percent-escapes decoded as UTF-8.
pub fn unquote(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Some(byte) = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uris() {
        assert_eq!(file_uri_to_path("file:///abs/my%20files"), "/abs/my files");
        assert_eq!(file_uri_to_path("file://host/abs/x"), "/abs/x");
        assert_eq!(file_uri_to_path("rel/x.pdf"), "rel/x.pdf");
        assert_eq!(file_uri_to_path("file:///a%2"), "/a%2");
        assert_eq!(file_uri_to_path("file:///a%41"), "/aA");
    }
}
