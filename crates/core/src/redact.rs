//! Secrets kept out of what is said: an API key or an e-mail address in the
//! text of an error, such as the URL of a request that failed.

/// The text with each secret replaced by `***`; an empty secret is passed
/// over. The forms a secret takes (as it is, as a URL encodes it) are the
/// caller's to give.
pub fn redact(text: &str, secrets: &[&str]) -> String {
    let mut text = text.to_string();
    for secret in secrets.iter().filter(|s| !s.is_empty()) {
        text = text.replace(secret, "***");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_replaced_wherever_they_are() {
        let url = "https://api.openalex.org/works?api_key=k3y&mailto=me%40example.org: HTTP 500";
        assert_eq!(
            redact(url, &["k3y", "me%40example.org", "me@example.org"]),
            "https://api.openalex.org/works?api_key=***&mailto=***: HTTP 500"
        );
        assert_eq!(redact("api_key=", &[""]), "api_key=");
    }
}
