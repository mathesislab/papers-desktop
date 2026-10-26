//! The CSL styles of the citations: a few bundled ones (see styles/README.md),
//! or any .csl file.

use std::fs;

use serde::Serialize;

pub const DEFAULT: &str = "copernicus-publications";

const BUNDLED: &[(&str, &str)] = &[
    (
        "copernicus-publications",
        include_str!("../styles/copernicus-publications.csl"),
    ),
    (
        "american-geophysical-union",
        include_str!("../styles/american-geophysical-union.csl"),
    ),
    (
        "american-meteorological-society",
        include_str!("../styles/american-meteorological-society.csl"),
    ),
    ("nature", include_str!("../styles/nature.csl")),
    ("science", include_str!("../styles/science.csl")),
    ("apa", include_str!("../styles/apa.csl")),
    (
        "elsevier-harvard",
        include_str!("../styles/elsevier-harvard.csl"),
    ),
    (
        "chicago-author-date",
        include_str!("../styles/chicago-author-date.csl"),
    ),
];

/// The citation processor takes this locale for every language.
const LOCALE: &str = include_str!("../styles/locales-en-US.xml");

/// A style for the citation processor, sent to the panel.
#[derive(Debug, Clone, Serialize)]
pub struct Style {
    pub name: String,
    /// the CSL XML
    pub style: String,
    /// the CSL locale XML
    pub locale: String,
}

impl Style {
    /// A bundled style by name, else the .csl file at that path.
    pub fn load(name: &str) -> Result<Style, String> {
        let style = match BUNDLED.iter().find(|(n, _)| *n == name) {
            Some((_, xml)) => xml.to_string(),
            None => fs::read_to_string(crate::settings::expand(name)).map_err(|e| {
                let names: Vec<_> = BUNDLED.iter().map(|(n, _)| *n).collect();
                format!("citation style {name}: {e} (bundled: {})", names.join(", "))
            })?,
        };
        Ok(Style {
            name: name.to_string(),
            style,
            locale: LOCALE.to_string(),
        })
    }
}
