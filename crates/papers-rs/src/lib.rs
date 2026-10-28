//! A Rust port of papers-cli (https://github.com/perrette/papers): the
//! library. papers-cli is the reference; what this crate writes must match
//! what papers writes, byte for byte (see `spec/`).
//!
//! Independent of the app: the only crate of this workspace it depends on is
//! `latexenc-rs` (pylatexenc), which is independent of it too.

pub mod bib;
pub mod command;
pub mod config;
pub mod difflib;
pub mod entry;
pub mod error;
pub mod export;
pub mod extract;
pub mod fetch;
pub mod formats;
pub mod history;
pub mod identity;
pub mod install;
pub mod library;
pub mod lock;
pub mod log;
pub mod merge;
pub mod names;
pub mod pools;
pub mod search;
#[rustfmt::skip]
mod pytables;
