use super::*;
use core::str::FromStr as _;
use polymarket::auth::Credentials;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

const TEST_PRIVATE_KEY: &str = "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d";

#[test]
fn share_serialization_truncates_without_inflating() {
    assert_eq!(clob_share_decimal(10.609).unwrap().to_string(), "10.60");
    assert_eq!(clob_share_decimal(10.6).unwrap().to_string(), "10.60");
    assert!(clob_share_decimal(0.001).is_err());
}

#[test]
fn price_serialization_preserves_thousandth_ticks() {
    assert_eq!(clob_price_decimal(0.001).unwrap().to_string(), "0.001");
    assert_eq!(clob_price_decimal(0.555).unwrap().to_string(), "0.555");
}

#[tokio::test]
async fn declared_authority_constructor_rebuilds_eoa_and_proxy_identity() {
    let signer = LocalSigner::from_str(TEST_PRIVATE_KEY)
        .expect("test signer")
        .with_chain_id(Some(POLYGON));
    let config = ClobConfig::new("http://127.0.0.1:1", None);
    let eoa = ClobClient::from_authenticated_authority(
        config.clone(),
        Credentials::default(),
        signer.clone(),
        None,
        SignatureType::Eoa,
    )
    .await
    .expect("exact EOA authority");
    assert_eq!(eoa.signature_type(), SignatureType::Eoa);
    assert_eq!(eoa.account_address(), signer.address());

    let proxy =
        Address::from_str("0x1111111111111111111111111111111111111111").expect("proxy address");
    let rebuilt = ClobClient::from_authenticated_authority(
        config.clone(),
        Credentials::default(),
        signer.clone(),
        Some(proxy),
        SignatureType::Proxy,
    )
    .await
    .expect("matching proxy parts");
    assert_eq!(rebuilt.signature_type(), SignatureType::Proxy);
    assert_eq!(rebuilt.account_address(), proxy);

    assert!(
        ClobClient::from_authenticated_authority(
            config.clone(),
            Credentials::default(),
            signer.clone(),
            None,
            SignatureType::Proxy,
        )
        .await
        .is_err(),
        "Proxy auth without an explicit funder must fail closed"
    );
    assert!(
        ClobClient::from_authenticated_authority(
            config.clone(),
            Credentials::default(),
            signer.clone(),
            Some(proxy),
            SignatureType::Eoa,
        )
        .await
        .is_err(),
        "EOA auth with a funder must fail closed"
    );
    assert!(
        ClobClient::from_authenticated_authority(
            config,
            Credentials::default(),
            signer.clone(),
            Some(signer.address()),
            SignatureType::Proxy,
        )
        .await
        .is_err(),
        "Proxy funder equal to the signer is an ambiguous EOA identity"
    );
}

#[tokio::test]
async fn declared_authority_rejects_foreign_https_before_authentication() {
    let signer = LocalSigner::from_str(TEST_PRIVATE_KEY)
        .expect("test signer")
        .with_chain_id(Some(POLYGON));
    let result = ClobClient::from_authenticated_authority(
        ClobConfig::new("https://attacker.invalid", None),
        Credentials::default(),
        signer,
        None,
        SignatureType::Eoa,
    )
    .await;
    let Err(error) = result else {
        panic!("foreign credential-bearing authority must fail before a request");
    };
    assert!(matches!(error, crate::Error::InvalidClobConfiguration));
}

#[test]
fn declared_authority_is_the_only_owned_authenticated_constructor() {
    let source = include_str!("../client.rs");
    assert!(!source.contains("pub fn from_parts("));
    let constructor = source
        .split("pub async fn from_authenticated_authority(")
        .nth(1)
        .expect("owned authority constructor")
        .split("pub const fn account_address")
        .next()
        .expect("authority constructor boundary");
    assert!(constructor.contains("canonical_account_address("));
    assert!(constructor.contains("authenticate_declared_authority("));

    let rebuild = source
        .split("async fn authenticate_declared_authority(")
        .nth(1)
        .expect("authority rebuild")
        .split("impl ClobClient")
        .next()
        .expect("authority rebuild boundary");
    assert!(rebuild.contains(".signature_type(signature_type)"));
    assert!(rebuild.contains(".credentials(credentials)"));
    assert!(rebuild.contains("builder.funder(account_address)"));
    assert!(rebuild.contains("builder.authenticate().await?"));
}

#[test]
fn every_constructor_uses_the_same_eoa_or_distinct_proxy_identity_contract() {
    let signer = LocalSigner::from_str(TEST_PRIVATE_KEY)
        .expect("test signer")
        .with_chain_id(Some(POLYGON));
    assert_eq!(
        canonical_account_address(signer.address(), SignatureType::Eoa, None)
            .expect("canonical EOA"),
        signer.address()
    );
    assert!(
        canonical_account_address(
            signer.address(),
            SignatureType::Proxy,
            Some(signer.address()),
        )
        .is_err()
    );
    assert!(
        canonical_account_address(signer.address(), SignatureType::Proxy, Some(Address::ZERO),)
            .is_err()
    );
    assert!(
        canonical_account_address(
            signer.address(),
            SignatureType::Eoa,
            Some(Address::from([7_u8; 20])),
        )
        .is_err()
    );
}

#[test]
fn failed_activation_never_publishes_a_submission_permit() {
    let authority = AuthenticatedProtocolAuthority::for_test(ProtocolVersion::V2, 1);
    let mut gate = ProtocolSubmissionGate::default();
    assert!(gate.can_activate(authority));
    assert!(gate.permit().is_none());

    let failed = false;
    if failed {
        gate.activate(authority);
    }
    assert!(gate.permit().is_none());

    gate.activate(authority);
    let permit = gate
        .permit()
        .expect("successful activation publishes permit");
    assert!(gate.matches(permit));
    gate.quarantine(authority);
    assert!(!gate.matches(permit));
    assert!(
        !gate.can_activate(authority),
        "a quarantined generation requires a full refresh"
    );

    let fresh = AuthenticatedProtocolAuthority::for_test(ProtocolVersion::V2, 2);
    assert!(gate.can_activate(fresh));
    gate.activate(fresh);
    let fresh_permit = gate.permit().expect("fresh permit");
    gate.mark_indeterminate(fresh);
    assert!(gate.is_indeterminate());
    assert!(!gate.matches(fresh_permit));
    gate.close();
    assert!(
        gate.is_indeterminate(),
        "generic revocation must not erase an indeterminate submit"
    );
    assert!(!gate.can_activate(fresh));
    assert!(gate.can_activate(AuthenticatedProtocolAuthority::for_test(
        ProtocolVersion::V2,
        3,
    )));
}

#[test]
fn dropped_or_server_failed_post_is_ambiguous_but_definite_rejection_is_not() {
    use polymarket::error::{Kind, Method, StatusCode};

    let dropped = polymarket::error::Error::with_source(
        Kind::Internal,
        std::io::Error::new(std::io::ErrorKind::ConnectionReset, "synthetic drop"),
    );
    let failed = polymarket::error::Error::status(
        StatusCode::BAD_GATEWAY,
        Method::POST,
        "/order".to_owned(),
        "synthetic server failure",
    );
    let rejected = polymarket::error::Error::status(
        StatusCode::BAD_REQUEST,
        Method::POST,
        "/order".to_owned(),
        "synthetic rejection",
    );
    assert!(post_response_is_ambiguous(&dropped));
    assert!(post_response_is_ambiguous(&failed));
    assert!(!post_response_is_ambiguous(&rejected));
}

#[test]
fn submit_error_classification_discards_sdk_body_path_and_identifiers() {
    let sdk_error = polymarket::error::Error::status(
        polymarket::error::StatusCode::BAD_REQUEST,
        polymarket::error::Method::POST,
        "/order/private-order-id".to_owned(),
        "secret-body token=private-token no orders found to match with FAK order",
    );
    let classified = classify_operation_error(&sdk_error);
    assert_eq!(classified, crate::ClobOperationError::NoMatch);
    let rendered = format!("{classified:?} {classified}");
    for secret in [
        "secret-body",
        "private-order-id",
        "private-token",
        "/order/",
    ] {
        assert!(
            !rendered.contains(secret),
            "operation error leaked {secret}"
        );
    }
}

#[test]
fn stale_domain_balance_zero_revokes_protocol_authority_without_leaking_body() {
    let sdk_error = polymarket::error::Error::status(
        polymarket::error::StatusCode::BAD_REQUEST,
        polymarket::error::Method::POST,
        "/order/private-order-id".to_owned(),
        "not enough balance / allowance: balance: 0 private-token",
    );
    let status = sdk_error
        .downcast_ref::<polymarket::error::Status>()
        .expect("status error");
    assert!(status_revokes_protocol_authority(status));
    let classified = classify_operation_error(&sdk_error);
    assert_eq!(
        classified,
        crate::ClobOperationError::ProtocolAuthorityRevoked
    );
    let rendered = format!("{sdk_error:?} {sdk_error} {classified:?} {classified}");
    for secret in [
        "private-order-id",
        "private-token",
        "not enough balance",
        "balance: 0",
    ] {
        assert!(
            !rendered.contains(secret),
            "authority error leaked {secret}"
        );
    }
}

#[test]
fn protocol_version_retry_diagnostic_discards_provider_error() {
    let hostile =
        "Bearer api-key raw-body https://secret.invalid/order/private-order-id".to_owned();
    let rendered = protocol_version_retry_error_class(&hostile);
    assert_eq!(rendered, "clob_protocol_version_retry");
    for secret in [
        "Bearer",
        "api-key",
        "raw-body",
        "secret.invalid",
        "private-order-id",
    ] {
        assert!(!rendered.contains(secret));
    }
}

#[tokio::test]
async fn revoked_attempt_authority_prevents_the_first_post() {
    let post_attempts = Arc::new(AtomicUsize::new(0));
    let attempts_for_post = Arc::clone(&post_attempts);
    let result = retry_post_order(
        "revoked_before_first_attempt",
        crate::retry::RetryPolicy {
            max_attempts: 2,
            base_backoff: core::time::Duration::from_millis(10),
            max_backoff: core::time::Duration::from_millis(10),
        },
        || core::future::ready(None::<SubmissionAttemptAuthority>),
        || true,
        move |_authority: SubmissionAttemptAuthority| {
            attempts_for_post.fetch_add(1, Ordering::AcqRel);
            async { Err::<(), _>(crate::ClobOperationError::RateLimited { retry_after: None }) }
        },
    )
    .await;

    assert_eq!(result, Err(crate::ClobOperationError::SubmissionRevoked));
    assert_eq!(post_attempts.load(Ordering::Acquire), 0);
}

#[tokio::test]
async fn revocation_during_retry_backoff_prevents_a_second_post() {
    let post_attempts = Arc::new(AtomicUsize::new(0));
    let (first_post_tx, mut first_post_rx) = tokio::sync::mpsc::unbounded_channel();

    // A real controller/authority pair, exercising the exact `authorize_if`
    // path production callers use, rather than a synthetic stand-in - the
    // capability's fields are private outside `submission`, so this is the
    // only way to hand `retry_post_order` a genuine `SubmissionAttemptAuthority`.
    let (_binding, controller) = controller_pair().expect("controller identity");
    let protocol_authority = AuthenticatedProtocolAuthority::for_test(ProtocolVersion::V2, 1);
    let epoch = controller.revocation_epoch();
    let exclusive = controller.acquire_exclusive().await;
    assert!(exclusive.activate(epoch, protocol_authority));
    drop(exclusive);

    let controller_for_task = controller.clone();
    let attempts_for_task = Arc::clone(&post_attempts);
    let submit = tokio::spawn(async move {
        retry_post_order(
            "revoked_during_backoff",
            crate::retry::RetryPolicy {
                max_attempts: 2,
                base_backoff: core::time::Duration::from_millis(50),
                max_backoff: core::time::Duration::from_millis(50),
            },
            move || {
                let controller = controller_for_task.clone();
                async move { controller.authorize_if(protocol_authority, || true).await }
            },
            || true,
            move |_authority: SubmissionAttemptAuthority| {
                let attempt = attempts_for_task.fetch_add(1, Ordering::AcqRel) + 1;
                if attempt == 1 {
                    assert!(first_post_tx.send(()).is_ok());
                }
                async { Err::<(), _>(crate::ClobOperationError::RateLimited { retry_after: None }) }
            },
        )
        .await
    });

    first_post_rx
        .recv()
        .await
        .expect("first simulated POST reached the retry boundary");
    // Real revocation - `authorize_if` naturally denies every attempt after
    // this, since `request_revocation` clears the active authority.
    controller.request_revocation();
    let result = submit.await.expect("retry task joined");

    assert_eq!(result, Err(crate::ClobOperationError::SubmissionRevoked));
    assert_eq!(post_attempts.load(Ordering::Acquire), 1);
}
