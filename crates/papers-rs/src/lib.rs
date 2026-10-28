//! A Rust port of papers-cli (https://github.com/perrette/papers): the
//! library. papers-cli is the reference; what this crate writes must match
//! what papers writes, byte for byte (see `spec/`).
//!
//! Independent of the app: the only crate of this workspace it depends on is
//! `latexenc-rs` (pylatexenc), which is independent of it too.

pub mod bib;
pub mod config;
pub mod entry;
pub mod error;
pub mod history;
pub mod identity;
pub mod install;
pub mod library;
pub mod lock;
pub mod log;
pub mod merge;
pub mod names;
#[rustfmt::skip]
mod pytables;
