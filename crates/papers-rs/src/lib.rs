//! A Rust port of papers-cli (https://github.com/perrette/papers): the
//! library. papers-cli is the reference; what this crate writes must match
//! what papers writes, byte for byte (see `spec/`).
//!
//! Independent of the app: it depends on no other crate of this workspace.

pub mod bib;
pub mod config;
pub mod error;
pub mod install;
pub mod lock;
pub mod log;
