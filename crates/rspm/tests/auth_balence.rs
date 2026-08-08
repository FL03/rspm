use super::*;
use polymarket::clob::types::SignatureType;
use polymarket::types::address;

fn authority(version: ProtocolVersion) -> AuthenticatedProtocolAuthority {
    AuthenticatedProtocolAuthority::new(
        version,
        1,
        AuthenticatedCredentialIdentity::for_test("balance"),
    )
    .expect("positive test generation")
}

fn allowances(values: [U256; 3]) -> HashMap<Address, U256> {
    [
        (
            address!("0x1111111111111111111111111111111111111111"),
            values[0],
        ),
        (
            address!("0x2222222222222222222222222222222222222222"),
            values[1],
        ),
        (
            address!("0x3333333333333333333333333333333333333333"),
            values[2],
        ),
    ]
    .into_iter()
    .collect()
}

#[test]
fn collateral_base_units_are_scaled_exactly_without_heuristics() {
    for (raw, expected) in [
        (20_000_000_u64, "20.000000"),
        (999, "0.000999"),
        (1_000, "0.001000"),
        (1_001, "0.001001"),
    ] {
        assert_eq!(
            scale_collateral(U256::from(raw)).expect("bounded collateral"),
            Decimal::from_str_exact(expected).expect("exact expected decimal")
        );
    }
}

#[test]
fn owned_balance_read_and_refresh_share_the_exact_identity_query() {
    assert_eq!(
        AuthenticatedBalanceClient::query(SignatureType::Eoa),
        "asset_type=COLLATERAL&signature_type=0"
    );
    assert_eq!(
        AuthenticatedBalanceClient::query(SignatureType::Proxy),
        "asset_type=COLLATERAL&signature_type=1"
    );
    assert_eq!(BALANCE_ALLOWANCE_PATH, "/balance-allowance");
    assert_eq!(BALANCE_ALLOWANCE_UPDATE_PATH, "/balance-allowance/update");
}

#[test]
fn u256_max_allowances_do_not_overflow_before_binding_minimum() {
    let snapshot = AuthenticatedBalanceSnapshot::try_from_required_raw(
        authority(ProtocolVersion::V2),
        U256::from(20_000_000_u64),
        allowances([U256::MAX, U256::MAX, U256::MAX]),
    )
    .expect("binding balance is Decimal-bounded");
    assert_eq!(
        snapshot.executable_capital().expect("cached exact capital"),
        Decimal::from(20)
    );
}

#[test]
fn empty_allowance_set_is_never_executable_capital() {
    let error = AuthenticatedBalanceSnapshot::try_from_required_raw(
        authority(ProtocolVersion::V2),
        U256::from(20_000_000_u64),
        HashMap::new(),
    )
    .expect_err("an empty authority set is incomplete evidence");
    assert_eq!(error.response_path(), Some("allowances"));
}

#[test]
fn duplicate_allowance_keys_reject_the_complete_response() {
    let body = br#"{
            "balance":"20000000",
            "allowances":{
                "0x1111111111111111111111111111111111111111":"1",
                "0x1111111111111111111111111111111111111111":"2"
            }
        }"#;
    let error = decode_json::<RawBalanceAllowanceResponse>(
        crate::auth::AuthenticatedEndpoint::BalanceAllowance,
        body,
    )
    .expect_err("duplicate authority keys must fail");
    assert_eq!(error.response_path(), Some("allowances"));
    assert!(
        !error
            .to_string()
            .contains("0x1111111111111111111111111111111111111111")
    );
}

#[test]
fn malformed_or_signed_base_units_fail_closed() {
    for raw in ["-1", "+1", "01", "1.0", "1e3"] {
        let body = format!(r#"{{"balance":"{raw}","allowances":{{}}}}"#);
        assert!(
            decode_json::<RawBalanceAllowanceResponse>(
                crate::auth::AuthenticatedEndpoint::BalanceAllowance,
                body.as_bytes(),
            )
            .is_err(),
            "{raw}"
        );
    }
    assert!(
        decode_json::<RawBalanceAllowanceResponse>(
            crate::auth::AuthenticatedEndpoint::BalanceAllowance,
            br#"{"balance":20000000,"allowances":{}}"#,
        )
        .is_err(),
        "numeric JSON must not be accepted for quoted base units"
    );
}

#[test]
fn uint256_max_wire_allowance_decodes_exactly() {
    let address = "0x1111111111111111111111111111111111111111";
    let body = format!(
        r#"{{"balance":"20000000","allowances":{{"{address}":"{}"}}}}"#,
        U256::MAX
    );
    let raw: RawBalanceAllowanceResponse = decode_json(
        crate::auth::AuthenticatedEndpoint::BalanceAllowance,
        body.as_bytes(),
    )
    .expect("quoted uint256 max");
    assert_eq!(raw.allowances.values().copied().next(), Some(U256::MAX));
}

#[test]
fn active_protocol_spender_set_is_explicit_and_complete() {
    for version in [ProtocolVersion::V1, ProtocolVersion::V2] {
        let spenders = required_spenders(version).expect("Polygon config is complete");
        assert_eq!(spenders.len(), 3);
        assert_ne!(spenders[0], spenders[1]);
        assert_ne!(spenders[0], spenders[2]);
        assert_ne!(spenders[1], spenders[2]);
    }
}

#[test]
fn legacy_or_extra_allowance_cannot_replace_active_spender() {
    let [first, second, third] = required_spenders(ProtocolVersion::V2).expect("Polygon V2 config");
    let extra = address!("0x4444444444444444444444444444444444444444");
    let raw = RawBalanceAllowanceResponse {
        balance: U256::from(20_000_000_u64),
        allowances: HashMap::from([(first, U256::MAX), (second, U256::MAX), (extra, U256::MAX)]),
    };
    let error = select_required_snapshot(raw, authority(ProtocolVersion::V2))
        .expect_err("missing active adapter spender must fail closed");
    assert_eq!(error.response_path(), Some("allowances"));
    assert!(!error.to_string().contains(&third.to_string()));
}

#[test]
fn extra_allowance_never_binds_executable_capital() {
    let spenders = required_spenders(ProtocolVersion::V2).expect("Polygon V2 config");
    let mut all = spenders
        .into_iter()
        .map(|spender| (spender, U256::MAX))
        .collect::<HashMap<_, _>>();
    all.insert(
        address!("0x4444444444444444444444444444444444444444"),
        U256::ZERO,
    );
    let snapshot = select_required_snapshot(
        RawBalanceAllowanceResponse {
            balance: U256::from(20_000_000_u64),
            allowances: all,
        },
        authority(ProtocolVersion::V2),
    )
    .expect("complete active spender set");
    assert_eq!(
        snapshot.executable_capital().expect("bounded minimum"),
        Decimal::from(20)
    );
}

#[test]
fn unknown_protocol_or_balance_envelope_fields_fail_closed() {
    assert!(
        decode_json::<ProtocolVersionResponse>(
            crate::auth::AuthenticatedEndpoint::ProtocolVersion,
            br#"{"version":2,"new_protocol_evidence":"must-not-be-shed"}"#,
        )
        .is_err()
    );
    assert!(
        decode_json::<RawBalanceAllowanceResponse>(
            crate::auth::AuthenticatedEndpoint::BalanceAllowance,
            br#"{"balance":"20000000","allowances":{},"new_capital_evidence":"must-not-be-shed"}"#,
        )
        .is_err()
    );
}
