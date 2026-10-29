//! The merge of entries that are the same paper (`papers.merge`): the target
//! (the first entry, it keeps its key and its place) with the others, the
//! fields that are the same, those that are added, and the conflicts, and what
//! `--on-duplicate` does with them; two-way (`add`, `check`) or three-way with
//! a base (`pull`, `push`, `history --revert`).
//!
//! The dialogue (`--on-duplicate ask`: the screen, the answers) is the business of
//! the program that asks; it works on a [`Merge`] (choose the values of one
//! entry for all conflicts or for one field, change the target) and gives a
//! [`Decision`].
//!
//! # The three-way merge
//!
//! `pull`, `push` and `history --revert` (and the revert of a pool step by undo) merge
//! two versions of an entry with the version they both come from, the **base**
//! ([`Merge::exchange`]): what one side changed since the base wins, what both changed
//! is a conflict, an equivalent change (case, braces) of the incoming entry wins only when the
//! target has the base exactly, a removal counts as a change. The two sides are the **target** (entry 1,
//! it keeps its key and its place) and the **incoming** entry (entry 2); the base is the
//! origin record of the entry (`pull`: the target is the library entry, the incoming the pool
//! entry; `push`: the other way round, with the base under the pool key), or, for a revert, the
//! entry as step N left it, with the target the entry now and the incoming the entry before
//! step N. Without a base (a first exchange) the merge is two-way, and what one side has
//! alone is an addition.
//!
//! In an exchange the key, the former keys (`ids`) and the links (`file`) are not merged as
//! fields: each side keeps its own keys, the links are the union ([`Merge::new_files`] are
//! those that the target did not have).
//!
//! Each field has a [`Status`] (` ` same, `+` an addition from the incoming entry, `!` a
//! conflict) and, in a three-way merge, where it [`Changed`]. Additions are taken in every mode
//! (an addition may be [declined](FieldMerge::decline), field by field in the dialogue); a
//! removal in the incoming entry is an addition whose value is none
//! ([`FieldMerge::removal`]). A conflict is decided by whoever resolves it
//! ([`Resolve::decide`], with the [`Context`] `Pull`, `Push` or `Revert`):
//!
//! | mode | conflicts |
//! |---|---|
//! | `keep` | the value of entry 1: the library's in `pull`, **the pool's in `push`** (entry 1 is the pool entry), the entry now in a revert |
//! | `update` | the value of entry 2: the pool's in `pull`, **the library's in `push`**, the entry before step N in a revert |
//! | `skip` | [`Decision::Skip`]: the caller changes nothing for this entry (`skipped`) |
//! | `error` | [`DuplicateError`]: the command stops before saving |
//! | `ask` | the dialogue of the program (`papers-rs-cli`'s `Dialogue`) |
//!
//! (`exchange.md` section 5.6 says the same in words: `keep` and `update` name entries 1 and
//! 2, so the sides swap between `pull` and `push`.) Whoever applies the decision then calls
//! [`apply_entry`] with the merged entry, or [`Merge::result`] when there was no conflict.
//!
//! ```
//! use papers_rs::bib::Entry;
//! use papers_rs::merge::{Changed, Merge, Status};
//!
//! let entry = |key: &str, fields: &[(&str, &str)]| {
//!     let mut e = Entry::new("article", key);
//!     for (k, v) in fields {
//!         e.set(k, *v);
//!     }
//!     e
//! };
//! let base = entry("smith2020", &[("title", "T"), ("journal", "Nat."), ("volume", "1")]);
//! let library = entry("smith2020", &[("title", "T"), ("journal", "Nature"), ("volume", "1")]);
//! let pool = entry("Smith_2020", &[("title", "T"), ("journal", "Science"), ("volume", "2")]);
//!
//! let mut merge = Merge::exchange(library, pool, None, Some(&base));
//! let status = |m: &Merge, n: &str| {
//!     let f = m.fields.iter().find(|f| f.name == n).unwrap();
//!     (f.status, f.changed)
//! };
//! assert_eq!(status(&merge, "volume"), (Status::Added, Some(Changed::Other)));
//! assert_eq!(status(&merge, "journal"), (Status::Conflict, Some(Changed::Both)));
//! assert_eq!(merge.result().get("journal"), Some("Nature")); // until decided
//! merge.choose_all(1);
//! let merged = merge.result();
//! assert_eq!((merged.key.as_str(), merged.get("journal"), merged.get("volume")), ("smith2020", Some("Science"), Some("2")));
//! ```
//!
//! A two-way merge (`add`, `check`):
//!
//! ```
//! use papers_rs::bib::Entry;
//! use papers_rs::merge::Merge;
//!
//! let mut a = Entry::new("article", "smith2020");
//! a.set("author", "Smith, J.");
//! a.set("journal", "Nature");
//! let mut b = Entry::new("article", "Smith_2020");
//! b.set("author", "Smith, John");
//! b.set("journal", "Science");
//! b.set("volume", "10");
//!
//! let mut merge = Merge::new(&[a, b], None, false);
//! assert_eq!(merge.conflicts().len(), 1); // the journal; the longer name is kept
//! let kept = merge.result();
//! assert_eq!((kept.key.as_str(), kept.get("journal"), kept.get("volume")), ("smith2020", Some("Nature"), Some("10")));
//! assert_eq!(kept.get("author"), Some("Smith, John"));
//! merge.choose_all(1);
//! assert_eq!(merge.result().get("journal"), Some("Science"));
//! ```

use std::collections::HashSet;
use std::fmt;

use crate::bib::{Entry, Field};
use crate::config::paths::{isfile, realpath};
use crate::entry::{files, format_files, val};
use crate::identity::{Kind, Verdict, normalize_doi, normalize_text, parse_name, split_names};
use crate::log;

/// The state of a field in a merge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// the same on all sides (` `)
    Same,
    /// added from another entry (`+`)
    Added,
    /// the values differ (`!`)
    Conflict,
}

/// The fields with a rule of their own: the key, the links and the former keys.
const SPECIAL_FIELDS: [&str; 3] = ["ID", "file", "ids"];

/// How the dialogue shows a field that a side removed (three-way).
pub const REMOVED: &str = "(removed)";

/// The name that stands for the type of the entry among the fields.
pub const ENTRYTYPE: &str = "ENTRYTYPE";

fn given_tokens(given: &str) -> Vec<String> {
    normalize_text(given)
        .split(|c: char| crate::bib::pystr::is_space(c) || matches!(c, '.' | '-' | '~'))
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

/// John / J. / J. D. / John David: each word equal, or an initial of the
/// other (the words are compared two by two, up to the shorter list).
fn given_compatible(g1: &str, g2: &str) -> bool {
    given_tokens(g1)
        .iter()
        .zip(given_tokens(g2).iter())
        .all(|(x, y)| {
            x == y
                || (x.chars().count() == 1 && y.starts_with(x.as_str()))
                || (y.chars().count() == 1 && x.starts_with(y.as_str()))
        })
}

/// Two author lists that only differ by initials against full first names, or
/// by the order of the name (`names_compatible`).
///
/// ```
/// use papers_rs::merge::names_compatible;
/// assert!(names_compatible("Smith, John D.", "J. Smith"));
/// assert!(!names_compatible("Smith, John", "Smith, Jane"));
/// ```
pub fn names_compatible(field1: &str, field2: &str) -> bool {
    let (n1, n2) = (split_names(field1), split_names(field2));
    n1.len() == n2.len()
        && n1.iter().zip(&n2).all(|(a, b)| {
            let ((fa, ga), (fb, gb)) = (parse_name(a), parse_name(b));
            normalize_text(&fa) == normalize_text(&fb) && given_compatible(&ga, &gb)
        })
}

/// The sum of the lengths of the given-name words of a field.
fn fullness(field: &str) -> usize {
    split_names(field)
        .iter()
        .flat_map(|n| given_tokens(&parse_name(n).1))
        .map(|t| t.chars().count())
        .sum()
}

/// Whether two values of a field say the same: names by [`names_compatible`],
/// DOIs by their normal form, the other fields equal after
/// [`normalize_text`] (case, braces, accents, LaTeX, spaces).
pub fn equivalent(name: &str, v1: &str, v2: &str) -> bool {
    if name == "author" || name == "editor" {
        return names_compatible(v1, v2);
    }
    if name == "doi"
        && let (Some(d1), Some(d2)) = (normalize_doi(v1), normalize_doi(v2))
    {
        return d1 == d2;
    }
    normalize_text(v1) == normalize_text(v2)
}

/// The value kept among equivalent values: the first, or for names the one
/// with the longest given names (the first on a tie).
fn preferred<'a>(name: &str, values: &[&'a str]) -> &'a str {
    if name == "author" || name == "editor" {
        let mut best = values[0];
        for v in &values[1..] {
            if fullness(v) > fullness(best) {
                best = v;
            }
        }
        return best;
    }
    values[0]
}

/// An empty or blank value is no value.
fn no_empty(v: Option<&str>) -> Option<String> {
    v.filter(|v| !crate::bib::pystr::strip(v).is_empty())
        .map(str::to_string)
}

/// Where a field changed since the base (a three-way merge).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Changed {
    /// in the target only (it is kept)
    Target,
    /// in the incoming entry only (it is offered)
    Other,
    /// on both sides (a conflict)
    Both,
}

/// One field of a merge: the value of each entry (none if absent), its
/// status, and the entry whose value is used.
///
/// In a three-way merge (`three_way`), `base` is the value of the base, and
/// `changed` says on which side the field changed since.
#[derive(Debug, Clone)]
pub struct FieldMerge {
    pub name: String,
    /// the value of each entry; an empty value counts as absent
    pub values: Vec<Option<String>>,
    pub status: Status,
    /// the entry whose value is used (for `Same` and `Added`, the value is set directly)
    pub choice: usize,
    /// the value of the base (three-way)
    pub base: Option<String>,
    pub three_way: bool,
    /// where the field changed since the base (three-way, not for what is the same)
    pub changed: Option<Changed>,
    value: Option<String>,
}

impl FieldMerge {
    /// The field of entries that have these values (`FieldMerge(name, values)`).
    pub fn new(name: &str, original: &[Option<&str>]) -> FieldMerge {
        FieldMerge::build(name, original, None, false)
    }

    /// The field of two entries and their base (`FieldMerge(name, values, base=, three_way=True)`):
    /// whatever one side changed since the base wins, a field changed on both sides is a conflict.
    pub fn three_way(name: &str, original: &[Option<&str>], base: Option<&str>) -> FieldMerge {
        FieldMerge::build(name, original, base, true)
    }

    fn build(
        name: &str,
        original: &[Option<&str>],
        base: Option<&str>,
        three_way: bool,
    ) -> FieldMerge {
        let values: Vec<Option<String>> = original.iter().map(|v| no_empty(*v)).collect();
        let present: Vec<&str> = values.iter().flatten().map(String::as_str).collect();
        let mut field = FieldMerge {
            name: name.to_string(),
            values: values.clone(),
            status: Status::Same,
            choice: 0,
            base: no_empty(base),
            three_way,
            changed: None,
            value: None,
        };
        if present.is_empty() {
            // the target's value as written (possibly empty): an empty field of the target is
            // kept, one of another entry is not added
            field.value = original[0].map(str::to_string);
            return field;
        }
        field.choice = if values[0].is_some() {
            0
        } else {
            values.iter().position(Option::is_some).unwrap()
        };
        if three_way
            && field.base.is_some()
            && values[0] == field.base
            && values.get(1).is_some_and(|v| {
                v.is_some()
                    && *v != values[0]
                    && equivalent(
                        name,
                        values[0].as_deref().unwrap_or(""),
                        v.as_deref().unwrap(),
                    )
            })
        {
            // an equivalent change of the other side only (case, braces): offered
            field.status = Status::Added;
            field.value = values[1].clone();
            field.choice = 1;
            field.changed = Some(Changed::Other);
        } else if present[1..].iter().all(|v| equivalent(name, present[0], v))
            && !(three_way && values.iter().any(Option::is_none))
        {
            field.value = Some(preferred(name, &present).to_string());
            field.status = if values[0].is_some() {
                Status::Same
            } else {
                Status::Added
            };
        } else if three_way {
            field.decide_three_way();
        } else {
            field.status = Status::Conflict;
        }
        field
    }

    /// Target and incoming entry differ (one of them may be absent: added, or removed since the base).
    fn decide_three_way(&mut self) {
        let (target, other) = (self.values[0].clone(), self.values[1].clone());
        let base = self.base.clone();
        let same = |a: &Option<String>, b: &Option<String>| match (a, b) {
            (Some(a), Some(b)) => equivalent(&self.name, a, b),
            _ => false,
        };
        self.choice = 0;
        if base.is_none() && other.is_none() {
            // added in the target: kept
            self.status = Status::Same;
            self.changed = Some(Changed::Target);
        } else if base.is_none() && target.is_none() {
            // added in the incoming entry: offered
            self.status = Status::Added;
            self.choice = 1;
            self.changed = Some(Changed::Other);
        } else if same(&other, &base) {
            // changed or removed in the target only: kept
            self.status = Status::Same;
            self.changed = Some(Changed::Target);
        } else if same(&target, &base) {
            // changed or removed in the incoming entry only: offered
            self.status = Status::Added;
            self.value = other;
            self.choice = 1;
            self.changed = Some(Changed::Other);
        } else {
            // changed on both sides, or removed on one side and changed on the other
            self.status = Status::Conflict;
            self.changed = Some(Changed::Both);
        }
    }

    /// The value used.
    pub fn value(&self) -> Option<&str> {
        match &self.value {
            Some(v) => Some(v),
            None => self.values[self.choice].as_deref(),
        }
    }

    /// The change offered is the removal of the field (three-way).
    pub fn removal(&self) -> bool {
        self.status == Status::Added && self.values[self.choice].is_none()
    }

    /// Do not take an addition: keep the value of the target (none if it has none).
    pub fn decline(&mut self) {
        if self.status == Status::Added {
            self.status = Status::Same;
            self.value = None;
            self.choice = 0;
        }
    }

    /// Use the value of entry `i`, for a conflict (ignored when entry `i` does not have the
    /// field, except in a three-way merge, where entry `i` removed it).
    pub fn choose(&mut self, i: usize) {
        if self.status == Status::Conflict
            && self
                .values
                .get(i)
                .is_some_and(|v| v.is_some() || self.three_way)
        {
            self.choice = i;
        }
    }

    /// The values not used, grouped by value, in entry order: the entries and
    /// the value (`alternatives`); none for a field that the entry removed (three-way).
    pub fn alternatives(&self) -> Vec<(Vec<usize>, Option<String>)> {
        let mut groups: Vec<(Vec<usize>, Option<String>)> = Vec::new();
        let used = self.value();
        for (i, v) in self.values.iter().enumerate() {
            if i == self.choice {
                continue;
            }
            match v {
                None if !self.three_way || used.is_none() => continue,
                Some(v) if used.is_some_and(|u| equivalent(&self.name, v, u)) => continue,
                _ => {}
            }
            let same = |g: &Option<String>| match (g, v) {
                (None, None) => true,
                (Some(g), Some(v)) => equivalent(&self.name, g, v),
                _ => false,
            };
            match groups.iter_mut().find(|(_, g)| same(g)) {
                Some((idx, _)) => idx.push(i),
                None => groups.push((vec![i], v.clone())),
            }
        }
        groups
    }
}

/// The value of a field of an entry for a merge: none if absent, the type in lower case.
fn merge_value(e: &Entry, name: &str) -> Option<String> {
    if name == ENTRYTYPE {
        return Some(e.entry_type.to_lowercase());
    }
    e.get(name).map(str::to_string)
}

/// The merge of entries that are the same paper: `entries[0]` is the target.
///
/// `relative_to` is the directory the file links are relative to (the
/// bibtex directory), `keys_to_ids` puts the keys of the other entries in the
/// `ids` of the result (`check --duplicates`). A merge of an exchange
/// ([`Merge::exchange`]) has two entries and maybe a base.
#[derive(Debug, Clone)]
pub struct Merge {
    pub entries: Vec<Entry>,
    pub relative_to: Option<String>,
    pub keys_to_ids: bool,
    /// the version the two entries come from (a three-way merge: the origin record of the entry
    /// of a pull or push, the entry as step N left it for a revert); its key and links are not used
    pub base: Option<Entry>,
    /// a merge of an exchange (`pull`, `push`, a revert): the former keys of the target only,
    /// the links of both sides
    pub exchange: bool,
    /// the fields but the key, the links and the former keys: the type first
    pub fields: Vec<FieldMerge>,
    /// the union of the links of the entries, as absolute paths (the target's first)
    pub files: Vec<String>,
    /// those that the target does not have
    pub new_files: Vec<String>,
    /// the former keys of the result
    pub ids: Vec<String>,
    /// those that the target does not have
    pub ids_added: Vec<String>,
}

impl Merge {
    /// The two-way merge of entries that are the same paper (`add`, `check`).
    pub fn new(entries: &[Entry], relative_to: Option<&str>, keys_to_ids: bool) -> Merge {
        Merge::build(entries, relative_to, keys_to_ids, None, false)
    }

    /// The merge of an exchange: the `target` (it keeps its key) with the `incoming` entry, and
    /// with a `base`, three-way (see the module); without, two-way with the rules of an exchange
    /// (the former keys of the target only; what the incoming entry has alone is an addition).
    /// `pull`: the library entry, the pool entry, the origin record; `push`: the pool entry, the
    /// library entry, the record under the pool key; a revert: the entry now, the entry before
    /// step N, the entry after it.
    pub fn exchange(
        target: Entry,
        incoming: Entry,
        relative_to: Option<&str>,
        base: Option<&Entry>,
    ) -> Merge {
        Merge::build(&[target, incoming], relative_to, false, base.cloned(), true)
    }

    fn build(
        entries: &[Entry],
        relative_to: Option<&str>,
        keys_to_ids: bool,
        base: Option<Entry>,
        exchange: bool,
    ) -> Merge {
        let mut merge = Merge {
            entries: entries.to_vec(),
            relative_to: relative_to.map(str::to_string),
            keys_to_ids,
            exchange: exchange || base.is_some(),
            base,
            fields: Vec::new(),
            files: Vec::new(),
            new_files: Vec::new(),
            ids: Vec::new(),
            ids_added: Vec::new(),
        };
        let mut names: Vec<&str> = vec![ENTRYTYPE];
        for e in entries {
            for f in &e.fields {
                if !SPECIAL_FIELDS.contains(&f.key.as_str()) && !names.contains(&f.key.as_str()) {
                    names.push(&f.key);
                }
            }
        }
        for name in names {
            let values: Vec<Option<String>> =
                entries.iter().map(|e| merge_value(e, name)).collect();
            let refs: Vec<Option<&str>> = values.iter().map(Option::as_deref).collect();
            merge.fields.push(match &merge.base {
                Some(base) => {
                    FieldMerge::three_way(name, &refs, merge_value(base, name).as_deref())
                }
                None => FieldMerge::new(name, &refs),
            });
        }
        merge.merge_files();
        merge.merge_ids();
        merge
    }

    /// A base was given: whatever one side changed since it wins.
    pub fn is_three_way(&self) -> bool {
        self.base.is_some()
    }

    pub fn target(&self) -> &Entry {
        &self.entries[0]
    }

    pub fn key(&self) -> &str {
        &self.entries[0].key
    }

    pub fn others(&self) -> &[Entry] {
        &self.entries[1..]
    }

    fn files_of(&self, e: &Entry) -> Vec<String> {
        files(e, self.relative_to.as_deref())
    }

    /// The union of the links: the same file (real path, or content) once.
    fn merge_files(&mut self) {
        let mut merged: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for e in &self.entries {
            for f in files(e, self.relative_to.as_deref()) {
                let mut ids = vec![realpath(&f)];
                if isfile(&f) {
                    ids.push(format!("sha256:{}", checksum_hex(&f).unwrap_or_default()));
                }
                if !ids.iter().any(|i| seen.contains(i)) {
                    merged.push(f);
                }
                seen.extend(ids);
            }
        }
        let own: HashSet<String> = self
            .files_of(self.target())
            .iter()
            .map(|f| realpath(f))
            .collect();
        self.new_files = merged
            .iter()
            .filter(|f| !own.contains(&realpath(f)))
            .cloned()
            .collect();
        self.files = merged;
    }

    fn merge_ids(&mut self) {
        let key = self.key().to_lowercase();
        // the former keys of a pool mean nothing in a library, and conversely
        let owners = if self.exchange { 1 } else { self.entries.len() };
        let mut sources: Vec<String> = self.entries[..owners]
            .iter()
            .map(|e| val(e, "ids").to_string())
            .collect();
        if self.keys_to_ids {
            sources.extend(self.others().iter().map(|e| e.key.clone()));
        }
        let mut ids: Vec<String> = Vec::new();
        for source in &sources {
            for i in source.split(',') {
                let i = crate::bib::pystr::strip(i);
                if !i.is_empty()
                    && i.to_lowercase() != key
                    && !ids.iter().any(|x| x.to_lowercase() == i.to_lowercase())
                {
                    ids.push(i.to_string());
                }
            }
        }
        let own: Vec<String> = val(self.target(), "ids")
            .split(',')
            .map(|x| crate::bib::pystr::strip(x).to_lowercase())
            .collect();
        self.ids_added = ids
            .iter()
            .filter(|i| !own.contains(&i.to_lowercase()))
            .cloned()
            .collect();
        self.ids = ids;
    }

    pub fn conflicts(&self) -> Vec<&FieldMerge> {
        self.fields
            .iter()
            .filter(|f| f.status == Status::Conflict)
            .collect()
    }

    pub fn additions(&self) -> Vec<&FieldMerge> {
        self.fields
            .iter()
            .filter(|f| f.status == Status::Added)
            .collect()
    }

    /// Nothing to decide: no conflict and no field to add (the links and the former keys aside).
    pub fn is_exact(&self) -> bool {
        self.conflicts().is_empty() && self.additions().is_empty()
    }

    /// Resolve every conflict with the value of entry `i`, where it has one.
    pub fn choose_all(&mut self, i: usize) {
        for f in self
            .fields
            .iter_mut()
            .filter(|f| f.status == Status::Conflict)
        {
            f.choose(i);
        }
    }

    /// The same merge with entry `i` as the target.
    pub fn with_target(&self, i: usize) -> Merge {
        let mut entries = vec![self.entries[i].clone()];
        entries.extend(
            self.entries
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, e)| e.clone()),
        );
        Merge::build(
            &entries,
            self.relative_to.as_deref(),
            self.keys_to_ids,
            self.base.clone(),
            self.exchange,
        )
    }

    /// The field of the merged value of `f`: a bare value (`month = jan`, the
    /// name of a `@string`) stays bare when it is the value of one of the
    /// entries; the entry whose value was chosen is looked at first, then the
    /// entries in order, and the first that has the field with that value decides.
    fn field(&self, f: &FieldMerge, value: &str) -> Field {
        let mut field = Field::new(&f.name, value);
        let chosen = self.entries.get(f.choice);
        for e in chosen.into_iter().chain(&self.entries) {
            if let Some(same) = e.fields.iter().rev().find(|x| x.key == f.name)
                && same.value == value
            {
                field.verbatim = same.verbatim;
                break;
            }
        }
        field
    }

    /// The merged entry, with the key of the target: the fields that have a
    /// value (a value that is bare in an entry stays bare, the others are in
    /// braces), the links and the former keys.
    pub fn result(&self) -> Entry {
        let mut fields: Vec<Field> = self
            .fields
            .iter()
            .filter(|f| f.name != ENTRYTYPE)
            .filter_map(|f| f.value().map(|v| self.field(f, v)))
            .collect();
        if !self.files.is_empty() {
            fields.push(Field::new(
                "file",
                format_files(&self.files, self.relative_to.as_deref()),
            ));
        }
        if !self.ids.is_empty() {
            fields.push(Field::new("ids", self.ids.join(", ")));
        }
        let entry_type = self
            .fields
            .iter()
            .find(|f| f.name == ENTRYTYPE)
            .and_then(|f| f.value())
            .map_or_else(|| self.target().entry_type.clone(), str::to_string);
        let mut entry = Entry::new(entry_type, self.key());
        entry.fields = fields;
        entry
    }
}

/// The sha256 of a file, as hex text.
pub fn checksum_hex(path: &str) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 1 << 16];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

// what to do with a duplicate
// ---------------------------

/// The `--on-duplicate` modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnDuplicate {
    /// the dialogue
    Ask,
    /// merge, a conflict keeps the value of the target
    Keep,
    /// merge, a conflict takes the value of the new entry
    Update,
    /// change nothing
    Skip,
    /// stop before saving
    Error,
}

impl OnDuplicate {
    pub fn parse(mode: &str) -> Option<OnDuplicate> {
        Some(match mode {
            "ask" => OnDuplicate::Ask,
            "keep" => OnDuplicate::Keep,
            "update" => OnDuplicate::Update,
            "skip" => OnDuplicate::Skip,
            "error" => OnDuplicate::Error,
            _ => return None,
        })
    }
}

/// What a decision is for: `add` (entry 1 is the library entry, entry 2 the added one),
/// `check --duplicates` (a group of the library), `pull` (entry 1 is the library entry, entry 2
/// the pool entry), `push` (entry 1 is the pool entry, entry 2 the library entry) or
/// `history --revert` (entry 1 is the entry now, entry 2 the entry before the step).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Context {
    Add,
    Check,
    Pull,
    Push,
    Revert,
}

impl Context {
    /// The command to ask for help on, in the hint of the error.
    fn word(self) -> &'static str {
        match self {
            Context::Add => "add",
            Context::Check => "check",
            Context::Pull => "pull",
            Context::Push => "push",
            Context::Revert => "history",
        }
    }

    /// `pull` or `push`: the exchange of a library with its pool.
    pub fn is_exchange(self) -> bool {
        matches!(self, Context::Pull | Context::Push)
    }

    /// The entries have fixed names on the screen (`library`/`new`, `library`/`pool`,
    /// `pool`/`library`); otherwise they are named by their keys.
    pub fn labels(self) -> Option<[&'static str; 2]> {
        match self {
            Context::Add => Some(["library", "new"]),
            Context::Pull => Some(["library", "pool"]),
            Context::Push => Some(["pool", "library"]),
            Context::Check | Context::Revert => None,
        }
    }
}

/// Make the entry of a library equal to `entry` (`apply_entry`): its type, its key and its
/// fields (the merged entry of a decision).
pub fn apply_entry(target: &mut Entry, entry: &Entry) {
    target.entry_type = entry.entry_type.clone();
    target.key = entry.key.clone();
    target.fields = entry.fields.clone();
}

/// The outcome of a duplicate decision.
#[derive(Debug, Clone)]
pub enum Decision {
    /// merge: the merge decided (its target may have changed), and the merged entry
    Merge(Box<Merge>, Entry),
    /// not the same paper: the pair goes to `distinct.json`
    Distinct,
    /// change nothing
    Skip,
}

/// A duplicate with `--on-duplicate error`: the command stops before saving.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateError(pub String);

impl fmt::Display for DuplicateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DuplicateError {}

impl From<DuplicateError> for crate::error::Error {
    fn from(e: DuplicateError) -> Self {
        crate::error::Error::exit(e.0)
    }
}

/// Whoever decides what to do with a duplicate: [`Resolver`] for the modes
/// without a question, a dialogue for `ask`.
pub trait Resolve {
    /// The `--on-duplicate` mode: `check` does nothing with `skip`, and stops at the first group with `error`.
    fn mode(&self) -> OnDuplicate;

    /// The user quit the dialogue: the remaining duplicates are skipped.
    fn quit(&self) -> bool {
        false
    }

    fn decide(
        &mut self,
        merge: Merge,
        context: Context,
        verdict: &Verdict,
    ) -> Result<Decision, DuplicateError>;
}

/// The `--on-duplicate` mode of a command, and what was answered so far (after
/// `quit`, the remaining duplicates are skipped).
#[derive(Debug, Clone)]
pub struct Resolver {
    pub mode: OnDuplicate,
    pub quit: bool,
}

impl Resolver {
    pub fn new(mode: OnDuplicate) -> Resolver {
        Resolver { mode, quit: false }
    }

    /// What the merge says in the error (`describe`).
    pub fn describe(merge: &Merge, context: Context, verdict: &Verdict) -> String {
        let n = merge.conflicts().len();
        let what = if n > 0 || merge.additions().is_empty() {
            format!("{n} conflict{}", if n != 1 { "s" } else { "" })
        } else {
            "fields to add".to_string()
        };
        match context {
            Context::Add => {
                format!(
                    "{}: already in library as {} ({}; {what})",
                    merge.others()[0].key,
                    merge.key(),
                    verdict.reason
                )
            }
            Context::Pull => format!(
                "{}: changed in the library and in {} ({}; {what})",
                merge.key(),
                verdict.reason,
                merge.others()[0].key
            ),
            Context::Revert => format!(
                "{}: changed since {} too ({what})",
                merge.key(),
                verdict.reason
            ),
            Context::Push => format!(
                "{}: changed in the library and in {} ({}; {what})",
                merge.others()[0].key,
                verdict.reason,
                merge.key()
            ),
            Context::Check => {
                let keys: Vec<&str> = merge.entries.iter().map(|e| e.key.as_str()).collect();
                format!(
                    "same paper: {} ({}; {what})",
                    keys.join(", "),
                    verdict.reason
                )
            }
        }
    }

    /// The hint at the end of the error.
    pub fn option_hint(context: Context) -> String {
        let words = if context == Context::Check {
            "ask, keep, skip"
        } else {
            "ask, keep, update, skip"
        };
        format!(
            "Pass --on-duplicate with one of: {words} (ask is the default in a terminal), see `papers {} --help`.",
            context.word()
        )
    }
}

impl Resolve for Resolver {
    fn mode(&self) -> OnDuplicate {
        self.mode
    }

    fn quit(&self) -> bool {
        self.quit
    }

    fn decide(
        &mut self,
        mut merge: Merge,
        context: Context,
        verdict: &Verdict,
    ) -> Result<Decision, DuplicateError> {
        if self.quit {
            return Ok(Decision::Skip);
        }
        match self.mode {
            OnDuplicate::Error => Err(DuplicateError(format!(
                "{}\n{}",
                Resolver::describe(&merge, context, verdict),
                Resolver::option_hint(context)
            ))),
            OnDuplicate::Skip => Ok(Decision::Skip),
            _ if verdict.kind == Kind::Ask && self.mode != OnDuplicate::Ask => {
                log::warning(&format!(
                    "{}: not merged, decide in the dialogue (--on-duplicate ask)",
                    Resolver::describe(&merge, context, verdict)
                ));
                Ok(Decision::Skip)
            }
            OnDuplicate::Ask => Err(DuplicateError(
                "--on-duplicate ask needs a dialogue".to_string(),
            )),
            mode => {
                merge.choose_all(if mode == OnDuplicate::Keep { 0 } else { 1 });
                let entry = merge.result();
                Ok(Decision::Merge(Box::new(merge), entry))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(key: &str, fields: &[(&str, &str)]) -> Entry {
        let mut e = Entry::new("article", key);
        for (k, v) in fields {
            e.set(k, *v);
        }
        e
    }

    const SMITH: [(&str, &str); 3] = [
        ("author", "Smith, John and Doe, Jane"),
        ("title", "Climate sensitivity revisited"),
        ("year", "2020"),
    ];

    fn with(more: &[(&'static str, &'static str)]) -> Vec<(&'static str, &'static str)> {
        SMITH.iter().chain(more).copied().collect()
    }

    #[test]
    fn names() {
        assert!(names_compatible(
            "Smith, John and Doe, Jane",
            "Smith, J. and Doe, J."
        ));
        assert!(names_compatible("Perrette, M.", "M. Perrette"));
        assert!(!names_compatible("Smith, J.", "Jones, J."));
        assert!(!names_compatible("Smith, J.", "Smith, J. and Doe, J."));
        let (full, short) = ("Smith, John and Doe, Jane", "Smith, J. and Doe, J.");
        for (a, b) in [(short, full), (full, short)] {
            let m = Merge::new(
                &[
                    entry("a", &[("author", a), ("title", "T")]),
                    entry("b", &[("author", b), ("title", "T")]),
                ],
                None,
                false,
            );
            assert!(m.conflicts().is_empty());
            assert_eq!(m.result().get("author"), Some(full));
        }
    }

    #[test]
    fn model() {
        let lib = entry(
            "smith2020climate",
            &with(&[
                ("doi", "10.1000/abc123"),
                ("journal", "Nature"),
                ("keywords", "climate, mine"),
                ("file", ":/tmp/files/smith.pdf:pdf"),
            ]),
        );
        let new = entry(
            "Smith_2020",
            &[
                ("doi", "10.1000/ABC123"),
                ("journal", "Nature Climate Change"),
                ("volume", "10"),
                ("file", ":/tmp/other/smith-preprint.pdf:pdf"),
                ("author", "Smith, J. and Doe, J."),
                ("title", "Climate Sensitivity Revisited"),
                ("year", "2020"),
            ],
        );
        let mut m = Merge::new(&[lib, new], Some("/"), false);
        let status = |m: &Merge, n: &str| m.fields.iter().find(|f| f.name == n).unwrap().status;
        assert_eq!(status(&m, "journal"), Status::Conflict);
        assert_eq!(status(&m, "volume"), Status::Added);
        for n in ["doi", "title", "author", "year", "keywords"] {
            assert_eq!(status(&m, n), Status::Same, "{n}");
        }
        assert!(!m.is_exact());
        let r = m.result();
        assert_eq!(r.key, "smith2020climate");
        assert_eq!(r.get("doi"), Some("10.1000/abc123"));
        assert_eq!(r.get("title"), Some("Climate sensitivity revisited"));
        assert_eq!(r.get("journal"), Some("Nature"));
        assert_eq!(r.get("volume"), Some("10"));
        assert_eq!(
            r.get("file"),
            Some(":/tmp/files/smith.pdf:pdf;:/tmp/other/smith-preprint.pdf:pdf")
        );
        m.choose_all(1);
        assert_eq!(m.result().get("journal"), Some("Nature Climate Change"));
    }

    #[test]
    fn ids_and_target() {
        let a = entry("a", &[("ids", "old, b"), ("title", "T")]);
        let b = entry("b", &[("title", "T")]);
        let c = entry("c", &[("ids", "a, old2"), ("title", "T")]);
        let m = Merge::new(&[a, b, c], None, true);
        assert_eq!(m.result().get("ids"), Some("old, b, old2, c"));
        let m = Merge::new(
            &[
                entry("a", &[("journal", "J1")]),
                entry("b", &[("journal", "J2")]),
            ],
            None,
            true,
        )
        .with_target(1);
        let r = m.result();
        assert_eq!(
            (r.key.as_str(), r.get("journal"), r.get("ids")),
            ("b", Some("J2"), Some("a"))
        );
        assert_eq!(
            Merge::new(
                &[entry("a", &[("title", "T")]), entry("b", &[("title", "T")])],
                None,
                false
            )
            .result()
            .get("ids"),
            None
        );
    }

    #[test]
    fn resolver() {
        let a = entry("lib", &with(&[("journal", "Nature")]));
        let b = entry("new", &with(&[("journal", "Science")]));
        let verdict = Verdict {
            kind: Kind::Same,
            reason: "same DOI x".into(),
        };
        let merge = Merge::new(&[a, b], None, false);
        let mut r = Resolver::new(OnDuplicate::Update);
        match r.decide(merge.clone(), Context::Add, &verdict).unwrap() {
            Decision::Merge(_, e) => assert_eq!(e.get("journal"), Some("Science")),
            other => panic!("{other:?}"),
        }
        let err = Resolver::new(OnDuplicate::Error)
            .decide(merge, Context::Add, &verdict)
            .unwrap_err();
        assert!(err.0.starts_with("new: already in library as lib (same DOI x; 1 conflict)\nPass --on-duplicate with one of: ask, keep, update, skip"));
    }
}
