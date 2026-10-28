//! Pools: the libraries that a library exchanges entries with (`papers.pools`, spec `exchange.md`).
//!
//! A library is in one of two states, given by its `pool` setting: self-contained (no pool), or
//! shallow (the pool is a library without pool: the entries of the library link to the files of the
//! pool entries, and its new entries go to the pool). Any library can also exchange entries with
//! another library given on the command line (`--pool PATH`), as it does with its pool, except that
//! the files are copied rather than linked.
//!
//! - [`origin`]: `.papers/origin/`, the records of what was last exchanged with each pool (the
//!   base of the next three-way merge);
//! - [`pool`]: the pool of a library and the libraries given on the command line;
//! - [`exchange`]: pull, push, the links of a shallow library, the repair of broken links.

pub mod exchange;
pub mod lookup;
pub mod origin;
pub mod pool;
pub mod state;

pub use exchange::{Exchange, Outcome, Relinks, Repaired, What, exchange_copy, removed_note};
pub use origin::{Origin, Record, origin_file_name};
pub use pool::{NO_POOL_MESSAGE, Pool, PoolError, PoolIx, Pools, check_poolless, pool_files_dir};
