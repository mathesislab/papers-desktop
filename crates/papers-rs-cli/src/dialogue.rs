//! The dialogue of `--on-duplicate ask` (`papers.merge.Dialogue`, `ask`): the
//! duplicate shown as a screen (the entries, the merged entry with `+` for what
//! is added and `!` for a conflict, the choices), and the user decides: take
//! the values of an entry, go field by field, edit the merged entry, say it is
//! not the same paper, skip, quit. The questions go to the standard output and
//! the answers come from the standard input.
//!
//! The same dialogue serves the three-way merges (`pull`, `push`, `history --revert`): the
//! fields show where they changed, a removal is shown as `(removed)`, the value of the base
//! is the `origin` line of a conflict, and `f` goes through the additions too, which can be
//! declined. In a revert papers shows the entries by their keys, as `check` does, and offers
//! what `check` does not (`n`, `s`).

use std::io::{BufRead, Write};

use papers_rs::bib::{Bib, Block, Entry};
use papers_rs::command::edit::{Editor, edit_text, entries_text};
use papers_rs::command::session::no_input_error;
use papers_rs::config::paths::{is_abs, relpath};
use papers_rs::entry::{files, val};
use papers_rs::identity::Verdict;
use papers_rs::log;
use papers_rs::merge::{
    Changed, Context, Decision, DuplicateError, ENTRYTYPE, FieldMerge, Merge, OnDuplicate, REMOVED,
    Resolve, Status,
};

const WARNING: &str = "\x1b[93m";
const OK_BLUE: &str = "\x1b[94m";
const OK_GREEN: &str = "\x1b[92m";
const END: &str = "\x1b[0m";

/// Where the answers come from: `input(prompt)`.
type Input = Box<dyn FnMut(&str) -> Result<String, DuplicateError>>;

/// Where the screens and the questions go.
type Output = Box<dyn FnMut(&str)>;

pub struct Dialogue {
    color: bool,
    editor: Editor,
    quit: bool,
    input: Input,
    output: Output,
    /// the paths of the files are shown relative to it (the current directory)
    cwd: String,
}

impl Dialogue {
    /// The dialogue of a terminal: the questions go to the standard output, the answers come
    /// from the standard input.
    pub fn new(color: bool, editor: Editor) -> Dialogue {
        Dialogue::with_io(color, editor, Box::new(stdin_input), Box::new(say))
    }

    /// A dialogue with its own input and output.
    pub fn with_io(color: bool, editor: Editor, input: Input, output: Output) -> Dialogue {
        Dialogue {
            color,
            editor,
            quit: false,
            input,
            output,
            cwd: papers_rs::config::paths::getcwd(),
        }
    }

    /// The paths of the files are shown relative to this folder.
    #[cfg(test)]
    pub fn in_folder(mut self, cwd: &str) -> Dialogue {
        self.cwd = cwd.to_string();
        self
    }
}

/// An answer, parsed.
#[derive(Debug, PartialEq)]
enum Answer {
    Take(usize),
    Fields,
    Edit,
    Target(usize),
    Distinct,
    Skip,
    Quit,
    Help,
    Invalid(String),
}

/// The text on one line: the words joined by a space, cut at `width` with `...`.
fn oneline(value: &str, width: Option<usize>) -> String {
    let s = value.split_whitespace().collect::<Vec<_>>().join(" ");
    match width {
        Some(w) if s.chars().count() > w => format!(
            "{}...",
            s.chars().take(w.saturating_sub(3)).collect::<String>()
        ),
        _ => s,
    }
}

fn pad(text: &str, width: usize) -> String {
    let n = text.chars().count();
    format!("{text}{}", " ".repeat(width.saturating_sub(n)))
}

/// A path for the screen: relative to the current directory if absolute.
fn shown_path(f: &str, cwd: &str) -> String {
    if is_abs(f) {
        relpath(f, cwd)
    } else {
        f.to_string()
    }
}

/// The screen and the reading of the answers for one merge decision.
struct Screen<'a> {
    merge: &'a Merge,
    context: Context,
    reason: &'a str,
    color: bool,
    /// the paths are shown relative to it
    cwd: &'a str,
}

impl Screen<'_> {
    fn paint(&self, text: &str, color: &str) -> String {
        if self.color {
            format!("{color}{text}{END}")
        } else {
            text.to_string()
        }
    }

    fn is_add(&self) -> bool {
        self.context == Context::Add
    }

    /// `pull` or `push`.
    fn exchange(&self) -> bool {
        self.context.is_exchange()
    }

    /// The name of entry `i`: `library`/`new`, `library`/`pool`, `pool`/`library`, else its key.
    fn label(&self, i: usize) -> String {
        match self.context.labels() {
            Some(labels) => labels[i].to_string(),
            None => self.merge.entries[i].key.clone(),
        }
    }

    /// The command that the dialogue is for, in the menu.
    fn command(&self) -> &'static str {
        match self.context {
            Context::Pull => "pull",
            Context::Push => "push",
            Context::Add => "add",
            Context::Check => "check",
            Context::Revert => "revert",
        }
    }

    /// Where a field changed, in a three-way merge.
    fn change_note(&self, f: &FieldMerge) -> String {
        match f.changed {
            Some(Changed::Target) => format!("changed in {}, kept", self.label(0)),
            Some(Changed::Other) => {
                if f.values[1].is_none() {
                    format!("removed in {}", self.label(1))
                } else if f.values[0].is_none() {
                    format!("added in {}", self.label(1))
                } else {
                    format!("changed in {}", self.label(1))
                }
            }
            Some(Changed::Both) => {
                for i in 0..2 {
                    if f.values[i].is_none() {
                        return format!(
                            "removed in {}, changed in {}",
                            self.label(i),
                            self.label(1 - i)
                        );
                    }
                }
                "changed on both sides".to_string()
            }
            None => String::new(),
        }
    }

    fn header(&self) -> String {
        let m = self.merge;
        let reason = if self.reason.is_empty() {
            String::new()
        } else {
            format!(" ({})", self.reason)
        };
        let pool = if self.reason.is_empty() {
            "pool"
        } else {
            self.reason
        };
        let line = match self.context {
            Context::Add => format!("Already in library: {}{reason}", m.key()),
            Context::Pull => format!("{}  <-  {pool} ({})", m.key(), m.others()[0].key),
            Context::Push => format!("{}  ->  {pool} ({})", m.others()[0].key, m.key()),
            _ => format!("Same paper: {} entries{reason}", m.entries.len()),
        };
        let mut lines = vec![self.paint(&line, OK_BLUE)];
        let rows: Vec<Vec<String>> = m
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let fs: Vec<String> = files(e, m.relative_to.as_deref())
                    .iter()
                    .map(|f| shown_path(f, self.cwd))
                    .collect();
                let doi = val(e, "doi");
                vec![
                    (i + 1).to_string(),
                    if self.context.labels().is_some() {
                        self.label(i)
                    } else {
                        String::new()
                    },
                    e.key.clone(),
                    e.entry_type.to_lowercase(),
                    if doi.is_empty() {
                        "no doi".to_string()
                    } else {
                        format!("doi {doi}")
                    },
                    fs.join(" "),
                ]
            })
            .collect();
        let widths: Vec<usize> = (0..6)
            .map(|j| rows.iter().map(|r| r[j].chars().count()).max().unwrap_or(0))
            .collect();
        for r in &rows {
            let cells: Vec<String> = r
                .iter()
                .zip(&widths)
                .filter(|(_, w)| **w > 0)
                .map(|(c, w)| pad(c, *w))
                .collect();
            lines.push(format!("  {}", cells.join("  ").trim_end()));
        }
        if m.is_three_way() {
            lines.push("     origin = the entry as last exchanged (add, pull or push)".to_string());
        }
        lines.join("\n")
    }

    fn entry_lines(&self) -> Vec<String> {
        let m = self.merge;
        let etype = m
            .fields
            .iter()
            .find(|f| f.name == ENTRYTYPE)
            .expect("the type is a field of a merge");
        let mut names: Vec<&str> = m
            .fields
            .iter()
            .filter(|f| f.name != ENTRYTYPE)
            .map(|f| f.name.as_str())
            .collect();
        names.extend(["file", "ids"]);
        let width = names
            .iter()
            .map(|n| n.chars().count())
            .max()
            .unwrap_or(0)
            .max(4);
        let mut out = vec![format!("@{}{{{},", etype.value().unwrap_or(""), m.key())];
        let mut line =
            |mark: &str, name: &str, value: &str, alternatives: &[(String, String)], note: &str| {
                let mut text = format!("{mark}  {} = {value}", pad(name, width));
                if !note.is_empty() {
                    text = format!("{}   {note}", pad(&text, 50));
                }
                out.push(match mark {
                    "+" => self.paint(&text, OK_GREEN),
                    "!" => self.paint(&text, WARNING),
                    _ => text,
                });
                for (which, v) in alternatives {
                    out.push(
                        self.paint(&format!("!  {}   {which}: {v}", " ".repeat(width)), WARNING),
                    );
                }
            };
        // the other values of a field: which entries, and the value
        let numbered = |alts: Vec<(Vec<usize>, Option<String>)>,
                        show: &dyn Fn(Option<&str>) -> String| {
            alts.into_iter()
                .map(|(idx, v)| {
                    let which = idx
                        .iter()
                        .map(|i| (i + 1).to_string())
                        .collect::<Vec<_>>()
                        .join(",");
                    (which, show(v.as_deref()))
                })
                .collect::<Vec<_>>()
        };
        let braced = |v: Option<&str>, width: Option<usize>| match v {
            None => REMOVED.to_string(),
            Some(v) => format!("{{{}}}", oneline(v, width)),
        };
        // the value of the base, after the other values of a conflict
        let origin = |f: &FieldMerge| -> Vec<(String, String)> {
            match (&f.base, m.is_three_way()) {
                (Some(base), true) => vec![("origin".to_string(), braced(Some(base), None))],
                _ => Vec::new(),
            }
        };
        if etype.status == Status::Conflict {
            let mut alts = numbered(etype.alternatives(), &|v| v.unwrap_or("").to_string());
            alts.extend(origin(etype));
            line(
                "!",
                "type",
                etype.value().unwrap_or(""),
                &alts,
                &self.change_note(etype),
            );
        }
        let others: Vec<&str> = m.others().iter().map(|e| e.key.as_str()).collect();
        if self.context == Context::Check && others.iter().any(|k| *k != m.key()) {
            let alts: Vec<(String, String)> = others
                .iter()
                .enumerate()
                .filter(|(_, k)| **k != m.key())
                .map(|(i, k)| ((i + 2).to_string(), format!("{k} (kept in ids)")))
                .collect();
            line("!", "key", m.key(), &alts, "");
        }
        type Row = (String, String, String, Vec<(String, String)>, String);
        let mut rows: Vec<Row> = Vec::new();
        for f in &m.fields {
            if f.name == ENTRYTYPE || (f.value().is_none() && f.status == Status::Same) {
                continue;
            }
            let note = self.change_note(f);
            if f.status == Status::Conflict {
                let mut alts = numbered(f.alternatives(), &|v| braced(v, None));
                alts.extend(origin(f));
                rows.push((
                    "!".to_string(),
                    f.name.clone(),
                    braced(f.value(), None),
                    alts,
                    note,
                ));
            } else {
                let added = f.status == Status::Added;
                rows.push((
                    if added { "+" } else { " " }.to_string(),
                    f.name.clone(),
                    braced(f.value(), if added { None } else { Some(70) }),
                    Vec::new(),
                    note,
                ));
            }
        }
        if !m.files.is_empty() {
            let joined = m
                .files
                .iter()
                .map(|f| shown_path(f, self.cwd))
                .collect::<Vec<_>>()
                .join(";");
            rows.push((
                if m.new_files.is_empty() { " " } else { "+" }.to_string(),
                "file".to_string(),
                format!("{{{joined}}}"),
                Vec::new(),
                String::new(),
            ));
        }
        if !m.ids.is_empty() {
            rows.push((
                if m.ids_added.is_empty() { " " } else { "+" }.to_string(),
                "ids".to_string(),
                format!("{{{}}}", m.ids.join(", ")),
                Vec::new(),
                String::new(),
            ));
        }
        rows.sort_by(|a, b| a.1.cmp(&b.1));
        for (mark, name, value, alts, note) in &rows {
            line(mark, name, value, alts, note);
        }
        out.push("}".to_string());
        out
    }

    fn summary(&self) -> String {
        let m = self.merge;
        let n = m.conflicts().len();
        let conflicts = match n {
            0 => "No conflict".to_string(),
            1 => "1 conflict".to_string(),
            n => format!("{n} conflicts"),
        };
        if self.is_add() {
            return format!(
                "{conflicts}. The key {} is kept; + lines are added.",
                m.key()
            );
        }
        if self.exchange() {
            let n = m.additions().len();
            return format!(
                "{conflicts}, {n} change{} from the {}. The key {} is kept.",
                if n != 1 { "s" } else { "" },
                self.label(1),
                m.key()
            );
        }
        let others: Vec<String> = (0..m.others().len()).map(|i| (i + 2).to_string()).collect();
        if m.others().len() == 1 {
            format!(
                "{conflicts}. Entry {} is removed after the merge; its key goes to ids.",
                others.join(", ")
            )
        } else {
            format!(
                "{conflicts}. Entries {} are removed after the merge; their keys go to ids.",
                others.join(", ")
            )
        }
    }

    fn menu(&self) -> String {
        let m = self.merge;
        let n = m.entries.len();
        let mut first: Vec<String> = Vec::new();
        if !m.conflicts().is_empty() {
            if self.context.labels().is_some() {
                first.push(format!("1 keep {} values", self.label(0)));
                first.push(format!("2 take {} values", self.label(1)));
            } else {
                for i in 0..n {
                    first.push(format!(
                        "{} {} values of {}",
                        i + 1,
                        if i == 0 { "keep" } else { "take" },
                        i + 1
                    ));
                }
            }
            first.push("f field by field".to_string());
        } else {
            first.push("1 merge".to_string());
            if m.is_three_way() && !m.additions().is_empty() {
                first.push("f field by field".to_string());
            }
        }
        first.push("e edit".to_string());
        let mut second: Vec<String> = Vec::new();
        if self.context == Context::Check {
            second.push("t N make entry N the target".to_string());
        }
        if self.is_add() {
            second.push(format!(
                "n not the same paper (add {} separately)",
                m.others()[0].key
            ));
        } else if !self.exchange() {
            second.push("n not the same paper".to_string());
        }
        second.extend([
            if self.exchange() {
                format!("s skip ({} again later)", self.command())
            } else {
                "s skip".to_string()
            },
            "q quit".to_string(),
            "? help".to_string(),
        ]);
        format!("  {}\n  {}", first.join("   "), second.join("   "))
    }

    fn screen(&self) -> String {
        let mut lines = vec![self.header(), String::new()];
        lines.extend(self.entry_lines());
        lines.push(self.summary());
        lines.push(self.menu());
        lines.join("\n")
    }

    fn help(&self) -> String {
        let n = self.merge.entries.len();
        let mut lines = vec![
            format!(
                "  1..{n}  resolve all remaining conflicts with the values of that entry, and merge"
            ),
            format!(
                "  f      go through the conflicts one at a time{}",
                if self.merge.is_three_way() {
                    " (and the changes from the other side)"
                } else {
                    ""
                }
            ),
            "  e      edit the merged entry in the editor (the other values are % comment lines)"
                .to_string(),
        ];
        if self.context == Context::Check {
            lines.push(
                "  t N    make entry N the target: its key is kept, the other keys go to ids"
                    .to_string(),
            );
            lines.push(
                "  n      not the same paper: keep the entries separate, and do not ask again"
                    .to_string(),
            );
        } else if !self.exchange() {
            lines.push(
                "  n      not the same paper: add the new entry separately, and do not ask again"
                    .to_string(),
            );
        }
        lines.extend([
            "  s      skip: change nothing for this one (asked again next time)".to_string(),
            "  q      quit: skip this one and all remaining duplicates".to_string(),
            "  ?      this help".to_string(),
            "Lines: unmarked = same on all sides, + = added from another entry, ! = conflict (the value used, then the other values, numbered by entry).".to_string(),
        ]);
        lines.join("\n")
    }

    /// The fields that `f` asks about: the conflicts, and in a three-way merge the additions
    /// (after the conflicts).
    fn conflict_indexes(&self) -> Vec<usize> {
        let of = |status: Status| {
            self.merge
                .fields
                .iter()
                .enumerate()
                .filter(move |(_, f)| f.status == status)
                .map(|(i, _)| i)
        };
        let mut indexes: Vec<usize> = of(Status::Conflict).collect();
        if self.merge.is_three_way() {
            indexes.extend(of(Status::Added));
        }
        indexes
    }

    /// The question of a conflicting field and its prompt.
    fn field_question(&self, field: usize) -> (String, String) {
        let f = &self.merge.fields[field];
        let absent = if f.base.is_some() { REMOVED } else { "(none)" };
        let rows: Vec<(String, String, String)> = f
            .values
            .iter()
            .enumerate()
            .filter(|(_, v)| v.is_some() || f.status == Status::Added || f.three_way)
            .map(|(i, v)| {
                (
                    (i + 1).to_string(),
                    v.as_deref()
                        .map_or(absent.to_string(), |v| oneline(v, None)),
                    self.label(i),
                )
            })
            .collect();
        let width = rows.iter().map(|r| r.1.chars().count()).max().unwrap_or(0);
        let mut lines = vec![format!(
            "{}:",
            if f.name == ENTRYTYPE {
                "type"
            } else {
                f.name.as_str()
            }
        )];
        lines.extend(
            rows.iter()
                .map(|(n, v, label)| format!("  {n}  {}  {label}", pad(v, width))),
        );
        let choices = rows
            .iter()
            .map(|r| r.0.clone())
            .collect::<Vec<_>>()
            .join("/");
        (lines.join("\n"), format!("  [{choices}/e] >>> "))
    }

    fn parse(&self, text: &str) -> Answer {
        let s = text.trim();
        let n = self.merge.entries.len();
        if !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()) {
            return match s.parse::<usize>() {
                Ok(i) if (1..=n).contains(&i) => Answer::Take(i - 1),
                _ => Answer::Invalid(s.to_string()),
            };
        }
        match s {
            "f" if self.conflict_indexes().is_empty() => Answer::Invalid(s.to_string()),
            "f" => Answer::Fields,
            "e" => Answer::Edit,
            "n" if self.exchange() => Answer::Invalid(s.to_string()),
            "n" => Answer::Distinct,
            "s" => Answer::Skip,
            "q" => Answer::Quit,
            "?" => Answer::Help,
            _ => {
                let target = s
                    .strip_prefix('t')
                    .map(str::trim_start)
                    .filter(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()));
                match (self.context, target.and_then(|d| d.parse::<usize>().ok())) {
                    (Context::Check, Some(i)) if (1..=n).contains(&i) => Answer::Target(i - 1),
                    _ => Answer::Invalid(s.to_string()),
                }
            }
        }
    }

    fn parse_field(&self, field: usize, text: &str) -> Answer {
        let s = text.trim();
        if s == "e" {
            return Answer::Edit;
        }
        let f = &self.merge.fields[field];
        match s.parse::<usize>() {
            Ok(i)
                if s.chars().all(|c| c.is_ascii_digit())
                    && (1..=f.values.len()).contains(&i)
                    && (f.values[i - 1].is_some() || f.status == Status::Added || f.three_way) =>
            {
                Answer::Take(i - 1)
            }
            _ => Answer::Invalid(s.to_string()),
        }
    }
}

/// The merged entry as plain bibtex, and the alternatives as `%` comment lines below it.
fn merge_edit_text(merge: &Merge) -> String {
    let mut text = entries_text(&[merge.result()]);
    let mut lines = Vec::new();
    for f in merge.conflicts() {
        for (idx, v) in f.alternatives() {
            let which = idx
                .iter()
                .map(|i| (i + 1).to_string())
                .collect::<Vec<_>>()
                .join(", ");
            let value = match v {
                Some(v) => format!("{{{}}}", oneline(&v, None)),
                None => REMOVED.to_string(),
            };
            lines.push(format!("% {} = {value}   ({which})", f.name));
        }
    }
    if !lines.is_empty() {
        text = format!(
            "{}\n\n% values of the other entries (lines starting with % are ignored):\n{}\n",
            text.trim_end_matches('\n'),
            lines.join("\n")
        );
    }
    text
}

/// The entry of an edited text, with the key of the target; `None` if it is not exactly one valid entry.
fn parse_edit(merge: &Merge, text: &str) -> Option<Entry> {
    let text: String = text
        .lines()
        .filter(|l| !l.trim_start().starts_with('%'))
        .collect::<Vec<_>>()
        .join("\n");
    let bib = Bib::parse(&text);
    if bib.blocks.iter().any(|b| matches!(b, Block::Unreadable(_))) || bib.entries().count() != 1 {
        log::error("the edited text is not one bibtex entry");
        return None;
    }
    let mut e = bib.entries().next().cloned().expect("one entry");
    let bad: Vec<String> = e
        .fields
        .iter()
        .filter(|f| {
            let mut chars = f.key.chars();
            !(chars.next().is_some_and(|c| c.is_ascii_alphabetic())
                && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '-'))
        })
        .map(|f| papers_rs::config::json::py_repr_str(&f.key))
        .collect();
    if !bad.is_empty() {
        log::error(&format!(
            "malformed field name(s) in the edited entry: [{}]",
            bad.join(", ")
        ));
        return None;
    }
    if e.key != merge.key() {
        log::warning(&format!(
            "the key {} is kept (edited key {} ignored)",
            merge.key(),
            e.key
        ));
        e.key = merge.key().to_string();
    }
    Some(e)
}

fn say(text: &str) {
    log::println(text);
}

/// `input(prompt)`: the answer; an error of the duplicates when there is no more input.
fn stdin_input(prompt: &str) -> Result<String, DuplicateError> {
    log::print(prompt);
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    match std::io::stdin().lock().read_line(&mut line) {
        Ok(0) | Err(_) => Err(DuplicateError(no_input_error().message().to_string())),
        Ok(_) => Ok(line.trim_end_matches(['\n', '\r']).to_string()),
    }
}

impl Dialogue {
    fn edit(&self, merge: &Merge) -> Option<Decision> {
        let entry =
            edit_text(&merge_edit_text(merge), &self.editor).and_then(|t| parse_edit(merge, &t));
        match entry {
            Some(entry) => Some(Decision::Merge(Box::new(merge.clone()), entry)),
            None => {
                log::error("edit failed: back to the menu (nothing was changed)");
                None
            }
        }
    }

    /// Field by field; `None` when the user went back to the menu (a failed edit).
    fn ask_fields(
        &mut self,
        merge: &mut Merge,
        context: Context,
        reason: &str,
    ) -> Result<Option<Decision>, DuplicateError> {
        let questions = Screen {
            merge,
            context,
            reason,
            color: self.color,
            cwd: &self.cwd,
        }
        .conflict_indexes();
        for field in questions {
            loop {
                let screen = Screen {
                    merge,
                    context,
                    reason,
                    color: self.color,
                    cwd: &self.cwd,
                };
                let (question, prompt) = screen.field_question(field);
                (self.output)(&question);
                let answer = screen.parse_field(field, &(self.input)(&prompt)?);
                match answer {
                    Answer::Take(i) => {
                        let f = &mut merge.fields[field];
                        if f.status == Status::Added {
                            // an addition: entry 1 declines it
                            if i == 0 {
                                f.decline();
                            }
                        } else {
                            f.choose(i);
                        }
                        break;
                    }
                    Answer::Edit => return Ok(self.edit(merge)),
                    Answer::Invalid(text) => (self.output)(&format!(
                        "invalid answer: {}",
                        papers_rs::config::json::py_repr_str(&text)
                    )),
                    _ => {}
                }
            }
        }
        Ok(Some(Decision::Merge(
            Box::new(merge.clone()),
            merge.result(),
        )))
    }
}

impl Resolve for Dialogue {
    fn mode(&self) -> OnDuplicate {
        OnDuplicate::Ask
    }

    fn quit(&self) -> bool {
        self.quit
    }

    fn decide(
        &mut self,
        merge: Merge,
        context: Context,
        verdict: &Verdict,
    ) -> Result<Decision, DuplicateError> {
        if self.quit {
            return Ok(Decision::Skip);
        }
        let mut merge = merge;
        let mut show = true;
        loop {
            let screen = Screen {
                merge: &merge,
                context,
                reason: &verdict.reason,
                color: self.color,
                cwd: &self.cwd,
            };
            if show {
                (self.output)(&screen.screen());
            }
            show = true;
            let answer = screen.parse(&(self.input)(">>> ")?);
            match answer {
                Answer::Take(i) => {
                    merge.choose_all(i);
                    let entry = merge.result();
                    return Ok(Decision::Merge(Box::new(merge), entry));
                }
                Answer::Fields => {
                    if let Some(decision) = self.ask_fields(&mut merge, context, &verdict.reason)? {
                        return Ok(decision);
                    }
                }
                Answer::Edit => {
                    if let Some(decision) = self.edit(&merge) {
                        return Ok(decision);
                    }
                }
                Answer::Target(i) => merge = merge.with_target(i),
                Answer::Distinct => return Ok(Decision::Distinct),
                Answer::Skip => return Ok(Decision::Skip),
                Answer::Quit => {
                    self.quit = true;
                    return Ok(Decision::Skip);
                }
                Answer::Help => {
                    (self.output)(&screen.help());
                    show = false;
                }
                Answer::Invalid(text) => {
                    (self.output)(&format!(
                        "invalid answer: {} (? for help)",
                        papers_rs::config::json::py_repr_str(&text)
                    ));
                    show = false;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! The dialogue as papers does it: the screens, the questions, the answers, the decisions of
    //! the resolver and scripted dialogues, on the cases of `gen_merge3.py` (two-way and
    //! three-way merges, in each context). `PAPERS_RS_MERGE3_FUZZ=FILE` runs a larger file.

    use super::*;
    use papers_rs::bib::{Bib, Block};
    use papers_rs::identity::Kind;
    use serde_json::{Value, json};

    fn entry(v: &Value) -> Entry {
        let mut e = Entry::new(v["type"].as_str().unwrap(), v["key"].as_str().unwrap());
        for f in v["fields"].as_array().unwrap() {
            e.set(f[0].as_str().unwrap(), f[1].as_str().unwrap());
        }
        e
    }

    fn formatted(e: &Entry) -> String {
        let mut bib = Bib::default();
        bib.blocks.push(Block::Entry(e.clone()));
        bib.format()
    }

    fn make(case: &Value) -> Merge {
        let target = entry(&case["target"]);
        let other = entry(&case["other"]);
        let rel = case["relative_to"].as_str();
        match case["mode"].as_str().unwrap() {
            "three" => Merge::exchange(target, other, rel, Some(&entry(&case["base"]))),
            "exchange" => Merge::exchange(target, other, rel, None),
            _ => Merge::new(
                &[target, other],
                rel,
                case["keys_to_ids"].as_bool().unwrap(),
            ),
        }
    }

    fn context(name: &str) -> Context {
        match name {
            "add" => Context::Add,
            "check" => Context::Check,
            "pull" => Context::Pull,
            "push" => Context::Push,
            _ => Context::Revert,
        }
    }

    fn kind(a: &Answer) -> (&'static str, Value) {
        match a {
            Answer::Take(i) => ("take", json!(i)),
            Answer::Target(i) => ("target", json!(i)),
            Answer::Fields => ("fields", Value::Null),
            Answer::Edit => ("edit", Value::Null),
            Answer::Distinct => ("distinct", Value::Null),
            Answer::Skip => ("skip", Value::Null),
            Answer::Quit => ("quit", Value::Null),
            Answer::Help => ("help", Value::Null),
            Answer::Invalid(_) => ("invalid", Value::Null),
        }
    }

    fn check(case: &Value) -> Vec<String> {
        let mut failures = Vec::new();
        let d = &case["dialogue"];
        let (name, reason) = (
            d["context"].as_str().unwrap(),
            d["reason"].as_str().unwrap(),
        );
        let ctx = context(name);
        let mut expect = |what: &str, got: Value, want: &Value| {
            if &got != want {
                let text = |v: &Value| {
                    v.as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| v.to_string())
                };
                failures.push(format!(
                    "{name} {reason:?} {what}:\n--- papers-rs\n{}\n--- papers\n{}\n(target {} other {} base {})",
                    text(&got),
                    text(want),
                    case["target"],
                    case["other"],
                    case["base"]
                ));
            }
        };
        let m = make(case);
        let screen = |color: bool| Screen {
            merge: &m,
            context: ctx,
            reason,
            color,
            cwd: "/",
        };
        let s = screen(false);
        expect("screen", json!(s.screen()), &d["screen"]);
        expect("menu", json!(s.menu()), &d["menu"]);
        expect("summary", json!(s.summary()), &d["summary"]);
        expect("help", json!(s.help()), &d["help"]);
        expect("header", json!(s.header()), &d["header"]);
        if !d["screen_color"].is_null() {
            expect(
                "screen_color",
                json!(screen(true).screen()),
                &d["screen_color"],
            );
        }
        expect(
            "edit_text",
            json!(merge_edit_text(&m)),
            &case["model"]["edit_text"],
        );
        let fields = s.conflict_indexes();
        let questions: Vec<Value> = fields
            .iter()
            .map(|&i| {
                let (q, prompt) = s.field_question(i);
                json!([m.fields[i].name, format!("{q}\n{prompt}")])
            })
            .collect();
        expect("questions", Value::Array(questions), &d["questions"]);
        let parsed: Vec<Value> = d["parse"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| {
                let a = p[0].as_str().unwrap();
                let (k, i) = kind(&s.parse(a));
                json!([a, k, i])
            })
            .collect();
        expect("parse", Value::Array(parsed), &d["parse"]);
        let parsed: Vec<Value> = fields
            .iter()
            .enumerate()
            .map(|(n, &i)| {
                let answers: Vec<Value> = d["parse_field"][n][1]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| {
                        let a = p[0].as_str().unwrap();
                        let (k, n) = kind(&s.parse_field(i, a));
                        json!([a, k, n])
                    })
                    .collect();
                json!([m.fields[i].name, answers])
            })
            .collect();
        expect("parse_field", Value::Array(parsed), &d["parse_field"]);

        // the resolver, in each mode
        let verdict = Verdict {
            kind: Kind::Same,
            reason: reason.to_string(),
        };
        for (mode, word) in [
            (OnDuplicate::Keep, "keep"),
            (OnDuplicate::Update, "update"),
            (OnDuplicate::Skip, "skip"),
            (OnDuplicate::Error, "error"),
        ] {
            let got = match papers_rs::merge::Resolver::new(mode).decide(make(case), ctx, &verdict)
            {
                Ok(Decision::Merge(_, e)) => json!({"kind": "merge", "result": formatted(&e)}),
                Ok(Decision::Skip) => json!({"kind": "skip", "result": null}),
                Ok(Decision::Distinct) => json!({"kind": "distinct", "result": null}),
                Err(e) => json!({"error": e.0}),
            };
            expect(word, got, &d["resolver"][word]);
        }
        expect(
            "describe",
            json!(papers_rs::merge::Resolver::describe(
                &make(case),
                ctx,
                &verdict
            )),
            &d["describe"],
        );

        // a scripted dialogue
        let script = &d["script"];
        let strings = |v: &Value| -> Vec<String> {
            v.as_array()
                .unwrap()
                .iter()
                .map(|a| a.as_str().unwrap().to_string())
                .collect()
        };
        let (mut answers, mut field_answers) = (
            strings(&script["answers"]),
            strings(&script["field_answers"]),
        );
        let printed = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let sink = std::rc::Rc::clone(&printed);
        let input: Input = Box::new(move |prompt: &str| {
            let main = prompt == ">>> ";
            let queue = if main {
                &mut answers
            } else {
                &mut field_answers
            };
            Ok(if queue.is_empty() {
                if main { "q" } else { "1" }.to_string()
            } else {
                queue.remove(0)
            })
        });
        let mut dialogue = Dialogue::with_io(
            false,
            Editor::default(),
            input,
            Box::new(move |text: &str| sink.borrow_mut().push(text.to_string())),
        )
        .in_folder("/");
        let decision = dialogue.decide(make(case), ctx, &verdict).unwrap();
        let (kind, result, key) = match &decision {
            Decision::Merge(m, e) => ("merge", Some(formatted(e)), Some(m.key().to_string())),
            Decision::Distinct => ("distinct", None, None),
            Decision::Skip if dialogue.quit => ("quit", None, None),
            Decision::Skip => ("skip", None, None),
        };
        expect(
            "script printed",
            json!(printed.borrow().clone()),
            &script["printed"],
        );
        expect("script kind", json!(kind), &script["kind"]);
        expect("script result", json!(result), &script["result"]);
        if kind == "merge" {
            expect("script key", json!(key), &script["key"]);
        }
        failures
    }

    fn run(cases: &[Value]) {
        let failures: Vec<String> = cases.iter().flat_map(check).collect();
        assert!(
            failures.is_empty(),
            "{} of {} cases differ; the first:\n{}",
            failures.len(),
            cases.len(),
            failures
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n\n")
        );
    }

    fn load(path: &str) -> Vec<Value> {
        let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
        serde_json::from_str(&text).unwrap()
    }

    #[test]
    fn the_dialogue_as_papers_does_it() {
        let path = format!(
            "{}/../papers-rs/tests/expected/merge3.json",
            env!("CARGO_MANIFEST_DIR")
        );
        run(&load(&path));
    }

    #[test]
    fn a_larger_file() {
        if let Ok(path) = std::env::var("PAPERS_RS_MERGE3_FUZZ") {
            run(&load(&path));
        }
    }
}
