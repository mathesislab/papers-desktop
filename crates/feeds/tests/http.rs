//! Pacing per host on a test's clock, and the recorded replies.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use papers_feeds::http::{Clock, Paced, Pacing, Recorded};
use papers_feeds::{Http, Reply, Request};

/// A clock that only moves when slept on, and keeps the sleeps.
#[derive(Clone)]
struct FakeClock {
    start: Instant,
    state: Arc<Mutex<(Duration, Vec<Duration>)>>,
}

impl FakeClock {
    fn new() -> FakeClock {
        FakeClock {
            start: Instant::now(),
            state: Arc::new(Mutex::new((Duration::ZERO, Vec::new()))),
        }
    }

    fn advance(&self, by: Duration) {
        self.state.lock().unwrap().0 += by;
    }

    fn sleeps(&self) -> Vec<Duration> {
        self.state.lock().unwrap().1.clone()
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Instant {
        self.start + self.state.lock().unwrap().0
    }

    fn sleep(&self, duration: Duration) {
        let mut state = self.state.lock().unwrap();
        state.0 += duration;
        state.1.push(duration);
    }
}

fn secs(s: f64) -> Duration {
    Duration::from_secs_f64(s)
}

#[test]
fn requests_to_a_host_wait_its_interval() {
    let urls = [
        "https://tc.copernicus.org/a",
        "https://tc.copernicus.org/b",
        "https://www.nature.com/ngeo.rss",
        "https://rss.arxiv.org/atom/physics.ao-ph",
        "https://rss.arxiv.org/atom/physics.geo-ph",
        "https://tc.copernicus.org/c",
    ];
    let recorded = urls
        .iter()
        .fold(Recorded::new(), |r, url| r.reply(url, Reply::ok(url, "")));
    let clock = FakeClock::new();
    let paced = Paced::with_clock(recorded, Pacing::default(), clock.clone());
    for url in &urls[..5] {
        paced.get(&Request::get(url)).unwrap();
    }
    // copernicus waits 1 s; nature and the first arXiv none; arXiv's second 3 s
    assert_eq!(clock.sleeps(), [secs(1.0), secs(3.0)]);
    clock.advance(secs(0.5));
    paced.get(&Request::get(urls[5])).unwrap();
    assert_eq!(
        clock.sleeps(),
        [secs(1.0), secs(3.0)],
        "3.5 s since copernicus's last"
    );
}

#[test]
fn a_request_soon_after_waits_the_rest() {
    let url = "https://example.org/feed";
    let recorded = Recorded::new()
        .reply(url, Reply::ok(url, ""))
        .reply(url, Reply::ok(url, ""));
    let clock = FakeClock::new();
    let paced = Paced::with_clock(recorded, Pacing::default(), clock.clone());
    paced.get(&Request::get(url)).unwrap();
    clock.advance(secs(0.25));
    paced.get(&Request::get(url)).unwrap();
    assert_eq!(clock.sleeps(), [secs(0.75)]);
}

#[test]
fn pacing_of_subdomains() {
    let pacing = Pacing::default();
    assert_eq!(pacing.interval("export.arxiv.org"), secs(3.0));
    assert_eq!(pacing.interval("arxiv.org"), secs(3.0));
    assert_eq!(pacing.interval("notarxiv.org"), secs(1.0));
}

#[test]
fn recorded_replies_in_turn_and_the_requests() {
    let url = "https://example.org/feed";
    let recorded = Recorded::new()
        .reply(
            url,
            Reply::ok(url, "one").with_validators(Some("\"e1\""), None),
        )
        .fail(url, "connection refused");
    let first = recorded.get(&Request::get(url)).unwrap();
    assert_eq!(first.body, b"one");
    assert_eq!(first.etag.as_deref(), Some("\"e1\""));
    assert_eq!(
        recorded.get(&Request::get(url)),
        Err("connection refused".to_string())
    );
    assert!(
        recorded
            .get(&Request::get(url))
            .unwrap_err()
            .contains("no recorded reply")
    );
    assert_eq!(recorded.requests().len(), 3);
}
