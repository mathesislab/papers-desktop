//! The requests of papers (`papers.fetch.http_get`): a GET with the user agent of papers
//! and a timeout of 30 seconds, behind a trait so that the tests replay recorded answers.

use std::time::Duration;

use crate::error::PAPERS_VERSION;

/// The contact of crossref's "polite pool": the client and an address (the one of papers).
pub fn user_agent() -> String {
    format!(
        "papers/{PAPERS_VERSION} (https://github.com/perrette/papers; mailto:mahe.perrette@gmail.com)"
    )
}

pub const TIMEOUT: Duration = Duration::from_secs(30);

/// What a request got.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    /// the reason phrase of the status line
    pub reason: String,
    /// the `Content-Type` header, empty if there is none
    pub content_type: String,
    pub body: Vec<u8>,
    /// the URL that answered (after the redirects)
    pub url: String,
    /// the `Retry-After` header (of an HTTP 429), if there is one
    pub retry_after: Option<String>,
}

impl Response {
    /// `response.ok`
    pub fn ok(&self) -> bool {
        self.status < 400
    }

    /// `response.raise_for_status()`: the message of the error, if the status is one.
    pub fn status_error(&self) -> Option<String> {
        let kind = match self.status {
            400..=499 => "Client Error",
            500..=599 => "Server Error",
            _ => return None,
        };
        Some(format!(
            "{} {kind}: {} for url: {}",
            self.status, self.reason, self.url
        ))
    }

    /// `response.text`: the body decoded with the charset of the content type (ISO-8859-1 for
    /// a text type without one, as `requests` does, else UTF-8).
    pub fn text(&self) -> String {
        let lower = self.content_type.to_lowercase();
        let charset = lower.split(';').skip(1).find_map(|p| {
            p.trim()
                .strip_prefix("charset=")
                .map(|c| c.trim_matches('"').to_string())
        });
        match charset.as_deref() {
            Some("iso-8859-1" | "latin1" | "latin-1" | "us-ascii" | "ascii") => {
                self.body.iter().map(|&b| b as char).collect()
            }
            Some(_) => String::from_utf8_lossy(&self.body).into_owned(),
            None if lower.starts_with("text/") => self.body.iter().map(|&b| b as char).collect(),
            None => String::from_utf8_lossy(&self.body).into_owned(),
        }
    }
}

/// Where the requests go.
pub trait Transport: Send + Sync {
    /// GET a URL (with its query), with the `Accept` header if one is given. The error is the
    /// message of a request that got no answer (`requests.ConnectionError`, `Timeout`).
    fn get(&self, url: &str, accept: Option<&str>) -> Result<Response, String>;
}

/// The network.
pub struct Network {
    client: reqwest::blocking::Client,
}

impl Network {
    /// With the user agent of papers.
    pub fn new() -> Result<Network, String> {
        Network::with_user_agent(&user_agent())
    }

    /// With another user agent: a program that uses the library says who it is, with the address
    /// of its user when there is one (crossref's polite pool, OpenAlex).
    pub fn with_user_agent(user_agent: &str) -> Result<Network, String> {
        // the build that the app uses (rustls with ring) has no provider of its own
        if rustls::crypto::CryptoProvider::get_default().is_none() {
            let _ = rustls::crypto::ring::default_provider().install_default();
        }
        let client = reqwest::blocking::Client::builder()
            .user_agent(user_agent)
            .connect_timeout(TIMEOUT)
            .timeout(TIMEOUT)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Network { client })
    }
}

impl Transport for Network {
    fn get(&self, url: &str, accept: Option<&str>) -> Result<Response, String> {
        let mut request = self.client.get(url);
        if let Some(accept) = accept {
            request = request.header(reqwest::header::ACCEPT, accept);
        }
        let response = request.send().map_err(|e| describe(&e))?;
        let status = response.status();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let retry_after = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let final_url = response.url().to_string();
        let body = response.bytes().map_err(|e| describe(&e))?.to_vec();
        Ok(Response {
            status: status.as_u16(),
            reason: status.canonical_reason().unwrap_or("").to_string(),
            content_type,
            body,
            url: final_url,
            retry_after,
        })
    }
}

fn describe(error: &reqwest::Error) -> String {
    let mut text = error.to_string();
    let mut source = std::error::Error::source(error);
    while let Some(s) = source {
        text.push_str(&format!(": {s}"));
        source = s.source();
    }
    text
}

/// `urllib.parse.quote(text, safe=safe)`
pub fn quote(text: &str, safe: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        let c = b as char;
        if b.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-' | '~') || safe.contains(c) {
            out.push(c);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `urllib.parse.quote_plus(text)`
pub(crate) fn quote_plus(text: &str) -> String {
    quote(text, " ").replace(' ', "+")
}

/// The URL with the query that `requests` makes of the parameters.
pub fn with_params(url: &str, params: &[(&str, String)]) -> String {
    if params.is_empty() {
        return url.to_string();
    }
    let query: Vec<String> = params
        .iter()
        .map(|(k, v)| format!("{}={}", quote_plus(k), quote_plus(v)))
        .collect();
    format!("{url}?{}", query.join("&"))
}
