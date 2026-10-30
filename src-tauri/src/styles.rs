//! The CSL styles of the citations: a few bundled ones (see styles/README.md),
//! or any .csl file; and the CSL-JSON of a paper, as papers exports it.

use std::fs;

use papers_rs::formats::csl::entry_to_csl;
use papers_rs::history::ojson::J;
use serde::Serialize;
use serde_json::{Map, Value};

use crate::input::entry_of;

/// The CSL-JSON item of the BibTeX of a paper, as `papers export --format csl` writes it
/// (without the file links), for the citation processor.
pub fn item(bibtex: &str) -> Result<Value, String> {
    Ok(value(&entry_to_csl(&entry_of(bibtex, "The paper")?, &[])))
}

fn value(j: &J) -> Value {
    match j {
        J::Null => Value::Null,
        J::Bool(b) => Value::Bool(*b),
        J::Int(n) => Value::from(*n),
        J::Float(x) => Value::from(*x),
        J::Str(s) => Value::String(s.clone()),
        J::Arr(items) => Value::Array(items.iter().map(value).collect()),
        J::Obj(pairs) => Value::Object(
            pairs
                .iter()
                .map(|(k, v)| (k.clone(), value(v)))
                .collect::<Map<_, _>>(),
        ),
    }
}

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

/// The names of the bundled styles.
pub fn names() -> Vec<&'static str> {
    BUNDLED.iter().map(|(name, _)| *name).collect()
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_as_papers_exports_it() {
        let csl = item(
            "@article{robinson2012,\n author = {Robinson, Alexander and Calov, Reinhard},\n doi = {https://doi.org/10.1038/nclimate1449},\n title = {Ice -- sheet model},\n journal = {Ice -- {\\textendash} sheets},\n year = {2012},\n file = {:files/r.pdf:pdf}\n}",
        )
        .unwrap();
        assert_eq!(csl["id"], "robinson2012");
        assert_eq!(csl["DOI"], "10.1038/nclimate1449");
        assert_eq!(csl["author"][1]["family"], "Calov");
        assert_eq!(csl["issued"]["date-parts"][0][0], 2012);
        // papers keeps `--` in a value without LaTeX, else reads it as a dash
        assert_eq!(csl["title"], "Ice -- sheet model");
        assert_eq!(csl["container-title"], "Ice – – sheets");
        assert!(csl.get("file").is_none());
    }
}
