//! The download of a file by URL (`papers.files.download_url`): `add --attach URL`,
//! `file attach KEY URL`, `add URL.pdf`. The file is saved in a fresh temporary directory,
//! under the name of the URL, so that the renames that follow keep a meaningful name; a
//! file downloaded this way is always moved into the files directory (see [`is_download`]).

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::config::paths::{abspath, basename, dirname, getcwd, is_inside};

/// The prefix of the temporary directories of downloaded files.
pub const DOWNLOAD_PREFIX: &str = "papers_dl_";

const TIMEOUT: Duration = Duration::from_secs(30);

/// `tempfile.gettempdir()`: the first of `$TMPDIR`, `$TEMP`, `$TMP`, `/tmp`, `/var/tmp`,
/// `/usr/tmp` that is a directory one can write in (`std::env::temp_dir` is another one
/// on macOS without `$TMPDIR`).
pub fn temp_dir() -> String {
    let from_env = ["TMPDIR", "TEMP", "TMP"]
        .iter()
        .filter_map(|v| std::env::var(v).ok())
        .filter(|d| !d.is_empty());
    let defaults = ["/tmp", "/var/tmp", "/usr/tmp"].map(String::from);
    for dir in from_env.chain(defaults) {
        if std::fs::metadata(&dir).is_ok_and(|m| m.is_dir() && !m.permissions().readonly()) {
            return abspath(&dir);
        }
    }
    getcwd()
}

/// A file downloaded by papers into a temporary directory (`is_download`).
pub fn is_download(file: &str) -> bool {
    let parent = dirname(&abspath(file));
    basename(&parent).starts_with(DOWNLOAD_PREFIX) && is_inside(&parent, &temp_dir())
}

/// The name of the file in a URL (`_basename_from_url`): the last component of its path,
/// unquoted, with the characters that would break paths replaced.
fn basename_from_url(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let rest = rest.split(['?', '#']).next().unwrap_or("");
    let path = rest.find('/').map_or("", |i| &rest[i..]);
    let last = path.rsplit('/').next().unwrap_or("");
    let name = unquote(last);
    let name = if name.is_empty() {
        "download".to_string()
    } else {
        name
    };
    name.replace(['/', '\\'], "_")
}

/// `urllib.parse.unquote`: percent-escapes decoded as UTF-8.
fn unquote(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Some(byte) = std::str::from_utf8(&bytes[i + 1..i + 3])
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `mimetypes.guess_extension` for the types a download is likely to have.
fn guess_extension(content_type: &str) -> Option<&'static str> {
    Some(match content_type {
        "application/pdf" => ".pdf",
        "text/plain" => ".txt",
        "text/csv" => ".csv",
        "text/markdown" => ".md",
        "application/json" => ".json",
        "application/zip" => ".zip",
        "application/gzip" => ".gz",
        "application/x-tar" => ".tar",
        "application/octet-stream" => ".bin",
        "application/msword" => ".doc",
        "application/vnd.ms-excel" => ".xls",
        "application/vnd.ms-powerpoint" => ".ppt",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => ".docx",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => ".xlsx",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation" => ".pptx",
        "image/png" => ".png",
        "image/jpeg" => ".jpg",
        "image/gif" => ".gif",
        "image/svg+xml" => ".svg",
        "image/tiff" => ".tiff",
        _ => return None,
    })
}

fn temporary_directory() -> std::io::Result<String> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let base = std::path::PathBuf::from(temp_dir());
    loop {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = base.join(format!(
            "{DOWNLOAD_PREFIX}{:x}{n:x}",
            nanos ^ u128::from(std::process::id())
        ));
        match std::fs::create_dir(&dir) {
            Ok(()) => return Ok(dir.to_string_lossy().into_owned()),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
}

/// Download a URL to a local file and return its path. With `expect_pdf` the answer must
/// look like a PDF (content type with `pdf`, or bytes starting with `%PDF`); otherwise only
/// an HTML answer is refused (a login or landing page). The error is papers' message.
pub fn download_url(url: &str, expect_pdf: bool, dest_dir: Option<&str>) -> Result<String, String> {
    rustls_provider();
    let client = reqwest::blocking::Client::builder()
        .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36")
        .connect_timeout(TIMEOUT)
        .timeout(TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(url)
        .header(reqwest::header::ACCEPT, accept(expect_pdf))
        .send()
        .map_err(|e| connection_failed(url, error_kind(&e), &describe(&e)))?;
    let status = response.status();
    let ctype = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let waf = response.headers().contains_key("x-amzn-waf-action");
    let body = response.bytes().map_err(|e| describe(&e))?.to_vec();
    save_download(
        url,
        status.as_u16(),
        &ctype,
        waf,
        body,
        expect_pdf,
        dest_dir,
    )
}

/// The `Accept` header of a download.
pub fn accept(expect_pdf: bool) -> &'static str {
    if expect_pdf {
        "application/pdf,*/*;q=0.8"
    } else {
        "*/*"
    }
}

/// The error of a download that got no answer (`kind`: `ConnectionError`, `Timeout`).
pub fn connection_failed(url: &str, kind: &str, detail: &str) -> String {
    format!(
        "Failed to download from {url}: {kind}: {detail}. The connection failed before any HTTP response was received — this is often transient (try again), or due to TLS/network issues."
    )
}

/// The answer of a download, checked and saved as [`download_url`] does: the status (`waf`: the
/// server asks for a challenge, the `x-amzn-waf-action` header), the content type and the body.
/// The file is saved in `dest_dir`, else in a temporary directory of its own, under the name of
/// the URL; its path, or papers' message.
pub fn save_download(
    url: &str,
    status: u16,
    content_type: &str,
    waf: bool,
    body: Vec<u8>,
    expect_pdf: bool,
    dest_dir: Option<&str>,
) -> Result<String, String> {
    let ctype = content_type.to_lowercase();
    let shown = if ctype.is_empty() { "unknown" } else { &ctype };
    if status != 200 {
        let hint = if waf || status == 202 {
            " — server appears to require a JavaScript/CAPTCHA challenge (AWS WAF or similar); download manually in a browser and re-run with the local file path"
        } else {
            ""
        };
        return Err(format!(
            "Failed to download from {url}: HTTP {status} (content-type: {shown}){hint}"
        ));
    }
    if expect_pdf {
        if !ctype.contains("pdf") && !body.starts_with(b"%PDF") {
            return Err(format!(
                "URL did not return a PDF: {url} (content-type: {shown}). The server likely returned an HTML landing/login page; download the PDF manually in a browser and re-run with the local file path."
            ));
        }
    } else {
        let head = body[..body.len().min(512)]
            .trim_ascii_start()
            .to_ascii_lowercase();
        if ctype.contains("html")
            || head.starts_with(b"<!doctype html")
            || head.starts_with(b"<html")
        {
            return Err(format!(
                "URL returned an HTML page rather than a file: {url} (content-type: {shown}). The server likely returned a landing/login page; download the file manually in a browser and re-run with the local file path."
            ));
        }
    }
    let mut name = basename_from_url(url);
    if expect_pdf {
        if !name.to_lowercase().ends_with(".pdf") {
            name.push_str(".pdf");
        }
    } else if let Some(ext) = guess_extension(ctype.split(';').next().unwrap_or("").trim())
        && !name.to_lowercase().ends_with(ext)
    {
        name.push_str(ext);
    }
    let dir = match dest_dir {
        Some(dir) => dir.to_string(),
        None => temporary_directory().map_err(|e| e.to_string())?,
    };
    let local = Path::new(&dir).join(name).to_string_lossy().into_owned();
    std::fs::write(&local, body).map_err(|e| format!("{local}: {e}"))?;
    Ok(local)
}

fn rustls_provider() {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
}

fn error_kind(error: &reqwest::Error) -> &'static str {
    if error.is_timeout() {
        "Timeout"
    } else {
        "ConnectionError"
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_name_of_a_url() {
        assert_eq!(
            basename_from_url("https://x.org/a/b%20c.pdf?dl=1#p"),
            "b c.pdf"
        );
        assert_eq!(basename_from_url("https://x.org/"), "download");
        assert_eq!(basename_from_url("https://x.org"), "download");
    }

    #[test]
    fn a_download_is_in_a_temporary_directory_of_its_own() {
        let dir = temporary_directory().unwrap();
        assert!(is_download(&format!("{dir}/x.pdf")));
        assert!(!is_download(&format!(
            "{}/x.pdf",
            std::env::temp_dir().display()
        )));
        std::fs::remove_dir(dir).unwrap();
    }
}
