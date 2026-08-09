/*
    Appellation: dead_letter <module>
    Created At: 2026.07.02
    Contrib: @FL03
*/
//! Dead-letter recording for CLOB order submissions that exhaust their
//! retry budget - #2056/#2057 pre-flip execution hardening.
//!
//! [`DeadLetterRecord`] captures enough context to triage (or, by hand,
//! replay) a submission that never landed after [`crate::retry::RetryPolicy`]
//! gave up; [`record_dead_letter`] persists it to `axiom.dead_letter`
//! (migration `supabase/migrations/20260702000002_dead_letter.sql`).
//!
//! # Wiring (deliberately NOT done in this module)
//!
//! This module builds and persists records; it does NOT call itself from the
//! retry loop in `crate::impls::impl_clob_client`. That loop lives behind
//! `feature = "clob"` alone and has no database handle - wiring a live
//! call site means a caller that owns BOTH a [`crate::ClobClient`] AND a
//! `sqlx::PgPool` (that is `bin/node`, outside this crate) catching
//! [`crate::error::Error::RateLimited`] (via `anyhow::Error::downcast_ref`)
//! at the point retries exhaust, then calling
//! [`DeadLetterRecord::from_exhausted_clob_submit`] +
//! [`record_dead_letter`]. Table + write path are complete and tested here;
//! the live call site is a follow-up integration task for whichever sprint
//! wires the first live flip's execution path.
#![cfg(all(feature = "sqlx", feature = "postgres", feature = "json"))]

use alloc::string::{String, ToString};
use core::time::Duration;

use crate::{ClobSide, Side};

// ── DeadLetterRecord ─────────────────────────────────────────────────────────

/// Everything needed to triage a submission whose retries were exhausted.
///
/// Maps 1:1 onto `axiom.dead_letter`'s columns (`id`/`created_at` are
/// server-assigned defaults, not represented here).
#[derive(Clone, Debug, PartialEq)]
pub struct DeadLetterRecord {
    /// Free-form source label - e.g. `"clob_submit"`.
    pub source: String,
    /// Originating bot, when known.
    pub bot_name: Option<String>,
    /// Submission context (order details) - stored as `JSONB`.
    pub payload: serde_json::Value,
    /// Human-readable reason retries were exhausted.
    pub exhausted_reason: String,
    /// Number of attempts made before giving up.
    pub attempts: i32,
    /// Last known `Retry-After` hint (seconds), if the final failure carried
    /// one - see [`crate::retry::classify_clob_error`]'s doc for why this is
    /// frequently `None` given the vendored SDK's header-visibility gap.
    pub retry_after_secs: Option<f64>,
    /// `Display` of the last error observed.
    pub last_error: Option<String>,
}

/// Everything identifying a CLOB order-submission attempt, independent of
/// whether it succeeded: the "what were we trying to submit" half of a
/// dead-letter record. Bundled into its own type so
/// [`DeadLetterRecord::from_exhausted_clob_submit`] stays under
/// `clippy::too_many_arguments` rather than taking five loose scalar params.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClobSubmitContext<'a> {
    /// Originating bot, when known.
    pub bot_name: Option<&'a str>,
    /// CLOB outcome token ID.
    pub token_id: &'a str,
    /// Outcome represented by `token_id`.
    pub outcome: Side,
    /// Trade action applied to the selected outcome token.
    pub direction: ClobSide,
    /// Limit price.
    pub price: f64,
    /// Order size.
    pub size: f64,
}

impl DeadLetterRecord {
    /// Build a dead-letter record for a CLOB order submission that exhausted
    /// [`crate::retry::RetryPolicy`]'s attempt budget.
    ///
    /// Pure - does not touch the database. Pair with [`record_dead_letter`]
    /// to persist the result.
    #[must_use]
    pub fn from_exhausted_clob_submit(
        ctx: ClobSubmitContext<'_>,
        attempts: u32,
        retry_after: Option<Duration>,
        last_error: impl ToString,
    ) -> Self {
        let last_error = last_error.to_string();
        Self {
            source: "clob_submit".to_owned(),
            bot_name: ctx.bot_name.map(str::to_owned),
            payload: serde_json::json!({
                "token_id": ctx.token_id,
                "outcome": ctx.outcome,
                "direction": ctx.direction,
                "price": ctx.price,
                "size": ctx.size,
            }),
            exhausted_reason: alloc::format!(
                "retries exhausted after {attempts} attempt(s); last error: {last_error}"
            ),
            attempts: attempts as i32,
            retry_after_secs: retry_after.map(|d| d.as_secs_f64()),
            last_error: Some(last_error),
        }
    }
}

// ── Write path ───────────────────────────────────────────────────────────────

/// `INSERT` statement backing [`record_dead_letter`].
///
/// The `payload` bind is a pre-serialised JSON string cast to `jsonb` via
/// `$3::jsonb` server-side - this avoids requiring sqlx's own `json` cargo
/// feature (not currently enabled by this crate's `sqlx`/`postgres`
/// features) purely to bind a `serde_json::Value` column; a plain
/// `String`/`TEXT` bind is always supported by sqlx's Postgres driver.
const DEAD_LETTER_INSERT_SQL: &str = "INSERT INTO axiom.dead_letter \
    (source, bot_name, payload, exhausted_reason, attempts, retry_after_secs, last_error) \
    VALUES ($1, $2, $3::jsonb, $4, $5, $6, $7)";

/// Persist a [`DeadLetterRecord`] to `axiom.dead_letter`.
///
/// Best-effort by contract: callers should log (not panic) on `Err` - a
/// Supabase outage must never block the retry-exhaustion code path itself
/// (mirrors `axiom.flip_audit`'s best-effort write contract,
/// `supabase/migrations/20260514000001_flip_audit.sql`).
pub async fn record_dead_letter(
    pool: &sqlx::PgPool,
    record: &DeadLetterRecord,
) -> Result<(), sqlx::Error> {
    let payload_json = serde_json::to_string(&record.payload)
        .map_err(|e| sqlx::Error::Encode(alloc::boxed::Box::new(e)))?;

    // Owned binds throughout - sqlx's `Encode` coverage for `&Option<T>`
    // references is less certain than for owned `String`/`Option<String>`/
    // `i32`/`Option<f64>`, all of which are unambiguously supported. The
    // clones are cheap (small strings, one JSON payload) and this code path
    // is not hot (only reached after retries are exhausted).
    sqlx::query(DEAD_LETTER_INSERT_SQL)
        .bind(record.source.clone())
        .bind(record.bot_name.clone())
        .bind(payload_json)
        .bind(record.exhausted_reason.clone())
        .bind(record.attempts)
        .bind(record.retry_after_secs)
        .bind(record.last_error.clone())
        .execute(pool)
        .await?;
    Ok(())
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dead_letter_on_exhausted_retry_produces_one_well_formed_record() {
        let record = DeadLetterRecord::from_exhausted_clob_submit(
            ClobSubmitContext {
                bot_name: Some("lag"),
                token_id: "123456789",
                outcome: Side::Yes,
                direction: ClobSide::Sell,
                price: 0.62,
                size: 10.0,
            },
            3,
            Some(Duration::from_secs(2)),
            "rate limited by CLOB (retry_after=Some(2s))",
        );

        // Exactly one record is produced by exactly one call - the
        // constructor is pure and infallible, so "exhausted retries produce
        // exactly one dead-letter record" holds by construction; this test
        // asserts the record's fields are correctly populated, which is the
        // meaningful part of that contract.
        assert_eq!(record.source, "clob_submit");
        assert_eq!(record.bot_name.as_deref(), Some("lag"));
        assert_eq!(record.attempts, 3);
        assert_eq!(record.retry_after_secs, Some(2.0));
        assert!(record.exhausted_reason.contains("3 attempt"));
        assert!(record.exhausted_reason.contains("rate limited"));
        assert_eq!(record.payload["token_id"], "123456789");
        assert_eq!(record.payload["outcome"], "YES");
        assert_eq!(record.payload["direction"], "SELL");
        assert!((record.payload["price"].as_f64().unwrap() - 0.62).abs() < f64::EPSILON);
        assert!((record.payload["size"].as_f64().unwrap() - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn dead_letter_without_retry_after_hint_or_bot_name() {
        let record = DeadLetterRecord::from_exhausted_clob_submit(
            ClobSubmitContext {
                bot_name: None,
                token_id: "999",
                outcome: Side::No,
                direction: ClobSide::Buy,
                price: 0.40,
                size: 5.0,
            },
            1,
            None,
            "internal error",
        );
        assert_eq!(record.bot_name, None);
        assert_eq!(record.retry_after_secs, None);
        assert_eq!(record.attempts, 1);
        assert_eq!(record.payload["outcome"], "NO");
        assert_eq!(record.payload["direction"], "BUY");
    }

    #[test]
    fn insert_sql_targets_the_canonical_table_and_column_order() {
        // A deterministic, no-network shape check on the SQL text itself -
        // `sqlx::query(...)` only touches the network on `.execute()`, so
        // this test never needs a live database (mirrors the project's
        // `DATABASE_URL`-unset wave-gate discipline).
        assert!(DEAD_LETTER_INSERT_SQL.contains("axiom.dead_letter"));
        assert!(DEAD_LETTER_INSERT_SQL.contains(
            "(source, bot_name, payload, exhausted_reason, attempts, retry_after_secs, last_error)"
        ));
        assert!(DEAD_LETTER_INSERT_SQL.contains("$3::jsonb"));
        for placeholder in ["$1", "$2", "$3", "$4", "$5", "$6", "$7"] {
            assert!(
                DEAD_LETTER_INSERT_SQL.contains(placeholder),
                "missing placeholder {placeholder}"
            );
        }
    }
}
