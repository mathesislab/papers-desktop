//! A post's HTML as the details show it: what reads (text, links, images,
//! figures, tables) kept, what acts (scripts, styles, forms, frames, event
//! attributes) dropped, relative addresses made whole by the post's.

use ammonia::{Builder, UrlRelative};
use url::Url;

/// The HTML of a post, safe to show; its links and images relative to `base`
/// (the post's address) made whole, those without a base dropped.
pub fn safe_html(html: &str, base: Option<&str>) -> String {
    let mut builder = Builder::default();
    builder.link_rel(Some("noopener noreferrer"));
    builder.url_relative(match base.and_then(|b| Url::parse(b).ok()) {
        Some(base) => UrlRelative::RewriteWithBase(base),
        None => UrlRelative::Deny,
    });
    builder.clean(html).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_acts_dropped_images_kept_whole() {
        let html = "<p onclick=\"x()\">A <b>comic</b><script>steal()</script></p>\
            <img src=\"/comics/planets.png\" title=\"The joke\" alt=\"Planets\">\
            <iframe src=\"https://example.org\"></iframe><style>p{}</style>";
        let safe = safe_html(html, Some("https://xkcd.com/3000/"));
        assert_eq!(
            safe,
            "<p>A <b>comic</b></p><img src=\"https://xkcd.com/comics/planets.png\" title=\"The joke\" alt=\"Planets\">"
        );
        assert!(!safe_html("<img src=\"/a.png\">", None).contains("a.png"));
    }
}
