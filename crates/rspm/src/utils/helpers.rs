/*
    Appellation: helpers <module>
    Created At: 2026.08.08:07:42:02
    Contrib: @FL03
*/
#[cfg(feature = "clob")]
pub fn append_query_pair(
    query: &mut url::form_urlencoded::Serializer<'_, String>,
    name: &str,
    value: Option<&str>,
) {
    if let Some(value) = value {
        query.append_pair(name, value);
    }
}

pub fn canonical_unsigned_integer_text(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && (value.len() == 1 || !value.starts_with('0'))
}

pub fn canonical_nonnegative_decimal_text(value: &str) -> bool {
    let mut parts = value.split('.');
    let whole = parts.next().unwrap_or_default();
    let fraction = parts.next();
    if parts.next().is_some() || !canonical_unsigned_integer_text(whole) {
        return false;
    }
    fraction.is_none_or(|fraction| {
        !fraction.is_empty() && fraction.bytes().all(|byte| byte.is_ascii_digit())
    })
}
