/*
    Appellation: hash <module>
    Created At: 2026.08.08:06:43:38
    Contrib: @FL03
*/
use alloc::string::String;

/// Lower-hex encode without depending on `LowerHex` for the digest output
/// type, which the pinned `sha2`/`digest` (`hybrid-array`-backed) versions do
/// not implement. `no_std`-compatible: uses `core::fmt::Write` and the
/// crate's `alloc`-sourced `String`.
pub fn to_hex(bytes: &[u8]) -> String {
    use core::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

