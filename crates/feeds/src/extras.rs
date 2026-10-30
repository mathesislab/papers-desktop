//! The elements of a feed's items that feed-rs leaves out: `dc:identifier`,
//! `dc:description` (Wiley's abstract), `dc:creator` of Atom (arXiv),
//! `prism:doi`, `prism:publicationName`, `arxiv:doi`. One pass of quick-xml
//! over the feed, each item found again by its id or its link.

use std::collections::HashMap;

use quick_xml::NsReader;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::{Namespace, ResolveResult};

const DC: &str = "http://purl.org/dc/elements/1.1/";
const PRISM: &str = "http://prismstandard.org/namespaces/";
const ARXIV: &str = "http://arxiv.org/schemas/atom";
const RSS1: &str = "http://purl.org/rss/1.0/";
const ATOM: &str = "http://www.w3.org/2005/Atom";

/// What an item says beyond feed-rs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Extra {
    /// `dc:identifier`
    pub identifiers: Vec<String>,
    /// `prism:doi`, `arxiv:doi`
    pub dois: Vec<String>,
    /// `dc:description`
    pub description: Option<String>,
    /// `dc:creator`
    pub creators: Vec<String>,
    /// `prism:publicationName`
    pub publication: Option<String>,
}

/// The extras of the items of a feed, by the items' ids and links.
#[derive(Debug, Default)]
pub(crate) struct Extras {
    items: Vec<Extra>,
    by_key: HashMap<String, usize>,
}

/// An element of an item that is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    /// `guid`, `id` or `link`: what the item is found again by
    Key,
    Identifier,
    Doi,
    Description,
    Creator,
    Publication,
}

impl Field {
    fn of(ns: &ResolveResult, local: &str) -> Option<Field> {
        let ns = match ns {
            ResolveResult::Bound(Namespace(ns)) => Some(*ns),
            _ => None,
        };
        let feed = ns.is_none_or(|ns| ns == RSS1 || ns == ATOM);
        match (ns, local) {
            (_, "guid" | "id" | "link") if feed => Some(Field::Key),
            (Some(DC), "identifier") => Some(Field::Identifier),
            (Some(DC), "description") => Some(Field::Description),
            (Some(DC), "creator") => Some(Field::Creator),
            (Some(ns), "doi") if ns.starts_with(PRISM) || ns == ARXIV => Some(Field::Doi),
            (Some(ns), "publicationName") if ns.starts_with(PRISM) => Some(Field::Publication),
            _ => None,
        }
    }
}

/// The item being read.
#[derive(Default)]
struct Open {
    depth: usize,
    extra: Extra,
    keys: Vec<String>,
    /// the element being read, and its text so far
    field: Option<(Field, String)>,
}

impl Open {
    fn close_field(&mut self) {
        let Some((field, text)) = self.field.take() else {
            return;
        };
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        let extra = &mut self.extra;
        match field {
            Field::Key => self.keys.push(text),
            Field::Identifier => extra.identifiers.push(text),
            Field::Doi => extra.dois.push(text),
            Field::Description => extra.description = Some(text),
            Field::Creator => extra.creators.push(text),
            Field::Publication => extra.publication = Some(text),
        }
    }

    /// An Atom `<link href=…>`.
    fn href(&mut self, element: &BytesStart) {
        if let Ok(Some(href)) = element.try_get_attribute("href") {
            let href = href.value.trim().to_string();
            self.keys.push(href);
        }
    }
}

impl Extras {
    /// The extras of a feed's items; an error if it is not XML.
    pub fn read(xml: &[u8]) -> Result<Extras, String> {
        let mut reader = NsReader::from_reader(xml);
        reader.config_mut().check_end_names = false;
        let mut extras = Extras::default();
        let mut depth = 0;
        let mut open: Option<Open> = None;
        loop {
            let (ns, event) = reader.read_resolved_event().map_err(|e| e.to_string())?;
            match event {
                Event::Start(e) => {
                    depth += 1;
                    let local = e.local_name();
                    match &mut open {
                        None if matches!(local.as_ref(), "item" | "entry") => {
                            open = Some(Open {
                                depth,
                                ..Open::default()
                            });
                        }
                        Some(item) if depth == item.depth + 1 => {
                            let field = Field::of(&ns, local.as_ref());
                            if field == Some(Field::Key) && local.as_ref() == "link" {
                                item.href(&e);
                            }
                            item.field = field.map(|f| (f, String::new()));
                        }
                        _ => {}
                    }
                }
                Event::Empty(e) => {
                    if let Some(item) = &mut open
                        && depth == item.depth
                        && e.local_name().as_ref() == "link"
                    {
                        item.href(&e);
                    }
                }
                Event::Text(t) => push(&mut open, &t.xml10_content()),
                Event::CData(t) => push(&mut open, &t.xml10_content()),
                Event::GeneralRef(r) => {
                    let text = match r.resolve_char_ref() {
                        Ok(Some(c)) => c.to_string(),
                        _ => predefined(&r.xml10_content()),
                    };
                    push(&mut open, &text);
                }
                Event::End(_) => {
                    if let Some(item) = &mut open {
                        if depth == item.depth + 1 {
                            item.close_field();
                        } else if depth == item.depth {
                            let item = open.take().expect("an open item");
                            extras.add(item);
                        }
                    }
                    depth = depth.saturating_sub(1);
                }
                Event::Eof => break,
                _ => {}
            }
        }
        Ok(extras)
    }

    fn add(&mut self, item: Open) {
        let index = self.items.len();
        self.items.push(item.extra);
        for key in item.keys {
            self.by_key.entry(key).or_insert(index);
        }
    }

    /// The extras of the item of an id or of one of these links.
    pub fn of<'a>(&self, id: &'a str, links: impl IntoIterator<Item = &'a str>) -> Option<&Extra> {
        std::iter::once(id)
            .chain(links)
            .find_map(|key| self.by_key.get(key.trim()))
            .map(|&i| &self.items[i])
    }
}

/// Text of the element being read.
fn push(open: &mut Option<Open>, text: &str) {
    if let Some(Open {
        field: Some((_, buffer)),
        ..
    }) = open
    {
        buffer.push_str(text);
    }
}

/// An entity of XML as its character; any other (`&nbsp;` of HTML) as written,
/// for the HTML decoding of the text.
fn predefined(name: &str) -> String {
    match name {
        "lt" => "<".to_string(),
        "gt" => ">".to_string(),
        "amp" => "&".to_string(),
        "quot" => "\"".to_string(),
        "apos" => "'".to_string(),
        other => format!("&{other};"),
    }
}
