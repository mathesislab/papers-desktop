//! The requests of the feeds: conditional GETs (ETag, Last-Modified) behind a
//! trait, the network (as papers-rs does it: blocking reqwest, rustls with
//! ring), pacing per host, and recorded replies for the tests.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
use reqwest::header::{self, HeaderMap, HeaderName};
use serde::de::DeserializeOwned;

pub const TIMEOUT: Duration = Duration::from_secs(30);

/// A browser's user agent: Wiley, Science and PNAS refuse (403) a program's.
pub const BROWSER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
    AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36";

/// What a feed request accepts.
pub const FEED_ACCEPT: &str = "application/rss+xml, application/atom+xml, application/rdf+xml, \
    application/xml;q=0.9, text/xml;q=0.9, */*;q=0.8";

/// What a request of an API accepts.
pub const JSON_ACCEPT: &str = "application/json";

/// What a request of a page accepts.
pub const PAGE_ACCEPT: &str = "text/html, application/xhtml+xml;q=0.9, */*;q=0.8";

/// What the APIs are asked with: set by the user, never filled in, and sent
/// only when set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Access {
    /// an address for Crossref's and OpenAlex's polite pools
    pub mailto: Option<String>,
    /// OpenAlex's key, which its queries need
    pub openalex_key: Option<String>,
}

/// Crossref's API, whose list requests are paced (3 a second in its polite pool).
pub const CROSSREF: &str = "api.crossref.org";

/// What a query string leaves as written: the `:`, `,`, `|`, `/` and `*` of
/// filters and cursors stay readable.
const QUERY: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'&')
    .add(b'+')
    .add(b'<')
    .add(b'=')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}');

/// A URL of a base and its parameters, encoded.
pub(crate) fn with_query(base: &str, pairs: &[(&str, &str)]) -> String {
    let query: Vec<String> = pairs
        .iter()
        .map(|(k, v)| format!("{k}={}", utf8_percent_encode(v, QUERY)))
        .collect();
    match query.is_empty() {
        true => base.to_string(),
        false => format!("{base}?{}", query.join("&")),
    }
}

/// A DOI as a path of a URL (its slashes kept).
pub(crate) fn doi_path(doi: &str) -> String {
    utf8_percent_encode(doi, QUERY).to_string()
}

/// The JSON of a GET: None if not found (404 or 410), an error for no answer,
/// another error status or a body that does not parse.
pub(crate) fn get_json<T: DeserializeOwned>(
    http: &dyn Http,
    url: &str,
) -> Result<Option<T>, String> {
    let request = Request {
        url: url.to_string(),
        accept: Some(JSON_ACCEPT.to_string()),
        ..Request::default()
    };
    let reply = http.get(&request)?;
    match reply.status {
        200..=299 => serde_json::from_slice(&reply.body)
            .map(Some)
            .map_err(|e| format!("{url}: {e}")),
        404 | 410 => Ok(None),
        _ => Err(failure(&reply)),
    }
}

/// The message of an error status, with its `Retry-After`.
pub(crate) fn failure(reply: &Reply) -> String {
    let retry = reply
        .retry_after
        .as_ref()
        .map(|r| format!(", retry after {r}"))
        .unwrap_or_default();
    format!("HTTP {}{retry}", reply.status)
}

/// A GET, conditional when the validators of the last answer are given.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Request {
    pub url: String,
    pub accept: Option<String>,
    pub if_none_match: Option<String>,
    pub if_modified_since: Option<String>,
}

impl Request {
    /// A plain GET of a URL.
    pub fn get(url: &str) -> Request {
        Request {
            url: url.to_string(),
            ..Request::default()
        }
    }
}

/// What a request got.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Reply {
    pub status: u16,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    /// the `Retry-After` header of a 429 or 503, as sent (seconds or a date)
    pub retry_after: Option<String>,
    /// the `Content-Type` header, empty if there is none
    pub content_type: String,
    pub body: Vec<u8>,
    /// the URL that answered (after the redirects)
    pub url: String,
}

impl Reply {
    /// A 200 with a body, from a URL.
    pub fn ok(url: &str, body: impl Into<Vec<u8>>) -> Reply {
        Reply::status(url, 200).with_body(body)
    }

    /// An empty answer of a status.
    pub fn status(url: &str, status: u16) -> Reply {
        Reply {
            status,
            url: url.to_string(),
            ..Reply::default()
        }
    }

    /// With a body.
    pub fn with_body(mut self, body: impl Into<Vec<u8>>) -> Reply {
        self.body = body.into();
        self
    }

    /// With the validators of a conditional GET.
    pub fn with_validators(mut self, etag: Option<&str>, last_modified: Option<&str>) -> Reply {
        self.etag = etag.map(String::from);
        self.last_modified = last_modified.map(String::from);
        self
    }
}

/// Where the requests go.
pub trait Http: Send + Sync {
    /// GET a URL. The error is the message of a request that got no answer.
    fn get(&self, request: &Request) -> Result<Reply, String>;
}

/// The network.
pub struct Network {
    client: reqwest::blocking::Client,
}

impl Network {
    /// With a browser's user agent, which every publisher answers.
    pub fn new() -> Result<Network, String> {
        Network::with_user_agent(BROWSER_AGENT)
    }

    /// With another user agent.
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

impl Http for Network {
    fn get(&self, request: &Request) -> Result<Reply, String> {
        let mut get = self.client.get(&request.url);
        let headers = [
            (header::ACCEPT, &request.accept),
            (header::IF_NONE_MATCH, &request.if_none_match),
            (header::IF_MODIFIED_SINCE, &request.if_modified_since),
        ];
        for (name, value) in headers {
            if let Some(value) = value {
                get = get.header(name, value);
            }
        }
        let response = get.send().map_err(|e| describe(&e))?;
        let status = response.status().as_u16();
        let url = response.url().to_string();
        let headers = response.headers().clone();
        let body = response.bytes().map_err(|e| describe(&e))?.to_vec();
        Ok(Reply {
            status,
            etag: text(&headers, header::ETAG),
            last_modified: text(&headers, header::LAST_MODIFIED),
            retry_after: text(&headers, header::RETRY_AFTER),
            content_type: text(&headers, header::CONTENT_TYPE).unwrap_or_default(),
            body,
            url,
        })
    }
}

/// A header's value, if it is text.
fn text(headers: &HeaderMap, name: HeaderName) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(String::from)
}

/// An error and its causes.
fn describe(error: &reqwest::Error) -> String {
    let mut text = error.to_string();
    let mut source = std::error::Error::source(error);
    while let Some(s) = source {
        text.push_str(&format!(": {s}"));
        source = s.source();
    }
    text
}

/// The time of the pacing: real, or a test's.
pub trait Clock: Send + Sync {
    fn now(&self) -> Instant;
    fn sleep(&self, duration: Duration);
}

/// The real time.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// The least time between two requests to a host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pacing {
    pub default: Duration,
    /// a host (and its subdomains) and its interval
    pub hosts: Vec<(String, Duration)>,
}

impl Default for Pacing {
    /// One second; three for arXiv, as its terms ask.
    fn default() -> Pacing {
        Pacing {
            default: Duration::from_secs(1),
            hosts: vec![("arxiv.org".to_string(), Duration::from_secs(3))],
        }
    }
}

impl Pacing {
    /// The default, with Crossref at 3 requests a second with an address for
    /// its polite pool (1 a second without, as the default).
    pub fn polite(access: &Access) -> Pacing {
        let mut pacing = Pacing::default();
        if access.mailto.is_some() {
            let third = Duration::from_millis(334);
            pacing.hosts.push((CROSSREF.to_string(), third));
        }
        pacing
    }

    /// The interval of a host.
    pub fn interval(&self, host: &str) -> Duration {
        self.hosts
            .iter()
            .find(|(h, _)| host == h || host.ends_with(&format!(".{h}")))
            .map_or(self.default, |(_, interval)| *interval)
    }
}

/// An [`Http`] that waits before a request to a host until its interval has
/// passed since the last one.
pub struct Paced<H, C = SystemClock> {
    inner: H,
    pacing: Pacing,
    clock: C,
    /// the start of the last request to each host, given or reserved
    last: Mutex<HashMap<String, Instant>>,
}

impl<H: Http> Paced<H> {
    /// Paced in real time.
    pub fn new(inner: H, pacing: Pacing) -> Paced<H> {
        Paced::with_clock(inner, pacing, SystemClock)
    }
}

impl<H: Http, C: Clock> Paced<H, C> {
    /// Paced by a clock.
    pub fn with_clock(inner: H, pacing: Pacing, clock: C) -> Paced<H, C> {
        Paced {
            inner,
            pacing,
            clock,
            last: Mutex::new(HashMap::new()),
        }
    }

    /// The time a request to a host may start, reserved for it.
    fn reserve(&self, host: &str) -> Instant {
        let now = self.clock.now();
        let mut last = self.last.lock().unwrap();
        let start = match last.get(host) {
            Some(&previous) => now.max(previous + self.pacing.interval(host)),
            None => now,
        };
        last.insert(host.to_string(), start);
        start
    }
}

impl<H: Http, C: Clock> Http for Paced<H, C> {
    fn get(&self, request: &Request) -> Result<Reply, String> {
        let url = url::Url::parse(&request.url).map_err(|e| format!("{}: {e}", request.url))?;
        let host = url.host_str().unwrap_or_default().to_string();
        let start = self.reserve(&host);
        let wait = start.saturating_duration_since(self.clock.now());
        if !wait.is_zero() {
            self.clock.sleep(wait);
        }
        self.inner.get(request)
    }
}

/// Recorded replies, for the tests: each URL's in turn, and the requests made.
#[derive(Default)]
pub struct Recorded {
    replies: Mutex<HashMap<String, VecDeque<Result<Reply, String>>>>,
    requests: Mutex<Vec<Request>>,
}

impl Recorded {
    pub fn new() -> Recorded {
        Recorded::default()
    }

    /// The next reply to a URL.
    pub fn reply(self, url: &str, reply: Reply) -> Recorded {
        self.push(url, Ok(reply))
    }

    /// The next request to a URL gets no answer.
    pub fn fail(self, url: &str, message: &str) -> Recorded {
        self.push(url, Err(message.to_string()))
    }

    fn push(self, url: &str, reply: Result<Reply, String>) -> Recorded {
        let mut replies = self.replies.lock().unwrap();
        replies.entry(url.to_string()).or_default().push_back(reply);
        drop(replies);
        self
    }

    /// The requests made, in order.
    pub fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }
}

impl Http for Recorded {
    fn get(&self, request: &Request) -> Result<Reply, String> {
        self.requests.lock().unwrap().push(request.clone());
        let mut replies = self.replies.lock().unwrap();
        let queue = replies.get_mut(&request.url);
        queue
            .and_then(VecDeque::pop_front)
            .unwrap_or_else(|| Err(format!("{}: no recorded reply", request.url)))
    }
}
