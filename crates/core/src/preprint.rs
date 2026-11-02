//! The preprint servers: the start of their DOIs, their sites, their names.
//! An entry of the library with no journal, of a DOI or an address of one,
//! is a preprint: its server goes where its journal would (shown, searched);
//! the feeds take a subscription of a server's site for one of preprints.

/// A preprint server.
pub struct Server {
    /// the start of its DOIs, lowercase
    pub doi: &'static str,
    /// its site: the host of its addresses, or the end of it
    pub host: &'static str,
    pub name: &'static str,
}

/// The servers known. medRxiv shares bioRxiv's DOIs: told apart by the site.
pub const SERVERS: &[Server] = &[
    server("10.48550/arxiv", "arxiv.org", "arXiv"),
    server("", "medrxiv.org", "medRxiv"),
    server("10.1101/", "biorxiv.org", "bioRxiv"),
    server("10.26434/chemrxiv", "chemrxiv.org", "ChemRxiv"),
    server("10.31223/", "eartharxiv.org", "EarthArXiv"),
    server("10.22541/essoar", "essopenarchive.org", "ESS Open Archive"),
    server("10.1002/essoar", "essoar.org", "ESS Open Archive"),
    server("10.5194/egusphere", "egusphere.copernicus.org", "EGUsphere"),
    server("10.20944/preprints", "preprints.org", "Preprints.org"),
    server("10.21203/rs", "researchsquare.com", "Research Square"),
    server("10.2139/ssrn", "ssrn.com", "SSRN"),
];

const fn server(doi: &'static str, host: &'static str, name: &'static str) -> Server {
    Server { doi, host, name }
}

/// The host of an address, lowercase: none without a scheme.
fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let host = rest.split(['/', '?', '#']).next()?;
    let host = host.rsplit('@').next()?.split(':').next()?;
    Some(host.to_ascii_lowercase())
}

/// Whether a host is a server's site, or a subdomain of it.
fn on_site(host: &str, server: &Server) -> bool {
    host == server.host || host.ends_with(&format!(".{}", server.host))
}

/// Whether an address is of a preprint server's site.
pub fn preprint_host(url: &str) -> bool {
    host_of(url).is_some_and(|host| SERVERS.iter().any(|s| on_site(&host, s)))
}

/// The preprint server of a DOI or an address, by its name; the site first
/// (medRxiv's DOIs are bioRxiv's).
pub fn preprint_server(doi: Option<&str>, url: Option<&str>) -> Option<&'static str> {
    let host = url.and_then(host_of);
    let doi = doi.map(str::to_ascii_lowercase);
    let by_site = host.and_then(|h| SERVERS.iter().find(|s| on_site(&h, s)));
    let by_doi = || {
        let doi = doi.as_deref()?;
        SERVERS
            .iter()
            .find(|s| !s.doi.is_empty() && doi.starts_with(s.doi))
    };
    by_site.or_else(by_doi).map(|s| s.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn servers_by_their_dois_and_sites() {
        let of = |doi: &str| preprint_server(Some(doi), None);
        assert_eq!(of("10.5194/egusphere-2025-1234"), Some("EGUsphere"));
        assert_eq!(of("10.48550/arXiv.2401.00001"), Some("arXiv"));
        assert_eq!(of("10.22541/essoar.1234"), Some("ESS Open Archive"));
        assert_eq!(
            of("10.5194/tc-20-1047-2026"),
            None,
            "a journal of the same publisher"
        );
        let med = preprint_server(
            Some("10.1101/2024.01.01"),
            Some("https://www.medrxiv.org/content/x"),
        );
        assert_eq!(med, Some("medRxiv"));
        assert_eq!(of("10.1101/2024.01.01"), Some("bioRxiv"));
        assert_eq!(
            preprint_server(None, Some("https://arxiv.org/abs/2401.00001")),
            Some("arXiv")
        );
        assert_eq!(preprint_server(None, None), None);
    }

    #[test]
    fn sites_and_their_subdomains() {
        assert!(preprint_host("https://egusphere.copernicus.org/preprints/"));
        assert!(preprint_host("http://export.arxiv.org/rss/physics.ao-ph"));
        assert!(!preprint_host("https://notarxiv.org/"));
        assert!(!preprint_host("https://tc.copernicus.org/xml/rss2_0.xml"));
        assert!(!preprint_host("arxiv.org"), "no scheme: no address");
    }
}
