//! The subset of CSL-JSON that papers writes (`papers list --format csl-json`).

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub(crate) struct Entry {
    pub id: String,
    #[serde(rename = "citation-key")]
    pub citation_key: Option<String>,
    #[serde(rename = "type", default)]
    pub kind: String,
    pub title: Option<String>,
    #[serde(default)]
    pub author: Vec<Name>,
    #[serde(default)]
    pub editor: Vec<Name>,
    pub issued: Option<Date>,
    #[serde(rename = "container-title")]
    pub container_title: Option<String>,
    #[serde(rename = "DOI")]
    pub doi: Option<String>,
    #[serde(rename = "URL")]
    pub url: Option<String>,
    pub keyword: Option<String>,
    #[serde(default)]
    pub custom: Custom,
}

/// papers puts the file links of an entry in `custom.file`, as absolute paths.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct Custom {
    #[serde(default)]
    pub file: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Name {
    pub family: Option<String>,
    pub given: Option<String>,
    #[serde(rename = "non-dropping-particle")]
    pub particle: Option<String>,
    /// an organisation, or a name papers could not split
    pub literal: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Date {
    #[serde(rename = "date-parts", default)]
    pub date_parts: Vec<Vec<Value>>,
    pub literal: Option<String>,
    pub raw: Option<String>,
}

impl Date {
    /// The first date part, else the first four-digit number of the text.
    pub fn year(&self) -> Option<i32> {
        match self.date_parts.first().and_then(|p| p.first()) {
            Some(Value::Number(n)) => n.as_i64().and_then(|y| i32::try_from(y).ok()),
            Some(Value::String(s)) => s.trim().parse().ok(),
            _ => self
                .literal
                .as_deref()
                .or(self.raw.as_deref())
                .and_then(first_year),
        }
    }
}

fn first_year(text: &str) -> Option<i32> {
    text.split(|c: char| !c.is_ascii_digit())
        .find(|w| w.len() == 4)
        .and_then(|w| w.parse().ok())
}

pub(crate) fn parse(json: &str) -> serde_json::Result<Vec<Entry>> {
    serde_json::from_str(json)
}
