//! WalletAddress invariant and wire-contract evaluation.

use rspm::WalletAddress;
use static_assertions::assert_not_impl_any;

const ADDRESS_SOURCE: &str = include_str!("../src/types/address.rs");
const ZERO: &str = "0x0000000000000000000000000000000000000000";

assert_not_impl_any!(WalletAddress: Default);

#[test]
fn construction_canonicalizes_every_supported_hex_digit() {
    let mut passed = 0_usize;
    let digits = b"0123456789abcdefABCDEF";

    for &digit in digits {
        let raw = format!("0x{}", char::from(digit).to_string().repeat(40));
        let address: WalletAddress = raw.parse().expect("ASCII hex address");
        let expected = raw.to_ascii_lowercase();
        assert_eq!(address.as_str(), expected);
        assert_eq!(address.to_string(), expected);
        assert_eq!(address.as_ref(), expected);
        assert_eq!(address.get(), &expected);
        passed += 1;
    }

    assert_eq!(passed, digits.len(), "100% valid-case threshold");
}

/// [REGRESSION][EVAL] Every non-hex ASCII byte is rejected at every public
/// construction boundary. This catches validators that check only prefix and
/// length while still producing a deterministic 100% score.
#[test]
fn every_non_hex_ascii_byte_is_rejected() {
    let mut passed = 0_usize;
    let mut total = 0_usize;

    for byte in 0_u8..=127 {
        if byte.is_ascii_hexdigit() {
            continue;
        }
        let mut candidate = ZERO.as_bytes().to_vec();
        candidate[2] = byte;
        let candidate = String::from_utf8(candidate).expect("ASCII candidate");
        total += 1;
        assert!(
            WalletAddress::from_str(&candidate).is_err(),
            "non-hex byte {byte:#04x} bypassed validation"
        );
        passed += 1;
    }

    assert!(total >= 100, "eval corpus unexpectedly small: {total}");
    assert_eq!(passed, total, "100% invalid-character threshold");
}

#[test]
fn malformed_shape_prefix_and_unicode_are_rejected() {
    let invalid = [
        "",
        "0x",
        "0000000000000000000000000000000000000000",
        "0X0000000000000000000000000000000000000000",
        "0x000000000000000000000000000000000000000",
        "0x00000000000000000000000000000000000000000",
        " 0x0000000000000000000000000000000000000000",
        "0x0000000000000000000000000000000000000000 ",
        "0x000000000000000000000000000000000000000é",
        "0xgggggggggggggggggggggggggggggggggggggggg",
    ];

    for value in invalid {
        assert!(
            value.parse::<WalletAddress>().is_err(),
            "malformed address was accepted: {value:?}"
        );
    }
}

#[test]
fn serde_uses_the_same_validator_and_canonical_form() {
    let checksummed = "0xAbCdEf0123456789aBCdEf0123456789ABCDef01";
    let decoded: WalletAddress =
        serde_json::from_str(&serde_json::to_string(checksummed).expect("serialize fixture"))
            .expect("valid wallet JSON");
    assert_eq!(
        decoded.as_str(),
        "0xabcdef0123456789abcdef0123456789abcdef01"
    );
    assert_eq!(
        serde_json::to_string(&decoded).expect("serialize address"),
        "\"0xabcdef0123456789abcdef0123456789abcdef01\""
    );

    for invalid in [
        serde_json::json!(null),
        serde_json::json!(42),
        serde_json::json!({"address": ZERO}),
        serde_json::json!("0x000000000000000000000000000000000000000g"),
    ] {
        assert!(
            serde_json::from_value::<WalletAddress>(invalid.clone()).is_err(),
            "unchecked serde accepted {invalid}"
        );
    }
}

#[test]
fn representation_has_no_public_mutation_or_default_bypass() {
    assert!(ADDRESS_SOURCE.contains("pub struct WalletAddress(String);"));
    for forbidden in [
        "pub struct WalletAddress(pub String)",
        "derive(Clone, Debug, Default",
        "pub const fn get_mut",
        "derive(serde::Deserialize, serde::Serialize)",
    ] {
        assert!(
            !ADDRESS_SOURCE.contains(forbidden),
            "WalletAddress invariant bypass returned: {forbidden}"
        );
    }
    assert!(ADDRESS_SOURCE.contains("impl<'de> serde::Deserialize<'de> for WalletAddress"));
}
