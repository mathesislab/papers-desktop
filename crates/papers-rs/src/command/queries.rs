//! The queries that a command makes (crossref, doi.org, arXiv): over the network, or, for a
//! test, through a [`Fetcher`] with another transport. The `Fetcher` is made when the first
//! query needs it, so that a command that makes none does not set up the network.

use std::fmt;
use std::sync::{Arc, OnceLock};

use crate::config::dirs::cache_dir;
use crate::fetch::{FetchError, Fetcher};

/// Where the queries of a command go.
#[derive(Clone, Default)]
pub struct Queries {
    given: Option<Arc<Fetcher>>,
    network: Arc<OnceLock<Result<Arc<Fetcher>, FetchError>>>,
}

impl fmt::Debug for Queries {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Queries")
    }
}

impl Queries {
    /// Queries answered by this fetcher (a transport that replays recorded answers).
    pub fn with(fetcher: Fetcher) -> Queries {
        Queries {
            given: Some(Arc::new(fetcher)),
            network: Arc::default(),
        }
    }

    /// The fetcher: the one given, else the network, with the cache of papers.
    pub fn fetcher(&self) -> Result<Arc<Fetcher>, FetchError> {
        if let Some(fetcher) = &self.given {
            return Ok(Arc::clone(fetcher));
        }
        self.network
            .get_or_init(|| Fetcher::new(&cache_dir()).map(Arc::new))
            .clone()
    }
}
