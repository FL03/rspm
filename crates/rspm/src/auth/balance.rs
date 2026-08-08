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
