/*
    Appellation: auth <module>
    Created At: 2026.08.08:02:42:02
    Contrib: @FL03
*/
#[doc(inline)]
pub use self::prelude::*;

pub mod error;
#[cfg(feature = "ws")]
pub mod ws;

mod balance;
mod client;
mod consts;
mod orders;
mod trades;

mod types {
    #[doc(inline)]
    pub use self::prelude::*;

    mod endpoint;

    mod prelude {
        pub use super::endpoint::*;
    }
}

mod utils {
    #[doc(inline)]
    pub use self::prelude::*;

    mod helpers;
    mod schema;
    mod transport;

    mod prelude {
        pub use super::helpers::*;
        pub use super::schema::*;
        pub use super::transport::*;
    }
}

pub mod prelude {
    pub use super::balance::*;
    pub use super::client::*;
    pub(crate) use super::consts::*;
    pub use super::error::*;
    pub use super::orders::*;
    pub use super::trades::*;
    pub use super::types::*;
    pub use super::utils::*;
    #[cfg(feature = "ws")]
    pub use super::ws::prelude::*;
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::str::FromStr;
    use core::time::Duration;
    use polymarket::{
        POLYGON,
        auth::{Credentials, LocalSigner, Normal, Signer, Uuid, state::Authenticated},
        clob::{Client, Config},
        types::Address,
    };
    use std::sync::{Arc, Mutex};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    fn test_credentials() -> Credentials {
        Credentials::new(
            Uuid::nil(),
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".to_owned(),
            "test-only-passphrase".to_owned(),
        )
    }

    async fn test_client(endpoint: &str) -> Client<Authenticated<Normal>> {
        let signer = LocalSigner::from_str(
            "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d",
        )
        .expect("test signer")
        .with_chain_id(Some(POLYGON));
        Client::new(endpoint, Config::default())
            .expect("test endpoint")
            .authentication_builder(&signer)
            .credentials(test_credentials())
            .authenticate()
            .await
            .expect("provided test credentials avoid auth network")
    }

    #[derive(Clone, Copy)]
    enum FirstFailure {
        Status,
        Truncated,
        Timeout,
    }

    async fn assert_same_request_retry(first_failure: FirstFailure) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let endpoint = format!("http://{}", listener.local_addr().expect("address"));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&requests);
        let server = tokio::spawn(async move {
            for attempt in 0..2 {
                let (mut socket, _) = listener.accept().await.expect("accept");
                let captured = Arc::clone(&captured);
                tokio::spawn(async move {
                    let mut bytes = vec![0_u8; 8_192];
                    let length = socket.read(&mut bytes).await.expect("read request");
                    captured
                        .lock()
                        .expect("capture lock")
                        .push(String::from_utf8_lossy(&bytes[..length]).into_owned());
                    if attempt == 0 {
                        match first_failure {
                            FirstFailure::Status => {
                                socket
                                    .write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n")
                                    .await
                                    .expect("status response");
                            }
                            FirstFailure::Truncated => {
                                socket
                                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\n\r\nx")
                                    .await
                                    .expect("truncated response");
                            }
                            FirstFailure::Timeout => {
                                tokio::time::sleep(Duration::from_millis(100)).await;
                            }
                        }
                    } else {
                        socket
                            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                            .await
                            .expect("success response");
                    }
                });
            }
        });

        let transport = AuthenticatedHttpClient::try_new()
            .expect("test HTTP transport")
            .with_request_timeout(Duration::from_millis(25));
        let client = test_client(&endpoint).await;
        let body = transport
            .get(
                &client,
                crate::auth::AuthenticatedEndpoint::Trades,
                "/data/trades",
                Some("after=10&next_cursor=MTAw"),
            )
            .await
            .expect("second attempt succeeds");
        assert_eq!(body, b"ok");
        server.await.expect("server");
        let requests = requests.lock().expect("capture lock");
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[0], requests[1],
            "retry request must be byte-identical"
        );
    }

    #[test]
    fn l2_signature_excludes_query_and_binds_exact_path() {
        assert_eq!(signature_material(1, "/data/trades"), "1GET/data/trades");
        assert_ne!(
            l2_signature(&test_credentials(), 1, "/data/trades"),
            l2_signature(&test_credentials(), 1, "/data/orders")
        );
        let dynamic =
            authenticated_order_path("0xabc_DEF-123").expect("validated terminal order path");
        assert_eq!(
            signature_material(1, &dynamic),
            "1GET/data/order/0xabc_DEF-123"
        );
        assert_ne!(
            l2_signature(&test_credentials(), 1, &dynamic),
            l2_signature(&test_credentials(), 1, "/data/orders")
        );
    }

    /// [REGRESSION][EVAL] External official-SDK vector. This prevents the
    /// owned signing path and its tests from agreeing on the same wrong
    /// preimage, secret decoding, or URL-safe base64 representation.
    #[test]
    fn l2_signature_matches_official_sdk_vector() {
        assert_eq!(
            l2_signature(&test_credentials(), 1, "/").as_deref(),
            Some("eHaylCwqRSOa2LFD77Nt_SaTpbsxzN8eTEI3LryhEj4=")
        );
    }

    #[test]
    fn every_identity_or_secret_header_is_debug_redacted() {
        let headers = l2_headers(Address::ZERO, &test_credentials(), 1, "/balance-allowance")
            .expect("synthetic credentials are valid headers");

        assert_eq!(headers.len(), 5);
        let debug = format!("{headers:?}");
        let signature =
            l2_signature(&test_credentials(), 1, "/balance-allowance").expect("signature");
        for forbidden in [
            "0x0000000000000000000000000000000000000000",
            "test-only-passphrase",
            "00000000-0000-0000-0000-000000000000",
            signature.as_str(),
        ] {
            assert!(!debug.contains(forbidden), "sensitive header leaked");
        }
    }

    #[test]
    fn authenticated_http_accepts_only_canonical_authority_or_test_loopback() {
        for accepted in [
            "https://clob.polymarket.com",
            "http://localhost:8080",
            "http://127.0.0.1:8080",
            "http://[::1]:8080",
        ] {
            let endpoint = url::Url::parse(accepted).expect("fixture URL");
            assert!(authenticated_endpoint_is_safe(&endpoint), "{accepted}");
        }
        for rejected in [
            "https://attacker.invalid",
            "https://clob.polymarket.com:444",
            "https://clob.polymarket.com/api",
            "http://clob.polymarket.com",
            "http://example.invalid",
            "https://user@example.invalid",
            "https://example.invalid?forward=secret",
            "https://example.invalid/#fragment",
            "ftp://example.invalid",
        ] {
            let endpoint = url::Url::parse(rejected).expect("fixture URL");
            assert!(!authenticated_endpoint_is_safe(&endpoint), "{rejected}");
        }
    }

    #[tokio::test]
    async fn status_timeout_and_truncated_body_retry_the_exact_same_page() {
        for failure in [
            FirstFailure::Status,
            FirstFailure::Truncated,
            FirstFailure::Timeout,
        ] {
            assert_same_request_retry(failure).await;
        }
    }

    #[tokio::test]
    async fn redirects_never_forward_authenticated_headers() {
        let sink = TcpListener::bind("127.0.0.1:0").await.expect("sink bind");
        let sink_address = sink.local_addr().expect("sink address");
        let redirect = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("redirect bind");
        let endpoint = format!(
            "http://{}",
            redirect.local_addr().expect("redirect address")
        );
        let redirect_server = tokio::spawn(async move {
            for _ in 0..MAX_ATTEMPTS {
                let (mut socket, _) = redirect.accept().await.expect("redirect accept");
                let mut request = [0_u8; 8_192];
                let bytes_read = socket.read(&mut request).await.expect("redirect request");
                assert!(bytes_read > 0, "redirect request must not be empty");
                let response = format!(
                    "HTTP/1.1 302 Found\r\nLocation: http://{sink_address}/sink\r\nContent-Length: 0\r\n\r\n"
                );
                socket
                    .write_all(response.as_bytes())
                    .await
                    .expect("redirect response");
            }
        });

        let client = test_client(&endpoint).await;
        let error = AuthenticatedHttpClient::try_new()
            .expect("test HTTP transport")
            .get(
                &client,
                crate::auth::AuthenticatedEndpoint::Trades,
                "/data/trades",
                None,
            )
            .await
            .expect_err("redirect status is not followed");
        assert_eq!(error.error_class(), "request_failed");
        redirect_server.await.expect("redirect server");
        assert!(
            tokio::time::timeout(Duration::from_millis(50), sink.accept())
                .await
                .is_err(),
            "redirect sink must receive no request"
        );
    }
}
