//! The subscriptions as OPML, the list feed readers exchange: an outline of
//! each feed (`type="rss"`, `xmlUrl`), its title (`text`, `title`) and its
//! kind (`category`: journal, preprints, news, blog). A sweep is an outline of
//! its own type (`crossref` and `issn`, `openalex` and `filter`), which other
//! readers pass over. Read back: the outlines of feeds and sweeps at any
//! depth, the kind from `category`, else from the name of a group around it
//! ("News", "Blogs", "Preprints"), else a journal.

use quick_xml::escape::escape;
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};

use crate::feeds::Feeds;
use crate::model::{Feed, SourceKind, Subscription};
use crate::{Error, Result};

/// A subscription read from OPML: what it follows, its title, its kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outline {
    pub feed: Feed,
    pub title: Option<String>,
    pub kind: SourceKind,
}

/// The name of a kind in `category`.
fn kind_name(kind: SourceKind) -> &'static str {
    match kind {
        SourceKind::Journal => "journal",
        SourceKind::Preprints => "preprints",
        SourceKind::News => "news",
        SourceKind::Blog => "blog",
    }
}

/// The kind a category or a group's name says, if it says one.
fn kind_of(text: &str) -> Option<SourceKind> {
    let text = text.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| text.contains(w));
    if has(&["preprint"]) {
        Some(SourceKind::Preprints)
    } else if has(&["news", "press"]) {
        Some(SourceKind::News)
    } else if has(&["blog"]) {
        Some(SourceKind::Blog)
    } else if has(&["journal"]) {
        Some(SourceKind::Journal)
    } else {
        None
    }
}

/// The subscriptions as an OPML document.
pub fn write(subscriptions: &[Subscription]) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<opml version=\"2.0\">\n  <head>\n    \
         <title>Papers Desktop feeds</title>\n  </head>\n  <body>\n",
    );
    for s in subscriptions {
        let title = s.title.clone().unwrap_or_else(|| s.feed.to_string());
        let (kind, address) = match &s.feed {
            Feed::Rss { url } => ("rss", format!("xmlUrl=\"{}\"", escape(url))),
            Feed::Crossref { issn } => ("crossref", format!("issn=\"{}\"", escape(issn))),
            Feed::OpenAlex { filter } => ("openalex", format!("filter=\"{}\"", escape(filter))),
        };
        out.push_str(&format!(
            "    <outline type=\"{kind}\" text=\"{title}\" title=\"{title}\" category=\"{}\" {address}/>\n",
            kind_name(s.kind),
            title = escape(&title),
        ));
    }
    out.push_str("  </body>\n</opml>\n");
    out
}

/// The value of an attribute, unescaped; none if blank.
fn attribute(element: &BytesStart, name: &str) -> Option<String> {
    let attribute = element.try_get_attribute(name).ok().flatten()?;
    let value = attribute
        .normalized_value(XmlVersion::Implicit1_0)
        .ok()?
        .trim()
        .to_string();
    (!value.is_empty()).then_some(value)
}

/// The feed of an outline, if it is one: `xmlUrl`, `issn` or `filter` by its type.
fn feed_of(element: &BytesStart) -> Option<Feed> {
    let get = |name| attribute(element, name);
    match get("type").map(|t| t.to_lowercase()).as_deref() {
        Some("crossref") => get("issn").map(|issn| Feed::Crossref { issn }),
        Some("openalex") => get("filter").map(|filter| Feed::OpenAlex { filter }),
        _ => get("xmlUrl").map(|url| Feed::Rss { url }),
    }
}

/// The subscriptions of an OPML document, in order.
pub fn read(text: &str) -> std::result::Result<Vec<Outline>, String> {
    let mut reader = Reader::from_str(text);
    reader.config_mut().check_end_names = false;
    // the kinds the groups open around an outline say
    let mut groups: Vec<Option<SourceKind>> = Vec::new();
    let mut outlines = Vec::new();
    let mut seen_opml = false;
    loop {
        let event = reader.read_event().map_err(|e| e.to_string())?;
        let (element, empty) = match &event {
            Event::Start(e) => (e, false),
            Event::Empty(e) => (e, true),
            Event::End(e) => {
                if e.local_name().as_ref() == "outline" {
                    groups.pop();
                }
                continue;
            }
            Event::Eof => break,
            _ => continue,
        };
        match element.local_name().as_ref() {
            "opml" => seen_opml = true,
            "outline" => {
                let category = attribute(element, "category").and_then(|c| kind_of(&c));
                match feed_of(element) {
                    Some(feed) => {
                        let group = groups.iter().rev().find_map(|g| *g);
                        outlines.push(Outline {
                            feed,
                            title: attribute(element, "title")
                                .or_else(|| attribute(element, "text")),
                            kind: category.or(group).unwrap_or(SourceKind::Journal),
                        });
                        if !empty {
                            groups.push(None);
                        }
                    }
                    None if !empty => {
                        let name =
                            attribute(element, "text").or_else(|| attribute(element, "title"));
                        groups.push(category.or_else(|| name.as_deref().and_then(kind_of)));
                    }
                    None => {}
                }
            }
            _ => {}
        }
    }
    match seen_opml {
        true => Ok(outlines),
        false => Err("not an OPML document".to_string()),
    }
}

impl Feeds {
    /// The subscriptions as an OPML document.
    pub fn opml(&self) -> String {
        write(self.subscriptions())
    }

    /// Subscribes to the feeds and sweeps of an OPML document, titled as
    /// there; those subscribed already, and those whose address or ISSN is not
    /// valid, are passed over. The number of new subscriptions.
    pub fn import_opml(&mut self, text: &str) -> Result<usize> {
        let outlines = read(text).map_err(|message| Error::Opml { message })?;
        let mut added = 0;
        for outline in outlines {
            let subscription = match self.subscribe(outline.feed, outline.kind) {
                Ok(subscription) => subscription,
                Err(
                    Error::Subscribed { .. }
                    | Error::Url { .. }
                    | Error::Issn { .. }
                    | Error::Filter { .. },
                ) => continue,
                Err(e) => return Err(e),
            };
            if let Some(title) = outline.title {
                self.edit_subscription(&subscription.id, |s| s.title = Some(title))?;
            }
            added += 1;
        }
        Ok(added)
    }
}
