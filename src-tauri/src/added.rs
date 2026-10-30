//! When the papers came into the library: the *Recently added* view lists them, the newest first.
//! Nothing in a .bib says it, so the app keeps it in a file of its own (`added.json` in its data
//! folder, as `recent.json`; not portable: another computer starts again): for each .bib, the time
//! each key was first seen.
//!
//! The library is compared with the file each time it is read. A key not there yet is dated by the
//! papers history (the time of the step that brought it: exact for a paper added through papers or
//! papers-rs, the app included); else, the first time a .bib is seen, by the modification time of
//! its PDF, or not at all (an older paper); else, later, by the time it is seen. A key that changes
//! (`papers edit --key`, `check --auto-key`) keeps its date: a key that comes in place of one that
//! goes, the same paper by its DOI or its title and year, takes the date of that one.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use papers_core::{Identity, Library};
use papers_rs::config::{Config, MachineConfig, find_config};
use papers_rs::history::History;
use serde::{Deserialize, Serialize};

use crate::writer::papers_folder;

/// A paper of a .bib, as the file keeps it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Seen {
    /// when it came, in seconds since the epoch; none: there when the .bib was first seen, from a
    /// time nothing tells
    added: Option<f64>,
    /// what tells the paper apart when its key changes
    #[serde(flatten)]
    identity: Identity,
}

/// The papers of each .bib (its real path), by key.
type Known = BTreeMap<PathBuf, BTreeMap<String, Seen>>;

/// The library as it is now, to compare with the file.
#[derive(Debug, Clone, Default)]
pub struct Current {
    /// the real paths of its .bib files
    bibs: Vec<PathBuf>,
    papers: Vec<Paper>,
}

/// A paper of the library as it is now.
#[derive(Debug, Clone)]
struct Paper {
    /// the real path of its .bib
    bib: PathBuf,
    key: String,
    identity: Identity,
    /// the first PDF it links, if any
    pdf: Option<PathBuf>,
}

/// The real path of a .bib, which the file is by: the library may name it otherwise.
fn real(bib: &Path) -> PathBuf {
    fs::canonicalize(bib).unwrap_or_else(|_| bib.to_path_buf())
}

/// The library now, to compare with the file: quick, under the lock of the library.
pub fn current(library: &Library) -> Current {
    let bibs: HashMap<&Path, PathBuf> = library
        .bibs()
        .iter()
        .map(|b| (b.as_path(), real(b)))
        .collect();
    let papers = library
        .items()
        .iter()
        .map(|item| Paper {
            bib: bibs
                .get(item.bib.as_path())
                .cloned()
                .unwrap_or_else(|| item.bib.clone()),
            key: item.key.clone(),
            identity: item.identity(),
            pdf: item
                .files
                .iter()
                .find(|f| f.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")))
                .cloned(),
        })
        .collect();
    Current {
        bibs: library
            .bibs()
            .iter()
            .map(|b| bibs[b.as_path()].clone())
            .collect(),
        papers,
    }
}

fn seconds(time: SystemTime) -> f64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

/// When a PDF was last modified: when it was downloaded, roughly (a move keeps the time).
fn modified(pdf: &Path) -> Option<f64> {
    fs::metadata(pdf)
        .and_then(|m| m.modified())
        .ok()
        .map(seconds)
}

/// The papers known, brought up to date with the library now (`now` is the time; the .bib files
/// no longer in the library are kept as they were, in case they come back). `history` tells when
/// the entries of a .bib came into it; it is asked once per .bib, and only for a .bib with keys to
/// date.
fn updated(
    known: &Known,
    current: &Current,
    now: f64,
    mut history: impl FnMut(&Path) -> HashMap<String, f64>,
) -> Known {
    let mut by_bib: BTreeMap<&Path, Vec<&Paper>> = current
        .bibs
        .iter()
        .map(|b| (b.as_path(), Vec::new()))
        .collect();
    for paper in &current.papers {
        by_bib.entry(&paper.bib).or_default().push(paper);
    }
    let mut next = known.clone();
    for (bib, papers) in by_bib {
        let before = known.get(bib);
        let first = before.is_none();
        let here: HashSet<&str> = papers.iter().map(|p| p.key.as_str()).collect();
        let mut keys: BTreeMap<String, Seen> = BTreeMap::new();
        let mut new: Vec<&Paper> = Vec::new();
        for paper in papers {
            match before.and_then(|b| b.get(&paper.key)) {
                Some(seen) => {
                    keys.insert(
                        paper.key.clone(),
                        Seen {
                            added: seen.added,
                            identity: paper.identity.clone(),
                        },
                    );
                }
                None => new.push(paper),
            }
        }
        // the keys gone: one of them may be the same paper under a new key
        let mut gone: Vec<&Seen> = before
            .into_iter()
            .flatten()
            .filter(|(key, _)| !here.contains(key.as_str()))
            .map(|(_, seen)| seen)
            .collect();
        let mut undated: Vec<&Paper> = Vec::new();
        for paper in new {
            match gone
                .iter()
                .position(|seen| seen.identity.same_paper(&paper.identity))
            {
                Some(i) => {
                    let seen = gone.remove(i);
                    keys.insert(
                        paper.key.clone(),
                        Seen {
                            added: seen.added,
                            identity: paper.identity.clone(),
                        },
                    );
                }
                None => undated.push(paper),
            }
        }
        if !undated.is_empty() {
            let times = history(bib);
            for paper in undated {
                let added = times.get(&paper.key).copied().or_else(|| {
                    if first {
                        paper.pdf.as_deref().and_then(modified)
                    } else {
                        Some(now)
                    }
                });
                keys.insert(
                    paper.key.clone(),
                    Seen {
                        added,
                        identity: paper.identity.clone(),
                    },
                );
            }
        }
        next.insert(bib.to_path_buf(), keys);
    }
    next
}

/// When the entries of a .bib came into it, as the papers history tells (the store that papers,
/// papers-rs and the app share, read by papers-rs): the history of its papers library, or of the
/// .bib on its own (papers run with `--bibtex`). Nothing without one, or when it cannot be read.
fn history_times(bib: &Path, machine: MachineConfig) -> HashMap<String, f64> {
    let config = match papers_folder(bib) {
        Some(folder) => find_config(&folder.to_string_lossy(), machine),
        None => {
            let mut config = Config::new(machine);
            config.bibtex = Some(bib.to_string_lossy().into_owned());
            Ok(config)
        }
    };
    let history = config.and_then(|mut config| History::open(&mut config, false, false));
    match history {
        Ok(Some(history)) => history.entry_times(),
        Ok(None) => HashMap::new(),
        Err(e) => {
            eprintln!(
                "papers-desktop: the history of {} cannot be read: {}",
                bib.display(),
                e.message()
            );
            HashMap::new()
        }
    }
}

/// The history of the user's machine (its configuration of papers: `history-dir`).
pub fn from_history(bib: &Path) -> HashMap<String, f64> {
    match MachineConfig::load(None) {
        Ok(machine) => history_times(bib, machine),
        Err(e) => {
            eprintln!(
                "papers-desktop: the configuration of papers: {}",
                e.message()
            );
            HashMap::new()
        }
    }
}

/// A paper of the library with its date: what the page gets.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Dated {
    pub id: String,
    /// ms since the epoch
    pub added: i64,
}

/// When the papers came into the library, kept in a file of the app.
pub struct DatesAdded {
    path: PathBuf,
    known: Mutex<Known>,
    /// one update at a time: each starts from what the one before left
    updating: Mutex<()>,
}

impl DatesAdded {
    /// The papers of the file; none without it, or with a broken one.
    pub fn load(path: PathBuf) -> DatesAdded {
        let known = fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        DatesAdded {
            path,
            known: Mutex::new(known),
            updating: Mutex::new(()),
        }
    }

    /// Compare the library now (`current`, taken once this update has its turn) with the file,
    /// and save what changed; whether something did. Slow the first time a large library is seen
    /// (its history is read): not on the thread of the window.
    pub fn update(
        &self,
        current: impl FnOnce() -> Current,
        history: impl FnMut(&Path) -> HashMap<String, f64>,
    ) -> bool {
        let _turn = self.updating.lock().unwrap();
        let current = current();
        let known = self.known.lock().unwrap().clone();
        let next = updated(&known, &current, seconds(SystemTime::now()), history);
        if next == known {
            return false;
        }
        self.save(&next);
        *self.known.lock().unwrap() = next;
        true
    }

    /// The papers of the library with a date, the newest first.
    pub fn dated(&self, library: &Library) -> Vec<Dated> {
        let bibs: HashMap<&Path, PathBuf> = library
            .bibs()
            .iter()
            .map(|b| (b.as_path(), real(b)))
            .collect();
        let known = self.known.lock().unwrap();
        let mut dated: Vec<(f64, &str)> = library
            .items()
            .iter()
            .filter_map(|item| {
                let bib = bibs.get(item.bib.as_path())?;
                let added = known.get(bib)?.get(&item.key)?.added?;
                Some((added, item.id.as_str()))
            })
            .collect();
        dated.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.cmp(b.1)));
        dated
            .into_iter()
            .map(|(added, id)| Dated {
                id: id.to_string(),
                added: (added * 1000.0).round() as i64,
            })
            .collect()
    }

    /// Losing the dates is no error worth more than a line in the log.
    fn save(&self, known: &Known) {
        let saved = fs::create_dir_all(self.path.parent().unwrap())
            .and_then(|()| fs::write(&self.path, serde_json::to_string_pretty(known).unwrap()));
        if let Err(e) = saved {
            eprintln!("papers-desktop: cannot save {}: {e}", self.path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The library now: its .bib files are those of the papers.
    fn library(papers: &[Paper]) -> Current {
        let mut bibs: Vec<PathBuf> = papers.iter().map(|p| p.bib.clone()).collect();
        bibs.dedup();
        Current {
            bibs,
            papers: papers.to_vec(),
        }
    }

    fn paper(bib: &str, key: &str, doi: Option<&str>, title: &str) -> Paper {
        Paper {
            bib: PathBuf::from(bib),
            key: key.to_string(),
            identity: Identity {
                doi: doi.map(String::from),
                title: Some(title.to_string()),
            },
            pdf: None,
        }
    }

    /// The dates of the keys of a .bib.
    fn dates(known: &Known, bib: &str) -> Vec<(String, Option<f64>)> {
        known[Path::new(bib)]
            .iter()
            .map(|(key, seen)| (key.clone(), seen.added))
            .collect()
    }

    fn no_history(_: &Path) -> HashMap<String, f64> {
        HashMap::new()
    }

    #[test]
    fn first_seen_by_the_history_else_the_pdf_else_older() {
        let dir = tempfile::tempdir().unwrap();
        let pdf = dir.path().join("b.pdf");
        fs::write(&pdf, "%PDF").unwrap();
        let when = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        fs::File::options()
            .write(true)
            .open(&pdf)
            .unwrap()
            .set_modified(when)
            .unwrap();
        let mut papers = vec![
            paper("/l/a.bib", "a", None, "a (2000)"),
            paper("/l/a.bib", "b", None, "b (2000)"),
            paper("/l/a.bib", "c", None, "c (2000)"),
        ];
        papers[1].pdf = Some(pdf);
        let mut asked = Vec::new();
        let known = updated(&Known::new(), &library(&papers), 2e9, |bib| {
            asked.push(bib.to_path_buf());
            HashMap::from([("a".to_string(), 1.5e9)])
        });
        assert_eq!(asked, [PathBuf::from("/l/a.bib")]);
        assert_eq!(
            dates(&known, "/l/a.bib"),
            [
                ("a".into(), Some(1.5e9)),
                ("b".into(), Some(1.7e9)),
                ("c".into(), None)
            ]
        );
        // read again with nothing new: the history is not asked
        let again = updated(&known, &library(&papers), 2e9, |_| panic!("asked"));
        assert_eq!(again, known);
    }

    #[test]
    fn later_by_the_history_else_when_seen_and_the_keys_gone_are_dropped() {
        let known = updated(
            &Known::new(),
            &library(&[paper("/l/a.bib", "a", None, "a (2000)")]),
            1e9,
            no_history,
        );
        let papers = [
            paper("/l/a.bib", "b", None, "b (2000)"),
            paper("/l/a.bib", "c", None, "c (2000)"),
        ];
        let known = updated(&known, &library(&papers), 2e9, |_| {
            HashMap::from([("b".to_string(), 1.9e9)])
        });
        assert_eq!(
            dates(&known, "/l/a.bib"),
            [("b".into(), Some(1.9e9)), ("c".into(), Some(2e9))]
        );
        // an empty .bib was seen: its first paper is a new one
        let empty = Current {
            bibs: vec![PathBuf::from("/l/b.bib")],
            papers: Vec::new(),
        };
        let known = updated(&known, &empty, 3e9, no_history);
        assert!(known[Path::new("/l/b.bib")].is_empty());
        let known = updated(
            &known,
            &library(&[paper("/l/b.bib", "b", None, "b (2000)")]),
            4e9,
            no_history,
        );
        assert_eq!(dates(&known, "/l/b.bib"), [("b".into(), Some(4e9))]);
    }

    #[test]
    fn a_new_key_of_the_same_paper_keeps_the_date() {
        let papers = [
            paper("/l/a.bib", "old", Some("10.1/x"), "x (2000)"),
            paper("/l/a.bib", "other", None, "y (2001)"),
        ];
        let known = updated(&Known::new(), &library(&papers), 1e9, |_| {
            HashMap::from([("old".to_string(), 5e8), ("other".to_string(), 6e8)])
        });
        // renamed, and its title changed: the DOI tells it; the other by its title and year
        let papers = [
            paper("/l/a.bib", "new", Some("10.1/x"), "x revised (2000)"),
            paper("/l/a.bib", "other2001", None, "y (2001)"),
        ];
        let known = updated(&known, &library(&papers), 2e9, no_history);
        assert_eq!(
            dates(&known, "/l/a.bib"),
            [("new".into(), Some(5e8)), ("other2001".into(), Some(6e8))]
        );
        assert_eq!(
            known[Path::new("/l/a.bib")]["new"]
                .identity
                .title
                .as_deref(),
            Some("x revised (2000)")
        );
    }

    #[test]
    fn each_bib_on_its_own_and_one_no_longer_read_is_kept() {
        let both = [
            paper("/l/a.bib", "a", None, "a (2000)"),
            paper("/l/b.bib", "a", None, "a (2000)"),
        ];
        let known = updated(&Known::new(), &library(&both), 1e9, no_history);
        // c.bib alone in the library now: first seen, its keys older; a.bib is kept as it was
        let known = updated(
            &known,
            &library(&[paper("/l/c.bib", "c", None, "c (2000)")]),
            2e9,
            no_history,
        );
        assert_eq!(dates(&known, "/l/a.bib"), [("a".into(), None)]);
        assert_eq!(dates(&known, "/l/c.bib"), [("c".into(), None)]);
        let known = updated(
            &known,
            &library(&[paper("/l/c.bib", "d", None, "d (2000)")]),
            3e9,
            no_history,
        );
        assert_eq!(dates(&known, "/l/c.bib"), [("d".into(), Some(3e9))]);
    }

    #[test]
    fn kept_in_a_file_and_listed_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let bib = dir.path().join("refs.bib");
        fs::write(
            &bib,
            "@article{a,\n title = {A}\n}\n\n@article{b,\n title = {B}\n}\n\n@article{c,\n title = {C}\n}\n",
        )
        .unwrap();
        let library = Library::load(std::slice::from_ref(&bib)).unwrap();
        let path = dir.path().join("data/added.json");
        let dates = DatesAdded::load(path.clone());
        let history = |_: &Path| HashMap::from([("a".to_string(), 1e9), ("c".to_string(), 2e9)]);
        assert!(dates.update(|| current(&library), history));
        assert!(!dates.update(|| current(&library), history));
        let ids = |d: &DatesAdded| {
            d.dated(&library)
                .into_iter()
                .map(|d| (d.id, d.added))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            ids(&dates),
            [
                ("c".to_string(), 2_000_000_000_000),
                ("a".to_string(), 1_000_000_000_000)
            ]
        );
        // read again from the file, by the real path of the .bib
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains(&*real(&bib).to_string_lossy()), "{text}");
        assert_eq!(ids(&DatesAdded::load(path)), ids(&dates));
    }

    #[test]
    fn the_history_of_a_papers_library_dates_its_entries() {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let mut values = serde_json::Map::new();
        values.insert(
            "history-dir".into(),
            serde_json::json!(root.join("history").to_string_lossy()),
        );
        let machine = MachineConfig::new(None, values);
        let lib = root.join("lib");
        fs::create_dir_all(lib.join(".papers")).unwrap();
        fs::write(
            lib.join(".papers/config.json"),
            "{\"bibtex\": \"papers.bib\", \"filesdir\": \"files\", \"version\": 3}",
        )
        .unwrap();
        let bib = lib.join("papers.bib");
        fs::write(&bib, "@article{old,\n title = {Old}\n}\n").unwrap();
        // a command of papers: the library before it, then the entry it adds
        let mut config = find_config(&lib.to_string_lossy(), machine.clone()).unwrap();
        let before = History::before_command(&mut config);
        let mut history = History::open(&mut config, true, false).unwrap().unwrap();
        history.sync_head(before.as_ref()).unwrap();
        fs::write(
            &bib,
            "@article{new,\n title = {New}\n}\n\n@article{old,\n title = {Old}\n}\n",
        )
        .unwrap();
        let step = history
            .record(papers_rs::history::Step::new("papers add new.bib"))
            .unwrap();
        let time = history
            .project
            .state(step.number.as_ref().unwrap())
            .unwrap()
            .time();

        let times = history_times(&bib, machine.clone());
        assert_eq!(times, HashMap::from([("new".to_string(), time)]));
        // a .bib with no history
        let plain = root.join("plain.bib");
        fs::write(&plain, "@article{x,\n title = {X}\n}\n").unwrap();
        assert!(history_times(&plain, machine).is_empty());
    }
}
