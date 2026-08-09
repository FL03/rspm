/*
    Appellation: balance <module>
    Created At: 2026.08.08:06:21:09
    Contrib: @FL03
*/
//! Exact authenticated collateral and allowance recovery.
use crate::auth::{
    AuthenticatedEndpoint, AuthenticatedEndpointError, AuthenticatedHttpClient,
    BALANCE_ALLOWANCE_PATH, BALANCE_ALLOWANCE_UPDATE_PATH, VERSION_PATH, scale_collateral,
    select_required_snapshot,
};
use crate::types::{ProtocolVersion, ProtocolVersionResponse, RawBalanceAllowanceResponse};
use core::fmt;
use hashbrown::HashMap;
use polymarket::auth::{Credentials, ExposeSecret, Normal, state::Authenticated};
use polymarket::clob::{
    Client,
    types::{SignatureType, response::BalanceAllowanceResponse},
};
use polymarket::types::{Address, Decimal, U256};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Default)]
pub(crate) struct AuthenticatedBalanceClient;

impl AuthenticatedBalanceClient {
    fn identity_is_exact(
        client: &Client<Authenticated<Normal>>,
        account_address: Address,
        signature_type: SignatureType,
    ) -> bool {
        match signature_type {
            SignatureType::Eoa => account_address == client.address(),
            SignatureType::Proxy => account_address != Address::ZERO,
            _ => false,
        }
    }

    pub(super) fn query(signature_type: SignatureType) -> String {
        format!(
            "asset_type=COLLATERAL&signature_type={}",
            signature_type as u8
        )
    }

    pub(crate) async fn fetch(
        &self,
        http: &AuthenticatedHttpClient,
        client: &Client<Authenticated<Normal>>,
        account_address: Address,
        signature_type: SignatureType,
        protocol_authority: AuthenticatedProtocolAuthority,
    ) -> Result<AuthenticatedBalanceSnapshot, AuthenticatedEndpointError> {
        if !Self::identity_is_exact(client, account_address, signature_type) {
            return Err(AuthenticatedEndpointError::request_failed(
                AuthenticatedEndpoint::BalanceAllowance,
            ));
        }
        let query = Self::query(signature_type);
        let body = http
            .get(
                client,
                AuthenticatedEndpoint::BalanceAllowance,
                BALANCE_ALLOWANCE_PATH,
                Some(&query),
            )
            .await?;
        let raw: RawBalanceAllowanceResponse =
            AuthenticatedEndpoint::BalanceAllowance.decode(&body)?;
        select_required_snapshot(raw, protocol_authority)
    }

    pub(crate) async fn protocol_version(
        &self,
        http: &AuthenticatedHttpClient,
        client: &Client<Authenticated<Normal>>,
    ) -> Result<ProtocolVersion, crate::Error> {
        let version_body = http
            .get_public(
                client,
                crate::auth::AuthenticatedEndpoint::ProtocolVersion,
                VERSION_PATH,
            )
            .await?;
        let version: ProtocolVersionResponse =
            AuthenticatedEndpoint::ProtocolVersion.decode(&version_body)?;
        ProtocolVersion::try_from(version.version)
    }

    pub(crate) async fn refresh(
        &self,
        http: &AuthenticatedHttpClient,
        client: &Client<Authenticated<Normal>>,
        account_address: Address,
        signature_type: SignatureType,
    ) -> Result<(), crate::auth::AuthenticatedEndpointError> {
        if !Self::identity_is_exact(client, account_address, signature_type) {
            return Err(crate::auth::AuthenticatedEndpointError::request_failed(
                crate::auth::AuthenticatedEndpoint::BalanceAllowance,
            ));
        }
        http.get(
            client,
            crate::auth::AuthenticatedEndpoint::BalanceAllowance,
            BALANCE_ALLOWANCE_UPDATE_PATH,
            Some(&Self::query(signature_type)),
        )
        .await
        .map(|_| ())
    }
}

/// Stable non-secret identity for one exact CLOB credential tuple.
///
/// The one-way value binds REST and private-WS recovery without retaining or
/// rendering the API key, secret, or passphrase. Its byte representation is
/// deliberately private and `Debug` is closed so it cannot become a bearer
/// credential substitute in logs or diagnostics.
#[derive(Clone, Copy, Default, Eq, Hash, PartialEq)]
pub struct AuthenticatedCredentialIdentity([u8; 32]);

impl core::fmt::Debug for AuthenticatedCredentialIdentity {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str("AuthenticatedCredentialIdentity([redacted])")
    }
}

impl AuthenticatedCredentialIdentity {
    pub(crate) fn from_credentials(credentials: &Credentials) -> Self {
        let key = credentials.key().to_string();
        let secret = credentials.secret().expose_secret();
        let passphrase = credentials.passphrase().expose_secret();
        let mut digest = Sha256::new();
        digest.update(b"rspm-authenticated-credential-identity-v1\0");
        for component in [key.as_bytes(), secret.as_bytes(), passphrase.as_bytes()] {
            digest.update((component.len() as u64).to_be_bytes());
            digest.update(component);
        }
        Self(digest.finalize().into())
    }

    /// Construct a deterministic non-secret identity for downstream hostile
    /// authority tests without introducing credential-shaped fixture data.
    #[cfg(any(test, feature = "test-utils"))]
    #[doc(hidden)]
    #[must_use]
    pub fn for_test(label: &str) -> Self {
        let mut digest = Sha256::new();
        digest.update(b"rspm-authenticated-credential-test-v1\0");
        digest.update(label.as_bytes());
        Self(digest.finalize().into())
    }
}

/// One credential identity installed for one monotonic recovery generation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthenticatedCredentialAuthority {
    identity: AuthenticatedCredentialIdentity,
    generation: u64,
}

impl AuthenticatedCredentialAuthority {
    pub(crate) fn new(
        identity: AuthenticatedCredentialIdentity,
        generation: u64,
    ) -> Result<Self, crate::auth::AuthenticatedEndpointError> {
        if generation == 0 {
            return Err(crate::auth::AuthenticatedEndpointError::request_failed(
                crate::auth::AuthenticatedEndpoint::ProtocolVersion,
            ));
        }
        Ok(Self {
            identity,
            generation,
        })
    }

    /// Non-secret identity shared by authenticated REST and private WS.
    #[must_use]
    pub const fn identity(self) -> AuthenticatedCredentialIdentity {
        self.identity
    }

    /// Monotonic SDK/recovery generation carrying these credentials.
    #[must_use]
    pub const fn generation(self) -> u64 {
        self.generation
    }
}

/// One locally installed SDK protocol contract and credential generation.
///
/// `generation` changes whenever rspm replaces the authenticated SDK client;
/// `version` is the exact `/version` value resolved before that replacement is
/// published.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthenticatedProtocolAuthority {
    version: ProtocolVersion,
    credentials: AuthenticatedCredentialAuthority,
}

/// Non-authorizing projection used to partition caches by installed protocol.
///
/// Credential identity is deliberately absent, and this type cannot be
/// converted back into submission authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthenticatedProtocolCacheIdentity {
    version: ProtocolVersion,
    generation: u64,
}

impl AuthenticatedProtocolCacheIdentity {
    #[must_use]
    pub const fn version(self) -> ProtocolVersion {
        self.version
    }

    #[must_use]
    pub const fn generation(self) -> u64 {
        self.generation
    }
}

impl AuthenticatedProtocolAuthority {
    pub(crate) fn new(
        version: ProtocolVersion,
        generation: u64,
        credential_identity: AuthenticatedCredentialIdentity,
    ) -> Result<Self, crate::auth::AuthenticatedEndpointError> {
        Ok(Self {
            version,
            credentials: AuthenticatedCredentialAuthority::new(credential_identity, generation)?,
        })
    }

    /// Construct deterministic protocol authority for downstream tests.
    #[cfg(any(test, feature = "test-utils"))]
    #[doc(hidden)]
    #[must_use]
    pub fn for_test(version: ProtocolVersion, generation: u64) -> Self {
        Self::for_test_with_credential(version, generation, "default")
    }

    /// Construct deterministic protocol and credential authority for hostile
    /// cross-credential integration tests.
    #[cfg(any(test, feature = "test-utils"))]
    #[doc(hidden)]
    #[must_use]
    pub fn for_test_with_credential(
        version: ProtocolVersion,
        generation: u64,
        credential_label: &str,
    ) -> Self {
        Self::new(
            version,
            generation,
            AuthenticatedCredentialIdentity::for_test(credential_label),
        )
        .expect("test protocol generation must be positive")
    }

    /// Exact protocol contract version bound to this authority.
    #[must_use]
    pub const fn version(self) -> ProtocolVersion {
        self.version
    }

    /// Monotonic local SDK replacement generation.
    #[must_use]
    pub const fn generation(self) -> u64 {
        self.credentials.generation()
    }

    /// Exact non-secret credential identity and generation bound to REST.
    #[must_use]
    pub const fn credential_authority(self) -> AuthenticatedCredentialAuthority {
        self.credentials
    }

    /// Project this authority into a non-authorizing cache identity.
    #[must_use]
    pub const fn cache_identity(self) -> AuthenticatedProtocolCacheIdentity {
        AuthenticatedProtocolCacheIdentity {
            version: self.version,
            generation: self.credentials.generation(),
        }
    }
}

/// A canonical, exact account-capital snapshot.
///
/// Values stay in collateral base units until the binding minimum is known.
/// Authority addresses are retained only to make snapshot equality exact;
/// neither `Debug` nor the public accessors disclose them.
#[derive(Clone, Eq, PartialEq)]
pub struct AuthenticatedBalanceSnapshot {
    protocol_authority: AuthenticatedProtocolAuthority,
    balance_raw: U256,
    required_allowances_raw: HashMap<Address, U256>,
    executable_capital: Decimal,
}

impl fmt::Debug for AuthenticatedBalanceSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthenticatedBalanceSnapshot")
            .field("protocol_authority", &self.protocol_authority)
            .field(
                "required_allowance_count",
                &self.required_allowances_raw.len(),
            )
            .field("executable_capital", &self.executable_capital)
            .finish_non_exhaustive()
    }
}

impl AuthenticatedBalanceSnapshot {
    /// Construct a snapshot from an already-resolved complete spender set.
    pub fn try_from_required_raw(
        protocol_authority: AuthenticatedProtocolAuthority,
        balance_raw: U256,
        required_allowances_raw: HashMap<Address, U256>,
    ) -> Result<Self, crate::auth::AuthenticatedEndpointError> {
        if required_allowances_raw.is_empty() {
            return Err(
                crate::auth::AuthenticatedEndpointError::response_schema_decode(
                    crate::auth::AuthenticatedEndpoint::BalanceAllowance,
                    "allowances",
                ),
            );
        }
        let binding_raw = required_allowances_raw
            .values()
            .copied()
            .fold(balance_raw, U256::min);
        let executable_capital = scale_collateral(binding_raw)?;
        Ok(Self {
            protocol_authority,
            balance_raw,
            required_allowances_raw,
            executable_capital,
        })
    }

    /// Exact SDK protocol version and replacement generation used to select
    /// the required spender set for this snapshot.
    #[must_use]
    pub const fn protocol_authority(&self) -> AuthenticatedProtocolAuthority {
        self.protocol_authority
    }

    /// Non-authorizing protocol identity suitable for cache partitioning.
    #[must_use]
    pub const fn protocol_cache_identity(&self) -> AuthenticatedProtocolCacheIdentity {
        self.protocol_authority.cache_identity()
    }

    /// Exact executable capital after every required authority is applied.
    pub const fn executable_capital(
        &self,
    ) -> Result<Decimal, crate::auth::AuthenticatedEndpointError> {
        Ok(self.executable_capital)
    }

    /// Exact venue balance in collateral base units.
    #[must_use]
    pub const fn balance_raw(&self) -> U256 {
        self.balance_raw
    }

    /// Exact venue balance in collateral units when it fits `Decimal`.
    pub fn balance(&self) -> Result<Decimal, crate::auth::AuthenticatedEndpointError> {
        scale_collateral(self.balance_raw)
    }

    /// Binding required allowance in base units, or `None` without a complete set.
    #[must_use]
    pub fn binding_allowance_raw(&self) -> Option<U256> {
        self.required_allowances_raw.values().copied().min()
    }

    /// Exact binding required allowance when present and `Decimal`-bounded.
    pub fn binding_allowance(
        &self,
    ) -> Result<Option<Decimal>, crate::auth::AuthenticatedEndpointError> {
        self.binding_allowance_raw()
            .map(scale_collateral)
            .transpose()
    }

    /// Number of required spender allowances in this snapshot.
    #[must_use]
    pub fn required_allowance_count(&self) -> usize {
        self.required_allowances_raw.len()
    }

    /// Project the rspm-owned, strictly decoded raw snapshot into the legacy
    /// SDK-shaped response still consumed by node status/read paths.
    pub(crate) fn sdk_compatibility_response(
        &self,
    ) -> Result<BalanceAllowanceResponse, crate::auth::AuthenticatedEndpointError> {
        let balance = Decimal::from_str_exact(&self.balance_raw.to_string()).map_err(|_| {
            crate::auth::AuthenticatedEndpointError::response_schema_decode(
                crate::auth::AuthenticatedEndpoint::BalanceAllowance,
                "balance",
            )
        })?;
        let allowances = self
            .required_allowances_raw
            .iter()
            .map(|(address, allowance)| (*address, allowance.to_string()))
            .collect();
        Ok(BalanceAllowanceResponse::builder()
            .balance(balance)
            .allowances(allowances)
            .build())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{auth::required_spenders, decode_json};
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
        let [first, second, third] =
            required_spenders(ProtocolVersion::V2).expect("Polygon V2 config");
        let extra = address!("0x4444444444444444444444444444444444444444");
        let raw = RawBalanceAllowanceResponse {
            balance: U256::from(20_000_000_u64),
            allowances: HashMap::from([
                (first, U256::MAX),
                (second, U256::MAX),
                (extra, U256::MAX),
            ]),
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
}
