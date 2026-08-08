/*
    Appellation: parse <module>
    Created At: 2026.05.09:07:45:15
    Contrib: @FL03
*/
use polymarket::types::U256;

/// Parse a Polymarket token ID (large decimal integer string) into `U256`.
///
/// Rejects empty and zero inputs: `U256::from_str_radix("", 10)` is `Ok(0)`,
/// but no real CLOB token ID is zero — accepting it silently defeats the
/// `BadToken` abstention gate in `process_signal` (bin/node/src/state.rs),
/// which relies on this function erroring when upstream market resolution
/// fell back to an empty `token_id`.
pub fn parse_token_id(s: &str) -> anyhow::Result<U256> {
    let v = U256::from_str_radix(s, 10)
        .map_err(|e| anyhow::anyhow!("unable to parse token_id {s:?}: {e}"))?;
    if v.is_zero() {
        anyhow::bail!("unable to parse token_id {s:?}: token IDs are nonzero");
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::parse_token_id;

    #[test]
    fn parses_real_decimal_token_id() {
        let id = "21742633143463906290569050155826241533067272736897614950488156847949938836455";
        assert!(parse_token_id(id).is_ok());
    }

    #[test]
    fn rejects_empty_string() {
        assert!(parse_token_id("").is_err());
    }

    #[test]
    fn rejects_zero() {
        assert!(parse_token_id("0").is_err());
    }

    #[test]
    fn rejects_hex_and_garbage() {
        assert!(parse_token_id("0x1f").is_err());
        assert!(parse_token_id("not-a-token").is_err());
    }
}
