//! Credential subscription encoding and endpoint policy.

use polymarket::auth::{Credentials, ExposeSecret as _};
use serde::Serialize;

use super::{AuthenticatedUserWsError, USER_WS_HOST};

#[derive(Serialize)]
struct UserSubscription<'a> {
    #[serde(rename = "type")]
    channel: &'static str,
    auth: UserAuthentication<'a>,
}

#[derive(Serialize)]
pub(super) struct UserAuthentication<'a> {
    #[serde(rename = "apiKey")]
    pub(super) api_key: String,
    pub(super) secret: &'a str,
    pub(super) passphrase: &'a str,
}

impl core::fmt::Debug for UserAuthentication<'_> {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("UserAuthentication")
            .field("api_key", &"<redacted>")
            .field("secret", &"<redacted>")
            .field("passphrase", &"<redacted>")
            .finish()
    }
}

pub(super) fn subscription_payload(
    credentials: &Credentials,
) -> Result<String, AuthenticatedUserWsError> {
    serde_json::to_string(&UserSubscription {
        channel: "user",
        auth: UserAuthentication {
            api_key: credentials.key().to_string(),
            secret: credentials.secret().expose_secret(),
            passphrase: credentials.passphrase().expose_secret(),
        },
    })
    .map_err(|_| AuthenticatedUserWsError::SubscriptionEncoding)
}

fn endpoint_host_is_literal_loopback(endpoint: &url::Url) -> bool {
    endpoint.host().is_some_and(|host| match host {
        url::Host::Domain(_) => false,
        url::Host::Ipv4(address) => address.is_loopback(),
        url::Host::Ipv6(address) => address.is_loopback(),
    })
}

pub(super) fn user_endpoint(endpoint: &str) -> Result<String, AuthenticatedUserWsError> {
    let trimmed = endpoint.trim().trim_end_matches('/');
    if trimmed.ends_with("/ws/market") {
        return Err(AuthenticatedUserWsError::InvalidEndpoint);
    }
    let base = trimmed
        .strip_suffix("/ws/user")
        .or_else(|| trimmed.strip_suffix("/ws"))
        .unwrap_or(trimmed);
    let endpoint = format!("{base}/ws/user");
    let parsed =
        url::Url::parse(&endpoint).map_err(|_| AuthenticatedUserWsError::InvalidEndpoint)?;
    let loopback = endpoint_host_is_literal_loopback(&parsed);
    let transport_safe = parsed.scheme() == "wss" || (parsed.scheme() == "ws" && loopback);
    if !transport_safe
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.path() != "/ws/user"
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(AuthenticatedUserWsError::InvalidEndpoint);
    }
    Ok(endpoint)
}

pub(super) fn official_user_endpoint() -> Result<String, AuthenticatedUserWsError> {
    let endpoint = user_endpoint(USER_WS_HOST)?;
    let parsed =
        url::Url::parse(&endpoint).map_err(|_| AuthenticatedUserWsError::InvalidEndpoint)?;
    if parsed.scheme() != "wss"
        || parsed.host_str() != Some("ws-subscriptions-clob.polymarket.com")
        || parsed.port().is_some()
        || parsed.path() != "/ws/user"
    {
        return Err(AuthenticatedUserWsError::InvalidEndpoint);
    }
    Ok(endpoint)
}

#[cfg(any(test, feature = "test-utils"))]
pub(super) fn loopback_user_endpoint(endpoint: &str) -> Result<String, AuthenticatedUserWsError> {
    let endpoint = user_endpoint(endpoint)?;
    let parsed =
        url::Url::parse(&endpoint).map_err(|_| AuthenticatedUserWsError::InvalidEndpoint)?;
    if parsed.scheme() != "ws"
        || !endpoint_host_is_literal_loopback(&parsed)
        || parsed.port().is_none()
        || parsed.path() != "/ws/user"
    {
        return Err(AuthenticatedUserWsError::InvalidEndpoint);
    }
    Ok(endpoint)
}
