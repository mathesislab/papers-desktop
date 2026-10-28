//! Macro, environment and specials specifications, argument parsers and the context database
//! (port of `pylatexenc.macrospec`).
//!
//! A [`ContextDb`] is an ordered list of named categories, each holding macro, environment
//! and specials specs; a lookup takes the first category that defines the name. In Python the
//! one class `LatexContextDb` holds both the *parsing* specs of the latexwalker (`MacroSpec`,
//! `EnvironmentSpec`, `SpecialsSpec`: how a macro's arguments are read) and the *text* specs
//! of latex2text (`MacroTextSpec`, ...: what a macro becomes in text), because it only needs
//! the names of what it stores. Here it is generic over the three spec types:
//! [`LatexContextDb`] (the type of the latexwalker) and
//! [`crate::latex2text::TextContextDb`] (the type of latex2text) are its two instances.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

mod argparsers;

pub use argparsers::{
    ArgsParser, ArgsParserExtra, MacroStandardArgsParser, ParseArgsResult, ParsedMacroArgs,
    VerbatimArgsParser, VerbatimArgType,
};

use crate::latexwalker::{ParsingState, SourceText};

/// A spec that has a macro name (`macroname`).
pub trait HasMacroName {
    fn macroname(&self) -> &str;
}
/// A spec that has an environment name (`environmentname`).
pub trait HasEnvironmentName {
    fn environmentname(&self) -> &str;
}
/// A spec that has the characters of a "latex special" (`specials_chars`).
pub trait HasSpecialsChars {
    fn specials_chars(&self) -> &str;
}

/// Errors of the context database (Python's `ValueError`/`TypeError` there).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextDbError {
    /// `Category {} is already registered in the context database`
    CategoryExists(String),
    /// `Invalid latex macro spec db category: ...`
    InvalidCategory { kind: &'static str, category: String, expected: Vec<String> },
}

impl fmt::Display for ContextDbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ContextDbError::CategoryExists(c) => {
                write!(f, "Category {c} is already registered in the context database")
            }
            ContextDbError::InvalidCategory { kind, category, expected } => write!(
                f,
                "Invalid latex {kind} spec db category: {category:?} (Expected one of {expected:?})"
            ),
        }
    }
}

impl std::error::Error for ContextDbError {}

/// `MacroSpec(macroname, args_parser)`: how the arguments of a macro are parsed.
#[derive(Debug, Clone)]
pub struct MacroSpec {
    pub macroname: String,
    pub args_parser: Arc<dyn ArgsParser>,
}

impl MacroSpec {
    /// `MacroSpec(macroname, argspec)`: arguments given by an argspec string (`*`, `[`, `{`).
    ///
    /// # Panics
    /// If the argspec has other chars (`TypeError` in Python); see [`MacroSpec::try_new`].
    pub fn new(macroname: impl Into<String>, argspec: &str) -> Self {
        Self::try_new(macroname, argspec).expect("invalid argspec")
    }

    /// Like [`MacroSpec::new`], returning the error instead of panicking.
    pub fn try_new(macroname: impl Into<String>, argspec: &str) -> Result<Self, String> {
        Ok(MacroSpec {
            macroname: macroname.into(),
            args_parser: Arc::new(MacroStandardArgsParser::try_new(argspec)?),
        })
    }

    /// `MacroSpec(macroname, args_parser=...)`
    pub fn with_parser(macroname: impl Into<String>, args_parser: impl ArgsParser + 'static) -> Self {
        MacroSpec { macroname: macroname.into(), args_parser: Arc::new(args_parser) }
    }

    /// `MacroSpec.parse_args(w, pos, parsing_state)`
    pub fn parse_args(
        &self,
        w: &crate::latexwalker::LatexWalker,
        pos: usize,
        parsing_state: Option<&Arc<ParsingState>>,
    ) -> Result<ParseArgsResult, crate::latexwalker::LatexWalkerError> {
        self.args_parser.parse_args(w, pos, parsing_state)
    }
}

impl HasMacroName for MacroSpec {
    fn macroname(&self) -> &str {
        &self.macroname
    }
}

/// `EnvironmentSpec(environmentname, args_parser, is_math_mode)`
#[derive(Debug, Clone)]
pub struct EnvironmentSpec {
    pub environmentname: String,
    pub args_parser: Arc<dyn ArgsParser>,
    pub is_math_mode: bool,
}

impl EnvironmentSpec {
    /// `EnvironmentSpec(environmentname, argspec)`.
    ///
    /// # Panics
    /// If the argspec has other chars than `*`, `[`, `{`.
    pub fn new(environmentname: impl Into<String>, argspec: &str) -> Self {
        Self::try_new(environmentname, argspec).expect("invalid argspec")
    }

    /// Like [`EnvironmentSpec::new`], returning the error instead of panicking.
    pub fn try_new(environmentname: impl Into<String>, argspec: &str) -> Result<Self, String> {
        Ok(EnvironmentSpec {
            environmentname: environmentname.into(),
            args_parser: Arc::new(MacroStandardArgsParser::try_new(argspec)?),
            is_math_mode: false,
        })
    }

    /// `EnvironmentSpec(environmentname, args_parser=..., is_math_mode=...)`
    pub fn with_parser(
        environmentname: impl Into<String>,
        args_parser: impl ArgsParser + 'static,
        is_math_mode: bool,
    ) -> Self {
        EnvironmentSpec {
            environmentname: environmentname.into(),
            args_parser: Arc::new(args_parser),
            is_math_mode,
        }
    }

    /// The same spec with `is_math_mode` set.
    pub fn math_mode(mut self, is_math_mode: bool) -> Self {
        self.is_math_mode = is_math_mode;
        self
    }

    /// `EnvironmentSpec.parse_args(w, pos, parsing_state)`
    pub fn parse_args(
        &self,
        w: &crate::latexwalker::LatexWalker,
        pos: usize,
        parsing_state: Option<&Arc<ParsingState>>,
    ) -> Result<ParseArgsResult, crate::latexwalker::LatexWalkerError> {
        self.args_parser.parse_args(w, pos, parsing_state)
    }
}

impl HasEnvironmentName for EnvironmentSpec {
    fn environmentname(&self) -> &str {
        &self.environmentname
    }
}

/// `SpecialsSpec(specials_chars, args_parser)`
#[derive(Debug, Clone)]
pub struct SpecialsSpec {
    pub specials_chars: String,
    pub args_parser: Option<Arc<dyn ArgsParser>>,
}

impl SpecialsSpec {
    /// `SpecialsSpec(specials_chars)`: no arguments.
    pub fn new(specials_chars: impl Into<String>) -> Self {
        SpecialsSpec { specials_chars: specials_chars.into(), args_parser: None }
    }

    /// `SpecialsSpec(specials_chars, args_parser=...)`
    pub fn with_parser(specials_chars: impl Into<String>, args_parser: impl ArgsParser + 'static) -> Self {
        SpecialsSpec { specials_chars: specials_chars.into(), args_parser: Some(Arc::new(args_parser)) }
    }

    /// `SpecialsSpec.parse_args(...)`: `None` when the spec has no args parser.
    pub fn parse_args(
        &self,
        w: &crate::latexwalker::LatexWalker,
        pos: usize,
        parsing_state: Option<&Arc<ParsingState>>,
    ) -> Result<Option<ParseArgsResult>, crate::latexwalker::LatexWalkerError> {
        match &self.args_parser {
            None => Ok(None),
            Some(p) => p.parse_args(w, pos, parsing_state).map(Some),
        }
    }
}

impl HasSpecialsChars for SpecialsSpec {
    fn specials_chars(&self) -> &str {
        &self.specials_chars
    }
}

/// `std_macro(macname, optarg, numargs)`: an optional `[...]` argument (if `optarg`) and
/// `numargs` mandatory arguments.
pub fn std_macro(macname: &str, optarg: bool, numargs: usize) -> MacroSpec {
    MacroSpec::new(macname, &optarg_numargs_to_argspec(optarg, numargs))
}

/// `std_macro(macname, argspec)`
pub fn std_macro_argspec(macname: &str, argspec: &str) -> MacroSpec {
    MacroSpec::new(macname, argspec)
}

/// `std_environment(envname, optarg, numargs, is_math_mode)`
pub fn std_environment(envname: &str, optarg: bool, numargs: usize, is_math_mode: bool) -> EnvironmentSpec {
    EnvironmentSpec::new(envname, &optarg_numargs_to_argspec(optarg, numargs)).math_mode(is_math_mode)
}

/// `std_environment(envname, argspec, is_math_mode=...)`
pub fn std_environment_argspec(envname: &str, argspec: &str, is_math_mode: bool) -> EnvironmentSpec {
    EnvironmentSpec::new(envname, argspec).math_mode(is_math_mode)
}

/// `std_specials(specials_chars)`
pub fn std_specials(specials_chars: &str) -> SpecialsSpec {
    SpecialsSpec::new(specials_chars)
}

fn optarg_numargs_to_argspec(optarg: bool, numargs: usize) -> String {
    let mut a = String::new();
    if optarg {
        a.push('[');
    }
    a.push_str(&"{".repeat(numargs));
    a
}

/// An insertion-ordered map from names to specs (a Python `dict`: assigning an existing key
/// replaces the value and keeps the position).
#[derive(Debug, Clone)]
struct OrdMap<V> {
    index: HashMap<String, usize>,
    items: Vec<V>,
}

impl<V> Default for OrdMap<V> {
    fn default() -> Self {
        OrdMap { index: HashMap::new(), items: Vec::new() }
    }
}

impl<V> OrdMap<V> {
    fn insert(&mut self, key: &str, v: V) {
        match self.index.get(key) {
            Some(&i) => self.items[i] = v,
            None => {
                self.index.insert(key.to_string(), self.items.len());
                self.items.push(v);
            }
        }
    }
    fn get(&self, key: &str) -> Option<&V> {
        self.index.get(key).map(|&i| &self.items[i])
    }
}

#[derive(Debug, Clone)]
struct Category<M, E, S> {
    macros: OrdMap<Arc<M>>,
    environments: OrdMap<Arc<E>>,
    specials: OrdMap<Arc<S>>,
}

/// Where a new category goes in the lookup order (`prepend=`, `insert_before=`,
/// `insert_after=` of `add_context_category()`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum CategoryPosition {
    /// at the end (the default)
    #[default]
    Append,
    /// at the beginning: its specs take precedence over all others
    Prepend,
    /// just before this category (at the beginning if it does not exist)
    InsertBefore(String),
    /// just after this category (at the end if it does not exist)
    InsertAfter(String),
}

/// `LatexContextDb`: an ordered list of categories of macro, environment and specials specs.
///
/// The lookup methods return the spec of the first category that defines the name (else
/// the `unknown_*_spec`, if set).
#[derive(Debug, Clone)]
pub struct ContextDb<M, E, S> {
    category_list: Vec<String>,
    d: HashMap<String, Category<M, E, S>>,
    pub unknown_macro_spec: Option<Arc<M>>,
    pub unknown_environment_spec: Option<Arc<E>>,
    pub unknown_specials_spec: Option<Arc<S>>,
}

/// The context database of the latexwalker (`pylatexenc.macrospec.LatexContextDb` holding
/// parsing specs).
pub type LatexContextDb = ContextDb<MacroSpec, EnvironmentSpec, SpecialsSpec>;

impl<M, E, S> Default for ContextDb<M, E, S> {
    fn default() -> Self {
        ContextDb {
            category_list: Vec::new(),
            d: HashMap::new(),
            unknown_macro_spec: None,
            unknown_environment_spec: None,
            unknown_specials_spec: None,
        }
    }
}

impl<M, E, S> ContextDb<M, E, S>
where
    M: HasMacroName,
    E: HasEnvironmentName,
    S: HasSpecialsChars,
{
    /// An empty database.
    pub fn new() -> Self {
        Self::default()
    }

    /// `add_context_category(category, macros, environments, specials, prepend, insert_before,
    /// insert_after)`: the position is given by `position`.
    ///
    /// Fails if the category exists. If a name occurs twice among the specs, the last wins.
    pub fn add_context_category<IM, IE, IS>(
        &mut self,
        category: &str,
        macros: IM,
        environments: IE,
        specials: IS,
        position: CategoryPosition,
    ) -> Result<(), ContextDbError>
    where
        IM: IntoIterator,
        IM::Item: Into<Arc<M>>,
        IE: IntoIterator,
        IE::Item: Into<Arc<E>>,
        IS: IntoIterator,
        IS::Item: Into<Arc<S>>,
    {
        if self.category_list.iter().any(|c| c == category) {
            return Err(ContextDbError::CategoryExists(category.to_string()));
        }
        let find = |list: &Vec<String>, name: &str| list.iter().position(|c| c == name);
        match position {
            CategoryPosition::Append => self.category_list.push(category.to_string()),
            CategoryPosition::Prepend => self.category_list.insert(0, category.to_string()),
            CategoryPosition::InsertBefore(b) => {
                let i = find(&self.category_list, &b).unwrap_or(0);
                self.category_list.insert(i, category.to_string());
            }
            CategoryPosition::InsertAfter(a) => {
                let i = find(&self.category_list, &a).map(|i| i + 1).unwrap_or(self.category_list.len());
                self.category_list.insert(i, category.to_string());
            }
        }
        let mut cat = Category { macros: OrdMap::default(), environments: OrdMap::default(), specials: OrdMap::default() };
        for m in macros {
            let m: Arc<M> = m.into();
            cat.macros.insert(m.macroname(), m.clone());
        }
        for e in environments {
            let e: Arc<E> = e.into();
            cat.environments.insert(e.environmentname(), e.clone());
        }
        for s in specials {
            let s: Arc<S> = s.into();
            cat.specials.insert(s.specials_chars(), s.clone());
        }
        self.d.insert(category.to_string(), cat);
        Ok(())
    }

    /// A category with only macros: `add_context_category(category, macros=macros, ...)`.
    pub fn add_macro_category<IM>(
        &mut self,
        category: &str,
        macros: IM,
        position: CategoryPosition,
    ) -> Result<(), ContextDbError>
    where
        IM: IntoIterator,
        IM::Item: Into<Arc<M>>,
    {
        self.add_context_category(
            category,
            macros,
            Vec::<Arc<E>>::new(),
            Vec::<Arc<S>>::new(),
            position,
        )
    }

    /// `set_unknown_macro_spec(macrospec)`
    pub fn set_unknown_macro_spec(&mut self, macrospec: Option<Arc<M>>) {
        self.unknown_macro_spec = macrospec;
    }

    /// `set_unknown_environment_spec(environmentspec)`
    pub fn set_unknown_environment_spec(&mut self, environmentspec: Option<Arc<E>>) {
        self.unknown_environment_spec = environmentspec;
    }

    /// `set_unknown_specials_spec(specialsspec)`
    pub fn set_unknown_specials_spec(&mut self, specialsspec: Option<Arc<S>>) {
        self.unknown_specials_spec = specialsspec;
    }

    /// `categories()`: the names of the categories, in lookup order.
    pub fn categories(&self) -> Vec<String> {
        self.category_list.clone()
    }

    /// `get_macro_spec(macroname)`
    pub fn get_macro_spec(&self, macroname: &str) -> Option<&Arc<M>> {
        for cat in &self.category_list {
            if let Some(m) = self.d[cat].macros.get(macroname) {
                return Some(m);
            }
        }
        self.unknown_macro_spec.as_ref()
    }

    /// `get_environment_spec(environmentname)`
    pub fn get_environment_spec(&self, environmentname: &str) -> Option<&Arc<E>> {
        for cat in &self.category_list {
            if let Some(e) = self.d[cat].environments.get(environmentname) {
                return Some(e);
            }
        }
        self.unknown_environment_spec.as_ref()
    }

    /// `get_specials_spec(specials_chars)`
    pub fn get_specials_spec(&self, specials_chars: &str) -> Option<&Arc<S>> {
        for cat in &self.category_list {
            if let Some(s) = self.d[cat].specials.get(specials_chars) {
                return Some(s);
            }
        }
        self.unknown_specials_spec.as_ref()
    }

    /// `test_for_specials(s, pos, parsing_state)`: the specials spec with the longest
    /// `specials_chars` that `s` starts with at `pos` (the earliest category wins a tie).
    pub fn test_for_specials(
        &self,
        s: &SourceText,
        pos: usize,
        _parsing_state: Option<&ParsingState>,
    ) -> Option<&Arc<S>> {
        let mut best: Option<&Arc<S>> = None;
        let mut best_len = 0;
        for cat in &self.category_list {
            for sp in &self.d[cat].specials.items {
                let chars = sp.specials_chars();
                let n = chars.chars().count();
                if n > best_len && s.starts_with(chars, pos) {
                    best = Some(sp);
                    best_len = n;
                }
            }
        }
        best
    }

    fn check_categories(&self, kind: &'static str, categories: Option<&[&str]>) -> Result<Vec<&String>, ContextDbError> {
        match categories {
            None => Ok(self.category_list.iter().collect()),
            Some(cs) => cs
                .iter()
                .map(|c| {
                    self.category_list.iter().find(|x| x.as_str() == *c).ok_or_else(|| {
                        ContextDbError::InvalidCategory {
                            kind,
                            category: c.to_string(),
                            expected: self.category_list.clone(),
                        }
                    })
                })
                .collect(),
        }
    }

    /// `iter_macro_specs(categories)`: all macro specs of the given categories (default: all),
    /// in category order, then in insertion order.
    pub fn iter_macro_specs(&self, categories: Option<&[&str]>) -> Result<Vec<&Arc<M>>, ContextDbError> {
        let cats = self.check_categories("macro", categories)?;
        Ok(cats.into_iter().flat_map(|c| self.d[c].macros.items.iter()).collect())
    }

    /// `iter_environment_specs(categories)`
    pub fn iter_environment_specs(&self, categories: Option<&[&str]>) -> Result<Vec<&Arc<E>>, ContextDbError> {
        let cats = self.check_categories("environment", categories)?;
        Ok(cats.into_iter().flat_map(|c| self.d[c].environments.items.iter()).collect())
    }

    /// `iter_specials_specs(categories)`
    pub fn iter_specials_specs(&self, categories: Option<&[&str]>) -> Result<Vec<&Arc<S>>, ContextDbError> {
        let cats = self.check_categories("environment", categories)?;
        Ok(cats.into_iter().flat_map(|c| self.d[c].specials.items.iter()).collect())
    }

    /// `filter_context(keep_categories, exclude_categories, keep_which)`: a copy with only some
    /// of the categories and kinds of specs (`keep_which` is a subset of `"macros"`,
    /// `"environments"`, `"specials"`; empty means all). Empty `keep_categories` keeps all.
    pub fn filter_context(&self, keep_categories: &[&str], exclude_categories: &[&str], keep_which: &[&str]) -> Self {
        let mut new_context = Self::new();
        new_context.unknown_macro_spec = self.unknown_macro_spec.clone();
        new_context.unknown_environment_spec = self.unknown_environment_spec.clone();
        new_context.unknown_specials_spec = self.unknown_specials_spec.clone();
        let keep_macros = keep_which.is_empty() || keep_which.contains(&"macros");
        let keep_environments = keep_which.is_empty() || keep_which.contains(&"environments");
        let keep_specials = keep_which.is_empty() || keep_which.contains(&"specials");
        for cat in &self.category_list {
            if !keep_categories.is_empty() && !keep_categories.contains(&cat.as_str()) {
                continue;
            }
            if !exclude_categories.is_empty() && exclude_categories.contains(&cat.as_str()) {
                continue;
            }
            let d = &self.d[cat];
            let macros: Vec<Arc<M>> = if keep_macros { d.macros.items.clone() } else { vec![] };
            let envs: Vec<Arc<E>> = if keep_environments { d.environments.items.clone() } else { vec![] };
            let specials: Vec<Arc<S>> = if keep_specials { d.specials.items.clone() } else { vec![] };
            new_context
                .add_context_category(cat, macros, envs, specials, CategoryPosition::Append)
                .expect("categories of a database are unique");
        }
        new_context
    }
}
