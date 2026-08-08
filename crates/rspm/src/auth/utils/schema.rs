//! Strict authenticated response schema and pagination helpers.
use crate::auth::{
    AuthenticatedEndpoint, AuthenticatedEndpointError, MAX_PAGE_LIMIT, MAX_VENUE_IDENTIFIER_BYTES,
    TERMINAL_CURSOR,
};

/// Closed alphabet for Polymarket trade and order identities.
///
/// These values become components of canonical execution keys and may appear
/// near diagnostics. Delimiters, whitespace, controls, Unicode lookalikes,
/// and URL metacharacters are rejected before either use. Current venue
/// UUIDs, hexadecimal hashes, and synthetic test identities fit this alphabet.
pub fn venue_identifier_is_valid(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_VENUE_IDENTIFIER_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

pub fn cursor_is_terminal(cursor: &str) -> bool {
    cursor.is_empty() || cursor == TERMINAL_CURSOR
}

pub fn cursor_is_valid(cursor: &str) -> bool {
    !cursor.is_empty()
        && cursor.len() <= 1_024
        && cursor
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'='))
}

pub fn validate_page(
    endpoint: AuthenticatedEndpoint,
    data_len: usize,
    count: u64,
    limit: u64,
    next_cursor: &str,
) -> Result<(), AuthenticatedEndpointError> {
    let count_matches = usize::try_from(count).ok() == Some(data_len);
    let bounded = limit != 0
        && limit <= MAX_PAGE_LIMIT
        && count <= limit
        && u64::try_from(data_len).is_ok_and(|len| len <= limit);
    let cursor_valid = cursor_is_terminal(next_cursor) || cursor_is_valid(next_cursor);
    if !count_matches
        || !bounded
        || !cursor_valid
        || (data_len == 0 && !cursor_is_terminal(next_cursor))
    {
        return Err(AuthenticatedEndpointError::request_failed(
            endpoint,
        ));
    }
    Ok(())
}

