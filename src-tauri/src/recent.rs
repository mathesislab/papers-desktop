//! The papers used last, opened or copied: the empty panel lists them.

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use papers_core::{Hit, Item, Library};
use serde::{Deserialize, Serialize};

const KEEP: usize = 10;

/// A paper by its .bib file and key, which stay when the ids of the library
/// change with its files.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Paper {
    bib: PathBuf,
    key: String,
}

/// The recent papers, newest first, kept in a file of the app.
pub struct Recent {
    path: PathBuf,
    papers: Mutex<Vec<Paper>>,
}

impl Recent {
    /// The papers of the file; none without it, or with a broken one.
    pub fn load(path: PathBuf) -> Recent {
        let papers = fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Recent {
            path,
            papers: Mutex::new(papers),
        }
    }

    /// The item was used: it comes first.
    pub fn add(&self, item: &Item) {
        let paper = Paper {
            bib: item.bib.clone(),
            key: item.key.clone(),
        };
        let mut papers = self.papers.lock().unwrap();
        papers.retain(|p| *p != paper);
        papers.insert(0, paper);
        papers.truncate(KEEP);
        self.save(&papers);
    }

    pub fn clear(&self) {
        let mut papers = self.papers.lock().unwrap();
        papers.clear();
        self.save(&papers);
    }

    /// The recent papers still in the library, as hits.
    pub fn hits(&self, library: &Library) -> Vec<Hit> {
        let papers = self.papers.lock().unwrap();
        papers
            .iter()
            .filter_map(|p| library.find(&p.bib, &p.key))
            .filter_map(|item| library.hit(&item.id))
            .collect()
    }

    /// Losing the list is no error worth more than a line in the log.
    fn save(&self, papers: &[Paper]) {
        let saved = fs::create_dir_all(self.path.parent().unwrap())
            .and_then(|()| fs::write(&self.path, serde_json::to_string_pretty(papers).unwrap()));
        if let Err(e) = saved {
            eprintln!("papers-desktop: cannot save {}: {e}", self.path.display());
        }
    }
}
