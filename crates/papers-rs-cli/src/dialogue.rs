//! The dialogue of `--on-duplicate ask` (`papers.merge.Dialogue`, `ask`): the
//! duplicate shown as a screen (the entries, the merged entry with `+` for what
//! is added and `!` for a conflict, the choices), and the user decides: take
//! the values of an entry, go field by field, edit the merged entry, say it is
//! not the same paper, skip, quit. The questions go to the standard output and
//! the answers come from the standard input.

use std::io::{BufRead, Write};

use papers_rs::bib::{Bib, Block, Entry};
use papers_rs::command::edit::{Editor, edit_text, entries_text};
use papers_rs::command::session::no_input_error;
use papers_rs::config::paths::{is_abs, relpath};
use papers_rs::entry::{files, val};
use papers_rs::identity::Verdict;
use papers_rs::log;
use papers_rs::merge::{
    Context, Decision, DuplicateError, ENTRYTYPE, Merge, OnDuplicate, Resolve, Status,
};

const WARNING: &str = "\x1b[93m";
const OK_BLUE: &str = "\x1b[94m";
const OK_GREEN: &str = "\x1b[92m";
const END: &str = "\x1b[0m";

pub struct Dialogue {
    color: bool,
    editor: Editor,
    quit: bool,
}

impl Dialogue {
    pub fn new(color: bool, editor: Editor) -> Dialogue {
        Dialogue {
            color,
            editor,
            quit: false,
        }
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
fn shown_path(f: &str) -> String {
    if is_abs(f) {
        relpath(f, ".")
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

    fn label(&self, i: usize) -> String {
        if self.is_add() {
            ["library", "new"][i].to_string()
        } else {
            self.merge.entries[i].key.clone()
        }
    }

    fn header(&self) -> String {
        let m = self.merge;
        let reason = if self.reason.is_empty() {
            String::new()
        } else {
            format!(" ({})", self.reason)
        };
        let line = if self.is_add() {
            format!("Already in library: {}{reason}", m.key())
        } else {
            format!("Same paper: {} entries{reason}", m.entries.len())
        };
        let mut lines = vec![self.paint(&line, OK_BLUE)];
        let rows: Vec<Vec<String>> = m
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let fs: Vec<String> = files(e, m.relative_to.as_deref())
                    .iter()
                    .map(|f| shown_path(f))
                    .collect();
                let doi = val(e, "doi");
                vec![
                    (i + 1).to_string(),
                    if self.is_add() {
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
        let mut line = |mark: &str,
                        name: &str,
                        value: &str,
                        alternatives: &[(Vec<usize>, String)]| {
            let text = format!("{mark}  {} = {value}", pad(name, width));
            out.push(match mark {
                "+" => self.paint(&text, OK_GREEN),
                "!" => self.paint(&text, WARNING),
                _ => text,
            });
            for (idx, v) in alternatives {
                let which = idx
                    .iter()
                    .map(|i| (i + 1).to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                out.push(self.paint(&format!("!  {}   {which}: {v}", " ".repeat(width)), WARNING));
            }
        };
        if etype.status == Status::Conflict {
            line(
                "!",
                "type",
                etype.value().unwrap_or(""),
                &etype.alternatives(),
            );
        }
        let others: Vec<&str> = m.others().iter().map(|e| e.key.as_str()).collect();
        if self.context == Context::Check && others.iter().any(|k| *k != m.key()) {
            let alts: Vec<(Vec<usize>, String)> = others
                .iter()
                .enumerate()
                .filter(|(_, k)| **k != m.key())
                .map(|(i, k)| (vec![i + 1], format!("{k} (kept in ids)")))
                .collect();
            line("!", "key", m.key(), &alts);
        }
        let braced = |v: &str, width: Option<usize>| format!("{{{}}}", oneline(v, width));
        type Row = (String, String, String, Vec<(Vec<usize>, String)>);
        let mut rows: Vec<Row> = Vec::new();
        for f in &m.fields {
            if f.name == ENTRYTYPE || (f.value().is_none() && f.status == Status::Same) {
                continue;
            }
            let value = f.value().unwrap_or("");
            if f.status == Status::Conflict {
                let alts = f
                    .alternatives()
                    .into_iter()
                    .map(|(idx, v)| (idx, braced(&v, None)))
                    .collect();
                rows.push(("!".to_string(), f.name.clone(), braced(value, None), alts));
            } else {
                let mark = if f.status == Status::Added { "+" } else { " " };
                let width = if f.status == Status::Added {
                    None
                } else {
                    Some(70)
                };
                rows.push((
                    mark.to_string(),
                    f.name.clone(),
                    braced(value, width),
                    Vec::new(),
                ));
            }
        }
        if !m.files.is_empty() {
            let joined = m
                .files
                .iter()
                .map(|f| shown_path(f))
                .collect::<Vec<_>>()
                .join(";");
            rows.push((
                if m.new_files.is_empty() { " " } else { "+" }.to_string(),
                "file".to_string(),
                format!("{{{joined}}}"),
                Vec::new(),
            ));
        }
        if !m.ids.is_empty() {
            rows.push((
                if m.ids_added.is_empty() { " " } else { "+" }.to_string(),
                "ids".to_string(),
                format!("{{{}}}", m.ids.join(", ")),
                Vec::new(),
            ));
        }
        rows.sort_by(|a, b| a.1.cmp(&b.1));
        for (mark, name, value, alts) in &rows {
            line(mark, name, value, alts);
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
            if self.is_add() {
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
        } else {
            second.push("n not the same paper".to_string());
        }
        second.extend([
            "s skip".to_string(),
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
            "  f      go through the conflicts one at a time".to_string(),
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
        } else {
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

    fn conflict_indexes(&self) -> Vec<usize> {
        self.merge
            .fields
            .iter()
            .enumerate()
            .filter(|(_, f)| f.status == Status::Conflict)
            .map(|(i, _)| i)
            .collect()
    }

    /// The question of a conflicting field and its prompt.
    fn field_question(&self, field: usize) -> (String, String) {
        let f = &self.merge.fields[field];
        let rows: Vec<(String, String, String)> = f
            .values
            .iter()
            .enumerate()
            .filter_map(|(i, v)| {
                v.as_ref()
                    .map(|v| ((i + 1).to_string(), oneline(v, None), self.label(i)))
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
                    && f.values[i - 1].is_some() =>
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
            lines.push(format!(
                "% {} = {{{}}}   ({which})",
                f.name,
                oneline(&v, None)
            ));
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
fn input(prompt: &str) -> Result<String, DuplicateError> {
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
        &self,
        merge: &mut Merge,
        context: Context,
        reason: &str,
    ) -> Result<Option<Decision>, DuplicateError> {
        let questions = Screen {
            merge,
            context,
            reason,
            color: self.color,
        }
        .conflict_indexes();
        for field in questions {
            loop {
                let screen = Screen {
                    merge,
                    context,
                    reason,
                    color: self.color,
                };
                let (question, prompt) = screen.field_question(field);
                say(&question);
                let answer = screen.parse_field(field, &input(&prompt)?);
                match answer {
                    Answer::Take(i) => {
                        merge.fields[field].choose(i);
                        break;
                    }
                    Answer::Edit => return Ok(self.edit(merge)),
                    Answer::Invalid(text) => say(&format!(
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
            };
            if show {
                say(&screen.screen());
            }
            show = true;
            let answer = screen.parse(&input(">>> ")?);
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
                    say(&screen.help());
                    show = false;
                }
                Answer::Invalid(text) => {
                    say(&format!(
                        "invalid answer: {} (? for help)",
                        papers_rs::config::json::py_repr_str(&text)
                    ));
                    show = false;
                }
            }
        }
    }
}
