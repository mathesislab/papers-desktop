//! `.papers/files.json`: the checksums of the files that the entries link to
//! (spec/history.md, section 10). It is one of the files that a state keeps,
//! so that undo finds a file by its content.

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

    /// `{absolute path: sha256}` of the library; nothing when the file is
    /// missing or unreadable.
    pub fn load(&self) -> BTreeMap<String, String> {
        match fs::read(&self.path)
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
        {
            Some(text) => parse(&text, &self.libdir),
            None => BTreeMap::new(),
        }
    }

    /// Write the checksums (no checksum: no file).
    pub fn save(&self, hashes: &BTreeMap<String, String>) -> Result<()> {
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
        if let Some(dir) = std::path::Path::new(&self.path).parent() {
            fs::create_dir_all(dir)?;
        }
        let text = J::Obj(vec![("files".into(), J::Obj(stored))]).dumps() + "\n";
        write_atomic(&self.path, &text)
    }

    /// At the save of a library (before the bibtex): keep the checksums of the
    /// files still linked, drop the others; with `checksums`, compute those of
    /// the `touched` files (the `dst` of the operations of the command so far,
    /// and the files linked now that were not when the library was loaded)
    /// that are linked and exist. Written when the result differs. `links`
    /// are the files that the entries link to now (normalized absolute paths).
    /// Whether it changed.
    pub fn update(
        &self,
        links: &BTreeSet<String>,
        touched: &BTreeSet<String>,
        checksums: bool,
    ) -> Result<bool> {
        let hashes = self.load();
        let mut kept: BTreeMap<String, String> = hashes
            .iter()
            .filter(|(p, _)| links.contains(*p))
            .map(|(p, d)| (p.clone(), d.clone()))
            .collect();
        if checksums {
            for path in touched.intersection(links) {
                if isfile(path)
                    && let Some(digest) = file_digest(path)
                {
                    kept.insert(path.clone(), digest);
                }
            }
        }
        if kept != hashes {
            self.save(&kept)?;
            return Ok(true);
        }
        Ok(false)
    }
}

/// `{absolute path: sha256}` of the text of a checksums file: each key
/// resolved against the directory of the bibtex and normalized; an
/// unreadable text means no checksums.
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
            J::Str(d) => Some((normpath(&join(libdir, &path)), d)),
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
        f.save(&hashes).unwrap();
        assert_eq!(
            fs::read_to_string(&f.path).unwrap(),
            "{\n \"files\": {\n  \"../out/y.pdf\": \"7e4f\",\n  \"files/doe_2000_a.pdf\": \"2493\"\n }\n}\n"
        );
        assert_eq!(f.load(), hashes);
        f.save(&BTreeMap::new()).unwrap();
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
        assert!(f.update(&links, &[a.clone()].into(), true).unwrap());
        assert_eq!(f.load().len(), 1);
        assert!(!f.update(&links, &[a.clone()].into(), true).unwrap());
        // b is linked but not touched: no checksum; a no longer linked: dropped
        assert!(
            f.update(&[b.clone()].into(), &BTreeSet::new(), true)
                .unwrap()
        );
        assert!(f.load().is_empty() && !isfile(&f.path));
        // without the checksums setting nothing is computed
        assert!(!f.update(&links, &links, false).unwrap());
    }
}
