/*
    Appellation: time <module>
    Created At: 2026.08.08:06:49:26
    Contrib: @FL03
*/

#[cfg(feature = "std")]
pub fn unix_timestamp() -> Option<i64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
}
