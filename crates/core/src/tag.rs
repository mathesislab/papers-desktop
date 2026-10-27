//! Tags, and the tags under a tag: `climate/amoc` is under `climate`.

use crate::text::fold;

/// Whether two tags are the same, as the tags are counted: "AMOC" and "amoc".
pub fn same_tag(a: &str, b: &str) -> bool {
    fold(a) == fold(b)
}

/// The parents of a tag, the shortest first: `climate` and `climate/amoc`
/// for `climate/amoc/tipping`.
pub(crate) fn parents(tag: &str) -> impl Iterator<Item = &str> {
    tag.match_indices('/')
        .map(|(at, _)| &tag[..at])
        .filter(|parent| !parent.is_empty())
}

/// A tag and its parents, the shortest first: the names a search for a tag
/// takes it by, and the tags that count it.
pub(crate) fn branch(tag: &str) -> impl Iterator<Item = &str> {
    parents(tag).chain(std::iter::once(tag))
}

/// What is left of `tag` below `parent`, if it is under it: `amoc/tipping`
/// for `climate/amoc/tipping` under `climate`. `parent` is compared as tags
/// are counted, the rest is as written.
pub fn under<'a>(tag: &'a str, parent: &str) -> Option<&'a str> {
    let at = parents(tag).find(|p| same_tag(p, parent))?.len();
    Some(&tag[at + 1..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parents_of_a_tag() {
        let of = |tag| parents(tag).collect::<Vec<_>>();
        assert_eq!(of("climate/amoc/tipping"), ["climate", "climate/amoc"]);
        assert_eq!(of("climate/amoc"), ["climate"]);
        assert!(of("climate").is_empty());
        // no parent without a name
        assert_eq!(of("/amoc/x"), ["/amoc"]);
        let branch: Vec<_> = branch("climate/amoc").collect();
        assert_eq!(branch, ["climate", "climate/amoc"]);
    }

    #[test]
    fn under_a_parent() {
        assert_eq!(
            under("Climate/AMOC/tipping", "climate"),
            Some("AMOC/tipping")
        );
        assert_eq!(
            under("climate/amoc/tipping", "Climate/AMOC"),
            Some("tipping")
        );
        // not the tag itself, a longer name, or another tag
        assert_eq!(under("climate", "climate"), None);
        assert_eq!(under("climate-review/amoc", "climate"), None);
        assert_eq!(under("climate/amoc", "climate/amoc/tipping"), None);
        // folding changes the length of a name ("Æ" is "ae")
        assert_eq!(under("Æx/y", "aex"), Some("y"));
    }
}
