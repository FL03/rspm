/*
    Appellation: settings <module>
    Created At: 2026.07.12:08:20:24
    Contrib: @FL03
*/
#[cfg(feature = "alloc")]
use alloc::string::{String, ToString};

/// Configuration bundle for [`ClobClient`](crate::clob::ClobClient).
///
/// Build via [`ClobConfig::from_env`] (reads environment variables without
/// mutating them) or construct explicitly for testing.
///
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(default, rename_all = "snake_case")
)]
pub struct ClobConfig {
    /// CLOB API host, e.g. `"https://clob.polymarket.com"`.
    pub host: String,
    /// Optional SOCKS5 proxy URL (already normalised to `socks5h://` scheme for
    /// remote DNS resolution through the tunnel).
    ///
    /// When `Some`, callers must configure their HTTP client to honour this proxy.
    /// `ClobConfig` itself never mutates `ALL_PROXY` or any process-level state.
    pub proxy_url: Option<String>,
}

impl ClobConfig {
    /// Construct explicitly. `proxy_url` must already use the `socks5h://` scheme
    /// if remote DNS resolution through the proxy is required.
    pub fn new(host: impl ToString, proxy_url: Option<String>) -> Self {
        Self {
            host: host.to_string(),
            proxy_url,
        }
    }
    /// Read configuration from environment variables.
    ///
    /// | Variable     | Default                           |
    /// |--------------|-----------------------------------|
    /// | `CLOB_HOST`  | `https://clob.polymarket.com`     |
    /// | `CLOB_PROXY` | — (absent = no proxy)             |
    ///
    /// The proxy URL is normalised from `socks5://` → `socks5h://` here so
    /// that reqwest resolves hostnames remotely through the tunnel. This
    /// function never calls `std::env::set_var`.
    pub fn from_env() -> Result<Self, std::env::VarError> {
        let host = std::env::var("CLOB_HOST")?;
        let proxy_url = std::env::var("CLOB_PROXY")
            .map(|raw| raw.replace("socks5://", "socks5h://"))
            .ok();
        Ok(Self::new(host, proxy_url))
    }
    /// consumes the current instance to create another with the given host
    pub fn with_host<T>(self, host: T) -> Self
    where
        T: ToString,
    {
        Self {
            host: host.to_string(),
            ..self
        }
    }
    /// consumes the current instance to create another with the given proxy url
    pub fn with_proxy_url(self, proxy_url: impl ToString) -> Self {
        Self {
            proxy_url: Some(proxy_url.to_string()),
            ..self
        }
    }
    /// returns a reference to the configured host.
    pub fn host(&self) -> &str {
        &self.host
    }
    /// returns a reference to the optional proxy url configured for the clob client.
    pub fn proxy_url(&self) -> Option<&str> {
        self.proxy_url.as_deref()
    }

    pub(crate) fn validate(&self) -> crate::Result<()> {
        if !clob_host_is_safe(&self.host)
            || self
                .proxy_url
                .as_deref()
                .is_some_and(|proxy| !proxy_url_is_safe(proxy))
        {
            return Err(crate::Error::InvalidClobConfiguration);
        }
        Ok(())
    }

    /// Validate the credential-bearing CLOB authority.
    ///
    /// Public market metadata may use an explicitly configured HTTPS origin,
    /// but L1/L2 authentication material may only leave the process for the
    /// canonical Polymarket CLOB authority. Unit tests retain a loopback-only
    /// seam; no production build can opt another authenticated origin in via
    /// `CLOB_HOST`.
    pub(crate) fn validate_authenticated(&self) -> crate::Result<()> {
        if !authenticated_clob_host_is_safe(&self.host)
            || self
                .proxy_url
                .as_deref()
                .is_some_and(|proxy| !proxy_url_is_safe(proxy))
        {
            return Err(crate::Error::InvalidClobConfiguration);
        }
        Ok(())
    }

    pub(crate) fn host_class(&self) -> &'static str {
        host_class(&self.host)
    }

    pub fn set_host<T>(&mut self, host: T)
    where
        T: ToString,
    {
        self.host = host.to_string()
    }

    pub fn set_proxy_url<T>(&mut self, proxy_url: Option<T>)
    where
        T: ToString,
    {
        self.proxy_url = proxy_url.map(|i| i.to_string())
    }
}

impl core::fmt::Debug for ClobConfig {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("ClobConfig")
            .field("host_class", &self.host_class())
            .field("proxy_configured", &self.proxy_url.is_some())
            .finish()
    }
}

fn has_base_path(url: &url::Url) -> bool {
    matches!(url.path(), "" | "/")
}

fn has_no_ambient_url_data(url: &url::Url) -> bool {
    url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && has_base_path(url)
}

fn loopback_host(url: &url::Url) -> bool {
    url.host().is_some_and(|host| match host {
        url::Host::Domain(domain) => domain.eq_ignore_ascii_case("localhost"),
        url::Host::Ipv4(address) => address.is_loopback(),
        url::Host::Ipv6(address) => address.is_loopback(),
    })
}

fn clob_host_is_safe(raw: &str) -> bool {
    let Ok(url) = url::Url::parse(raw) else {
        return false;
    };
    if !has_no_ambient_url_data(&url) {
        return false;
    }
    match url.scheme() {
        "https" => url.host().is_some(),
        "http" => loopback_host(&url),
        _ => false,
    }
}

fn canonical_polymarket_clob_host(raw: &str) -> bool {
    raw == crate::clob::CLOB_HOST || raw == concat!("https://clob.polymarket.com", "/")
}

fn authenticated_clob_host_is_safe(raw: &str) -> bool {
    if canonical_polymarket_clob_host(raw) {
        return true;
    }
    #[cfg(any(test, feature = "test-utils"))]
    {
        let Ok(url) = url::Url::parse(raw) else {
            return false;
        };
        url.scheme() == "http" && has_no_ambient_url_data(&url) && loopback_host(&url)
    }
    #[cfg(not(any(test, feature = "test-utils")))]
    {
        false
    }
}

pub(crate) fn authenticated_clob_endpoint_is_safe(endpoint: &url::Url) -> bool {
    let canonical = endpoint.scheme() == "https"
        && endpoint
            .host_str()
            .is_some_and(|host| host == "clob.polymarket.com")
        && endpoint.port().is_none()
        && has_no_ambient_url_data(endpoint);
    if canonical {
        return true;
    }
    #[cfg(any(test, feature = "test-utils"))]
    {
        endpoint.scheme() == "http"
            && endpoint.port().is_some()
            && has_no_ambient_url_data(endpoint)
            && loopback_host(endpoint)
    }
    #[cfg(not(any(test, feature = "test-utils")))]
    {
        false
    }
}

fn proxy_url_is_safe(raw: &str) -> bool {
    let Ok(url) = url::Url::parse(raw) else {
        return false;
    };
    has_no_ambient_url_data(&url) && url.scheme() == "socks5h" && url.host().is_some()
}

fn host_class(raw: &str) -> &'static str {
    let Ok(url) = url::Url::parse(raw) else {
        return "invalid";
    };
    if !clob_host_is_safe(raw) {
        return "invalid";
    }
    if url.scheme() == "https"
        && url
            .host_str()
            .is_some_and(|host| host.eq_ignore_ascii_case("clob.polymarket.com"))
    {
        "polymarket_https"
    } else if url.scheme() == "https" {
        "https"
    } else {
        "loopback_http"
    }
}

impl Default for ClobConfig {
    fn default() -> Self {
        Self::from_env().unwrap_or(Self::new(crate::clob::CLOB_HOST, None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_renders_host_or_proxy_credentials() {
        let config = ClobConfig::new(
            "https://user:host-secret@example.invalid?token=host-secret",
            Some("socks5h://user:proxy-secret@proxy.invalid".to_owned()),
        );
        let rendered = format!("{config:?}");
        for secret in [
            "user",
            "host-secret",
            "example.invalid",
            "proxy-secret",
            "proxy.invalid",
        ] {
            assert!(!rendered.contains(secret), "Debug leaked {secret}");
        }
        assert!(rendered.contains("proxy_configured: true"));
    }

    #[test]
    fn authentication_urls_reject_embedded_authority_and_url_suffixes() {
        for host in [
            "https://user:pass@clob.polymarket.com",
            "https://clob.polymarket.com?token=secret",
            "https://clob.polymarket.com#secret",
            "http://clob.polymarket.com",
        ] {
            assert!(ClobConfig::new(host, None).validate().is_err(), "{host}");
        }
        for proxy in [
            "socks5h://user:pass@proxy.example",
            "socks5h://proxy.example?token=secret",
            "socks5h://proxy.example#secret",
        ] {
            assert!(
                ClobConfig::new("https://clob.polymarket.com", Some(proxy.to_owned()))
                    .validate()
                    .is_err(),
                "{proxy}"
            );
        }
        assert!(
            ClobConfig::new(
                "https://clob.polymarket.com",
                Some("socks5h://proxy.example:1080".to_owned()),
            )
            .validate()
            .is_ok()
        );
        assert!(
            ClobConfig::new("http://127.0.0.1:8080", None)
                .validate()
                .is_ok()
        );
    }

    #[test]
    fn authenticated_authority_is_exact_in_production_and_loopback_only_in_tests() {
        for host in [
            "https://attacker.invalid",
            "https://clob.polymarket.com:444",
            "https://clob.polymarket.com/api",
            "HTTPS://clob.polymarket.com",
        ] {
            assert!(
                ClobConfig::new(host, None)
                    .validate_authenticated()
                    .is_err(),
                "credential-bearing authority accepted {host}"
            );
        }
        for host in [
            "https://clob.polymarket.com",
            "https://clob.polymarket.com/",
            "http://127.0.0.1:8080",
            "http://[::1]:8080",
        ] {
            assert!(
                ClobConfig::new(host, None).validate_authenticated().is_ok(),
                "expected authenticated authority rejected {host}"
            );
        }
    }
}
