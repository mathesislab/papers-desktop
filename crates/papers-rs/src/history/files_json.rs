//! `.papers/files.json`: the checksums of the files that the entries link to, and the version of
//! the PDFs that `papers file fetch` attached (`{sha256: published, accepted or submitted}`; a file
//! without version: unknown) (design/history.md, section 10). It is one of the files that a state
//! keeps, so that undo finds a file by its content.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use super::ojson::J;
use super::restore::file_digest;
use crate::config::files::write_atomic;
use crate::config::paths::{isfile, join, normpath, relpath};
use crate::config::{Config, config_root};
use crate::error::Result;

/// `.papers/files.json`, relative to the library directory (and to the
/// directory of the bibtex, where the states look for it).
pub const FILES_JSON: &str = ".papers/files.json";

/// The versions of a PDF, the final one first (`VERSIONS`).
pub const VERSIONS: [&str; 3] = ["published", "accepted", "submitted"];

/// The checksums file of a library.
#[derive(Debug, Clone)]
pub struct FilesJson {
    pub path: String,
    /// The directory of the bibtex: the paths of the file are relative to it.
    pub libdir: String,
    pub absolute_paths: bool,
}

impl FilesJson {
    /// The checksums file of a library, or `None` (a bibtex without library).
    pub fn of(config: &Config, libdir: &str) -> Option<FilesJson> {
        let file = config.file.as_deref()?;
        if !config.is_library {
            return None;
        }
        Some(FilesJson {
            path: join(&config_root(file), FILES_JSON),
            libdir: libdir.to_string(),
            absolute_paths: config.absolute_paths,
        })
    }

    /// The text of the file, none when it is missing or unreadable (`utf-8-sig`: a byte order
    /// mark is dropped).
    fn text(&self) -> Option<String> {
        let text = String::from_utf8(fs::read(&self.path).ok()?).ok()?;
        Some(
            text.strip_prefix('\u{feff}')
                .map_or(text.clone(), str::to_string),
        )
    }

    /// `{absolute path: sha256}` of the library; nothing when the file is
    /// missing or unreadable.
    pub fn load(&self) -> BTreeMap<String, String> {
        self.text()
            .map(|text| parse(&text, &self.libdir))
            .unwrap_or_default()
    }

    /// `{sha256: version}` of the library (`load_versions`).
    pub fn load_versions(&self) -> BTreeMap<String, String> {
        self.text()
            .map(|text| parse_versions(&text))
            .unwrap_or_default()
    }

    /// `{absolute path: version}` of the linked files whose version is known (`file_versions`).
    pub fn file_versions(&self) -> BTreeMap<String, String> {
        let Some(text) = self.text() else {
            return BTreeMap::new();
        };
        let versions = parse_versions(&text);
        parse(&text, &self.libdir)
            .into_iter()
            .filter_map(|(path, digest)| versions.get(&digest).map(|v| (path, v.clone())))
            .collect()
    }

    /// Write the checksums (no checksum: no file), and the versions (none: those of the file) of
    /// the checksums kept.
    pub fn save(
        &self,
        hashes: &BTreeMap<String, String>,
        versions: Option<&BTreeMap<String, String>>,
    ) -> Result<()> {
        let versions = versions.cloned().unwrap_or_else(|| self.load_versions());
        if hashes.is_empty() {
            let _ = fs::remove_file(&self.path);
            return Ok(());
        }
        let mut stored: Vec<(String, J)> = hashes
            .iter()
            .map(|(p, digest)| {
                let key = if self.absolute_paths {
                    p.clone()
                } else {
                    relpath(p, &self.libdir)
                };
                (key, J::str(digest))
            })
            .collect();
        stored.sort_by(|a, b| a.0.cmp(&b.0));
        stored.dedup_by(|a, b| a.0 == b.0);
        let mut content = vec![("files".to_string(), J::Obj(stored))];
        let linked: BTreeSet<&String> = hashes.values().collect();
        let kept: Vec<(String, J)> = versions
            .iter()
            .filter(|(digest, _)| linked.contains(digest))
            .map(|(digest, version)| (digest.clone(), J::str(version)))
            .collect();
        if !kept.is_empty() {
            content.push(("versions".to_string(), J::Obj(kept)));
        }
        if let Some(dir) = std::path::Path::new(&self.path).parent() {
            fs::create_dir_all(dir)?;
        }
        write_atomic(&self.path, &(J::Obj(content).dumps() + "\n"))
    }

    /// At the save of a library (before the bibtex): keep the checksums of the
    /// files still linked, drop the others; with `checksums`, compute those of
    /// the `touched` files (the `dst` of the operations of the command so far,
    /// and the files linked now that were not when the library was loaded)
    /// that are linked and exist. The PDFs that `papers file fetch` attached or replaced
    /// (`fetched`: `{normalized absolute path: version}`) that are linked and exist get their
    /// checksum (also without `checksums`) and their version; a version whose checksum is no
    /// longer in the file is dropped. Written when the result differs. `links`
    /// are the files that the entries link to now (normalized absolute paths).
    /// Whether it changed.
    pub fn update(
        &self,
        links: &BTreeSet<String>,
        touched: &BTreeSet<String>,
        checksums: bool,
        fetched: &BTreeMap<String, String>,
    ) -> Result<bool> {
        let (hashes, versions) = (self.load(), self.load_versions());
        let mut kept: BTreeMap<String, String> = hashes
            .iter()
            .filter(|(p, _)| links.contains(*p))
            .map(|(p, d)| (p.clone(), d.clone()))
            .collect();
        let mut computed = BTreeSet::new();
        if checksums {
            for path in touched.intersection(links) {
                computed.insert(path);
                if isfile(path)
                    && let Some(digest) = file_digest(path)
                {
                    kept.insert(path.clone(), digest);
                }
            }
        }
        let mut new_versions = versions.clone();
        for (path, version) in fetched {
            if !links.contains(path) || !isfile(path) {
                continue;
            }
            if !computed.contains(path)
                && let Some(digest) = file_digest(path)
            {
                kept.insert(path.clone(), digest);
            }
            if let Some(digest) = kept.get(path) {
                new_versions.insert(digest.clone(), version.clone());
            }
        }
        let linked: BTreeSet<&String> = kept.values().collect();
        new_versions.retain(|digest, _| linked.contains(digest));
        if kept != hashes || new_versions != versions {
            self.save(&kept, Some(&new_versions))?;
            return Ok(true);
        }
        Ok(false)
    }
}

/// `{sha256: version}` of the text of a checksums file (`parse_versions`): a file without
/// `versions` has none, and a value other than the [`VERSIONS`] is ignored.
pub fn parse_versions(text: &str) -> BTreeMap<String, String> {
    let Some(J::Obj(top)) = J::parse(text) else {
        return BTreeMap::new();
    };
    let Some((_, J::Obj(stored))) = top.into_iter().find(|(k, _)| k == "versions") else {
        return BTreeMap::new();
    };
    stored
        .into_iter()
        .filter_map(|(digest, version)| match version {
            J::Str(v) if VERSIONS.contains(&v.as_str()) => Some((digest, v)),
            _ => None,
        })
        .collect()
}

/// `{absolute path: sha256}` of the text of a checksums file: each key, with any `\` read as `/`,
/// resolved against the directory of the bibtex and normalized (so two spellings of one file are
/// one link); an unreadable text means no checksums.
pub fn parse(text: &str, libdir: &str) -> BTreeMap<String, String> {
    let Some(J::Obj(top)) = J::parse(text) else {
        return BTreeMap::new();
    };
    let Some((_, J::Obj(stored))) = top.into_iter().find(|(k, _)| k == "files") else {
        return BTreeMap::new();
    };
    stored
        .into_iter()
        .filter_map(|(path, digest)| match digest {
            // written with / (with the separator of the system by papers on Windows before 3.0.0b6)
            J::Str(d) => Some((normpath(&join(libdir, &path.replace('\\', "/"))), d)),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn written_as_papers_writes_it() {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let f = FilesJson {
            path: join(&root, ".papers/files.json"),
            libdir: root.clone(),
            absolute_paths: false,
        };
        let hashes: BTreeMap<String, String> = [
            (format!("{root}/files/doe_2000_a.pdf"), "2493".to_string()),
            (format!("{root}/../out/y.pdf"), "7e4f".to_string()),
        ]
        .into_iter()
        .map(|(p, d)| (normpath(&p), d))
        .collect();
        f.save(&hashes, None).unwrap();
        assert_eq!(
            fs::read_to_string(&f.path).unwrap(),
            "{\n \"files\": {\n  \"../out/y.pdf\": \"7e4f\",\n  \"files/doe_2000_a.pdf\": \"2493\"\n }\n}\n"
        );
        assert_eq!(f.load(), hashes);
        f.save(&BTreeMap::new(), None).unwrap();
        assert!(!isfile(&f.path));
        assert!(f.load().is_empty());
    }

    #[test]
    fn update_keeps_the_linked_and_hashes_the_touched() {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let f = FilesJson {
            path: join(&root, ".papers/files.json"),
            libdir: root.clone(),
            absolute_paths: false,
        };
        let a = format!("{root}/a.pdf");
        let b = format!("{root}/b.pdf");
        fs::write(&a, "a").unwrap();
        fs::write(&b, "b").unwrap();
        let links: BTreeSet<String> = [a.clone(), b.clone()].into();
        let none = BTreeMap::new();
        assert!(f.update(&links, &[a.clone()].into(), true, &none).unwrap());
        assert_eq!(f.load().len(), 1);
        assert!(!f.update(&links, &[a.clone()].into(), true, &none).unwrap());
        // b is linked but not touched: no checksum; a no longer linked: dropped
        assert!(
            f.update(&[b.clone()].into(), &BTreeSet::new(), true, &none)
                .unwrap()
        );
        assert!(f.load().is_empty() && !isfile(&f.path));
        // without the checksums setting nothing is computed
        assert!(!f.update(&links, &links, false, &none).unwrap());
        // a fetched PDF gets its checksum and its version, also without the setting
        let fetched: BTreeMap<String, String> = [(a.clone(), "accepted".to_string())].into();
        assert!(f.update(&links, &BTreeSet::new(), false, &fetched).unwrap());
        let digest = file_digest(&a).unwrap();
        assert_eq!(
            fs::read_to_string(&f.path).unwrap(),
            format!(
                "{{\n \"files\": {{\n  \"a.pdf\": \"{digest}\"\n }},\n \"versions\": {{\n  \"{digest}\": \"accepted\"\n }}\n}}\n"
            )
        );
        assert_eq!(
            f.file_versions(),
            [(a.clone(), "accepted".to_string())].into()
        );
        // the version goes with its checksum
        assert!(
            f.update(&[b.clone()].into(), &[b.clone()].into(), true, &none)
                .unwrap()
        );
        assert!(f.load_versions().is_empty());
        assert!(!fs::read_to_string(&f.path).unwrap().contains("versions"));
    }
}
