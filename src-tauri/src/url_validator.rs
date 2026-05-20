//! URL syntax validation + extractor host resolution.
//!
//! `validate_url` parses the input string with the `url` crate and returns
//! `UrlValidation { valid, extractor }` where `valid = true` iff scheme is
//! http/https and host is non-empty. `resolve_extractor` consults
//! `crate::extractors::match_host`.

use crate::error::{AppError, AppResult};
use crate::extractors;
use crate::models::UrlValidation;
use url::Url;

/// Returns Ok(UrlValidation) — never Err for invalid syntax (caller decides
/// how to react). For internal callers that want a Result, use `parse_url`.
pub fn validate_url(s: &str) -> UrlValidation {
    let trimmed = s.trim();
    let parsed = match Url::parse(trimmed) {
        Ok(u) => u,
        Err(_) => return UrlValidation { valid: false, extractor: None },
    };
    let scheme_ok = matches!(parsed.scheme(), "http" | "https");
    let host = parsed.host_str().unwrap_or("");
    if !scheme_ok || host.is_empty() {
        return UrlValidation { valid: false, extractor: None };
    }
    let extractor = extractors::match_host(host).map(|s| s.to_string());
    UrlValidation { valid: true, extractor }
}

/// Strict variant returning AppError on invalid syntax.
pub fn parse_url(s: &str) -> AppResult<Url> {
    let u = Url::parse(s.trim())?;
    if !matches!(u.scheme(), "http" | "https") || u.host_str().unwrap_or("").is_empty() {
        return Err(AppError::InvalidUrl);
    }
    Ok(u)
}

/// Resolve extractor from a URL string (parses first); returns None if URL
/// invalid OR host doesn't match any extractor.
pub fn resolve_extractor(url: &str) -> Option<&'static str> {
    let parsed = Url::parse(url.trim()).ok()?;
    let host = parsed.host_str()?;
    extractors::match_host(host)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_https() {
        let v = validate_url("https://www.youtube.com/watch?v=abc");
        assert!(v.valid);
        assert_eq!(v.extractor.as_deref(), Some("youtube"));
    }

    #[test]
    fn invalid_scheme() {
        let v = validate_url("ftp://example.com/x");
        assert!(!v.valid);
    }

    #[test]
    fn invalid_syntax() {
        let v = validate_url("not a url");
        assert!(!v.valid);
    }

    #[test]
    fn unknown_extractor() {
        let v = validate_url("https://random.example/x");
        assert!(v.valid);
        assert!(v.extractor.is_none());
    }

    #[test]
    fn resolve_extractor_helper() {
        assert_eq!(resolve_extractor("https://x.com/a/status/1"), Some("twitter"));
        assert!(resolve_extractor("not a url").is_none());
    }
}
