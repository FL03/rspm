/*
    Appellation: retry <module>
    Created At: 2026.07.02
    Contrib: @FL03
*/
//! Bounded retry/backoff for CLOB order submission - #2056/#2057 pre-flip
//! execution hardening.
//!
//! [`RetryPolicy`] caps submission attempts and computes the delay between
//! them, honoring a CLOB-supplied `Retry-After` hint when one is available
//! and falling back to exponential backoff otherwise. [`classify_clob_error`]
//! inspects a [`polymarket::error::Error`] and produces a [`RateLimitSignal`]
//! telling the caller whether (and how long) to wait before retrying.
//! [`parse_retry_after`] and [`parse_retry_after_from_body`] are the two pure
//! parsers behind that classification. The patched SDK reduces raw response
//! bodies to a closed status class before rspm sees them; the latter parser
//! therefore consumes only a bounded `retry_after=N` class in production.

use core::time::Duration;

// ── RetryPolicy ─────────────────────────────────────────────────────────────

/// Bounded retry/backoff policy for CLOB order submission.
///
/// Honors an explicit `Retry-After` hint when present (see
/// [`classify_clob_error`]); falls back to exponential backoff, doubling
/// `base_backoff` per attempt and capping at `max_backoff`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Maximum number of submission attempts (including the first) before
    /// giving up and surfacing the error.
    pub max_attempts: u32,
    /// Base delay used for exponential backoff when no explicit
    /// `Retry-After` hint is available.
    pub base_backoff: Duration,
    /// Hard ceiling on any single backoff delay - explicit hint or computed.
    pub max_backoff: Duration,
}

impl Default for RetryPolicy {
    /// 3 attempts, 250ms base backoff doubling per attempt, capped at 5s -
    /// tuned to stay well inside a single bot decision tick rather than
    /// stalling the caller for the CLOB's full rate-limit window (see the
    /// `polymarket` skill: general CLOB budget refills within a 10s window).
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_backoff: Duration::from_millis(250),
            max_backoff: Duration::from_secs(5),
        }
    }
}

impl RetryPolicy {
    /// `true` when `attempt` (the 1-indexed attempt number that just failed)
    /// has not yet exhausted `max_attempts` - i.e. another attempt is
    /// permitted.
    #[must_use]
    pub fn should_retry(&self, attempt: u32) -> bool {
        attempt < self.max_attempts
    }

    /// Compute the delay before the NEXT attempt.
    ///
    /// `retry_after`, when `Some`, wins outright - still capped at
    /// `max_backoff` (a CLOB-supplied hint larger than our own ceiling is
    /// clamped, never honored past the point of stalling the caller
    /// indefinitely). Otherwise the delay doubles per attempt:
    /// `base_backoff * 2^(attempt-1)`, capped at `max_backoff`.
    #[must_use]
    pub fn backoff_for(&self, attempt: u32, retry_after: Option<Duration>) -> Duration {
        if let Some(hint) = retry_after {
            return hint.min(self.max_backoff);
        }
        let exponent = attempt.saturating_sub(1);
        let factor = 2u32.saturating_pow(exponent);
        self.base_backoff
            .saturating_mul(factor)
            .min(self.max_backoff)
    }
}

// ── RateLimitSignal ──────────────────────────────────────────────────────────

/// Outcome of inspecting a CLOB SDK error for a rate-limit (429) signal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RateLimitSignal {
    /// Not a rate-limit error - callers must not blindly retry.
    NotRateLimited,
    /// HTTP 429 Too Many Requests. `retry_after` carries the bounded wait hint
    /// preserved by the patched SDK's closed status class.
    RateLimited {
        /// Parsed wait hint, or `None` when no hint could be recovered.
        retry_after: Option<Duration>,
    },
}

impl RateLimitSignal {
    /// `true` for [`RateLimitSignal::RateLimited`].
    #[must_use]
    pub fn is_rate_limited(&self) -> bool {
        matches!(self, Self::RateLimited { .. })
    }

    /// The parsed `Retry-After` hint, when this is a rate-limit signal AND a
    /// hint was recoverable. `None` for [`RateLimitSignal::NotRateLimited`]
    /// or a rate-limit signal with no recoverable hint.
    #[must_use]
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::RateLimited { retry_after } => *retry_after,
            Self::NotRateLimited => None,
        }
    }
}

/// Classify a CLOB SDK error as rate-limited (HTTP 429) or not.
///
/// Only [`polymarket::error::Kind::Status`] errors carrying a
/// `429 Too Many Requests` response are treated as rate-limited - every
/// other error kind (validation, synchronization, internal, websocket,
/// geoblock, or a different HTTP status) returns
/// [`RateLimitSignal::NotRateLimited`] so callers never blindly retry a
/// non-transient failure.
///
/// The vendored SDK does not expose response headers, so a genuine
/// `Retry-After` header remains unavailable. Its patched status constructor
/// may preserve only a bounded `retry_after=N` class parsed from a 429 body;
/// every other body byte and every dynamic request path is discarded before
/// this function runs. When no hint survives, callers use the bounded
/// [`RetryPolicy`] backoff. [`parse_retry_after`] separately proves the RFC
/// 9110 delta-seconds contract for any future transport that exposes headers.
#[must_use]

pub fn classify_clob_error(e: &polymarket::error::Error) -> RateLimitSignal {
    let Some(status) = e.downcast_ref::<polymarket::error::Status>() else {
        return RateLimitSignal::NotRateLimited;
    };
    if status.status_code != polymarket::error::StatusCode::TOO_MANY_REQUESTS {
        return RateLimitSignal::NotRateLimited;
    }
    RateLimitSignal::RateLimited {
        retry_after: parse_retry_after_from_body(&status.message),
    }
}

// ── Parsing ──────────────────────────────────────────────────────────────────

/// Parse an HTTP `Retry-After` header value into a [`Duration`].
///
/// Per RFC 9110 §10.2.3, the header carries EITHER delta-seconds (a plain
/// non-negative integer, e.g. `"120"`) OR an HTTP-date. This parser only
/// handles the delta-seconds form - CLOB responses observed in the wild use
/// delta-seconds exclusively; HTTP-date values return `None` rather than
/// guessing at a timezone-aware parse.
///
/// No current call site in this crate has a real header value to hand it
/// (see [`classify_clob_error`]'s doc) - this function exists so the parsing
/// contract is proven independently of that upstream limitation, and so any
/// future direct-HTTP or upgraded-SDK path can wire straight into it.
#[must_use]
pub fn parse_retry_after(header_value: &str) -> Option<Duration> {
    header_value
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

/// Parse a bounded `retry_after` class.
///
/// Production input is the patched SDK's closed `retry_after=N` status
/// message, not a raw response body. The wider case-insensitive grammar is
/// retained for compatibility and pure regression vectors. It never panics,
/// allocates, or returns a hint when no ASCII digit run is present.
#[must_use]
pub fn parse_retry_after_from_body(body: &str) -> Option<Duration> {
    let idx = find_ci(body, "retry_after").or_else(|| find_ci(body, "retry-after"))?;
    let tail = body.as_bytes().get(idx..)?;

    let mut value: u64 = 0;
    let mut found_digit = false;
    for &b in tail {
        if b.is_ascii_digit() {
            found_digit = true;
            value = value.saturating_mul(10).saturating_add(u64::from(b - b'0'));
        } else if found_digit {
            break;
        }
        // else: keep skipping non-digit chars (colon, quote, space, ...)
        // until the first digit is found.
    }
    found_digit.then(|| Duration::from_secs(value))
}

/// Case-insensitive (ASCII) substring search - returns the byte index of the
/// first match, or `None`. Avoids allocating a lowercased copy of `haystack`
/// (kept `alloc`-free so [`parse_retry_after_from_body`] stays usable in a
/// `no_std + alloc`-only build).
fn find_ci(haystack: &str, needle: &str) -> Option<usize> {
    let hb = haystack.as_bytes();
    let nb = needle.as_bytes();
    if nb.is_empty() || nb.len() > hb.len() {
        return None;
    }
    hb.windows(nb.len())
        .position(|w| w.eq_ignore_ascii_case(nb))
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── RetryPolicy::backoff_for ──────────────────────────────────────────

    #[test]
    fn retry_honors_retry_after() {
        let policy = RetryPolicy::default();
        // An explicit hint under the ceiling wins outright, regardless of
        // attempt number.
        assert_eq!(
            policy.backoff_for(1, Some(Duration::from_secs(2))),
            Duration::from_secs(2)
        );
        assert_eq!(
            policy.backoff_for(5, Some(Duration::from_secs(2))),
            Duration::from_secs(2)
        );
    }

    #[test]
    fn retry_after_hint_is_capped_at_max_backoff() {
        let policy = RetryPolicy::default(); // max_backoff = 5s
        assert_eq!(
            policy.backoff_for(1, Some(Duration::from_secs(120))),
            Duration::from_secs(5),
            "a CLOB-supplied hint larger than max_backoff must be clamped"
        );
    }

    #[test]
    fn backoff_schedule_without_hint_is_exponential() {
        let policy = RetryPolicy {
            max_attempts: 6,
            base_backoff: Duration::from_millis(250),
            max_backoff: Duration::from_secs(5),
        };
        assert_eq!(policy.backoff_for(1, None), Duration::from_millis(250));
        assert_eq!(policy.backoff_for(2, None), Duration::from_millis(500));
        assert_eq!(policy.backoff_for(3, None), Duration::from_millis(1_000));
        assert_eq!(policy.backoff_for(4, None), Duration::from_millis(2_000));
        // 250ms * 2^4 = 4000ms, still under the 5s ceiling.
        assert_eq!(policy.backoff_for(5, None), Duration::from_millis(4_000));
        // 250ms * 2^5 = 8000ms > 5s ceiling -> clamped.
        assert_eq!(policy.backoff_for(6, None), Duration::from_secs(5));
    }

    #[test]
    fn should_retry_respects_max_attempts() {
        let policy = RetryPolicy {
            max_attempts: 3,
            ..RetryPolicy::default()
        };
        assert!(policy.should_retry(1));
        assert!(policy.should_retry(2));
        assert!(
            !policy.should_retry(3),
            "3rd attempt already used the budget"
        );
    }

    // ── parse_retry_after (RFC 9110 delta-seconds header) ─────────────────

    #[test]
    fn parse_retry_after_delta_seconds() {
        assert_eq!(parse_retry_after("120"), Some(Duration::from_secs(120)));
        assert_eq!(parse_retry_after("  7 "), Some(Duration::from_secs(7)));
        assert_eq!(parse_retry_after("0"), Some(Duration::from_secs(0)));
    }

    #[test]
    fn parse_retry_after_rejects_http_date_and_garbage() {
        assert_eq!(
            parse_retry_after("Mon, 01 Jan 2026 00:00:00 GMT"),
            None,
            "HTTP-date form is intentionally unsupported"
        );
        assert_eq!(parse_retry_after(""), None);
        assert_eq!(parse_retry_after("-1"), None, "negative deltas are invalid");
    }

    // ── parse_retry_after_from_body (best-effort body scan) ────────────────

    #[test]
    fn parse_retry_after_from_body_json_snake_case() {
        let body = r#"{"error":"rate limited","retry_after":2}"#;
        assert_eq!(
            parse_retry_after_from_body(body),
            Some(Duration::from_secs(2))
        );
    }

    #[test]
    fn parse_retry_after_from_body_header_echo_kebab_case() {
        let body = "429 too many requests, Retry-After: 3";
        assert_eq!(
            parse_retry_after_from_body(body),
            Some(Duration::from_secs(3))
        );
    }

    #[test]
    fn parse_retry_after_from_body_no_hint_returns_none() {
        assert_eq!(parse_retry_after_from_body("rate limited"), None);
        assert_eq!(parse_retry_after_from_body(""), None);
        // Different key entirely - must not false-positive on unrelated digits.
        assert_eq!(parse_retry_after_from_body(r#"{"code":429}"#), None);
    }

    // ── classify_clob_error ─────────────────────────────────────────────────

    #[test]
    fn classify_429_status_as_rate_limited_with_body_hint() {
        let e = polymarket::error::Error::status(
            polymarket::error::StatusCode::TOO_MANY_REQUESTS,
            polymarket::error::Method::POST,
            "/order".to_owned(),
            r#"{"retry_after":2}"#,
        );
        let signal = classify_clob_error(&e);
        assert_eq!(
            signal,
            RateLimitSignal::RateLimited {
                retry_after: Some(Duration::from_secs(2))
            }
        );
        assert!(signal.is_rate_limited());
        assert_eq!(signal.retry_after(), Some(Duration::from_secs(2)));
    }

    #[test]
    fn classify_429_status_without_body_hint_still_rate_limited() {
        let e = polymarket::error::Error::status(
            polymarket::error::StatusCode::TOO_MANY_REQUESTS,
            polymarket::error::Method::POST,
            "/order".to_owned(),
            "too many requests",
        );
        let signal = classify_clob_error(&e);
        assert!(
            signal.is_rate_limited(),
            "429 must always classify as rate-limited"
        );
        assert_eq!(
            signal.retry_after(),
            None,
            "no body hint available -> None, caller falls back to exponential backoff"
        );
    }

    #[test]
    fn classify_non_429_status_as_not_rate_limited() {
        let e = polymarket::error::Error::status(
            polymarket::error::StatusCode::INTERNAL_SERVER_ERROR,
            polymarket::error::Method::POST,
            "/order".to_owned(),
            "internal error",
        );
        assert_eq!(classify_clob_error(&e), RateLimitSignal::NotRateLimited);
    }

    #[test]
    fn classify_validation_error_as_not_rate_limited() {
        let e = polymarket::error::Error::validation("bad price");
        assert_eq!(classify_clob_error(&e), RateLimitSignal::NotRateLimited);
    }
}
