use crate::auth::{
    AuthenticatedEndpoint, AuthenticatedEndpointError, RequestFailureClass,
    authenticated_endpoint_is_safe, consts::*, l2_headers,
};
use crate::utils::unix_timestamp;
use core::time::Duration;
use polymarket::{
    auth::{Normal, state::Authenticated},
    clob::Client,
};
use reqwest::header::HeaderMap;

/// Shared authenticated recovery transport.
#[derive(Clone, Debug)]
pub struct AuthenticatedHttpClient {
    http: reqwest::Client,
    pub(crate) max_attempts: usize,
    pub(crate) request_timeout: Duration,
}

impl AuthenticatedHttpClient {
    pub fn try_new() -> Result<Self, reqwest::Error> {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self::from_http_client(http))
    }

    pub fn from_http_client(http: reqwest::Client) -> Self {
        Self {
            http,
            request_timeout: REQUEST_TIMEOUT,
            max_attempts: MAX_ATTEMPTS,
        }
    }

    pub fn with_max_attempts(self, max_attempts: usize) -> Self {
        Self {
            max_attempts,
            ..self
        }
    }

    pub fn with_request_timeout(self, timeout: Duration) -> Self {
        Self {
            request_timeout: timeout,
            ..self
        }
    }

    pub(crate) async fn get(
        &self,
        client: &Client<Authenticated<Normal>>,
        endpoint: AuthenticatedEndpoint,
        path: &str,
        query: Option<&str>,
    ) -> Result<Vec<u8>, AuthenticatedEndpointError> {
        if !authenticated_endpoint_is_safe(client.host()) {
            return Err(AuthenticatedEndpointError::request_failed(endpoint));
        }
        let timestamp =
            unix_timestamp().ok_or_else(|| AuthenticatedEndpointError::request_failed(endpoint))?;
        let headers = l2_headers(client.address(), client.credentials(), timestamp, path)
            .ok_or_else(|| AuthenticatedEndpointError::request_failed(endpoint))?;
        self.send(client, endpoint, path, query, Some(headers))
            .await
    }

    pub(crate) async fn get_public(
        &self,
        client: &Client<Authenticated<Normal>>,
        endpoint: AuthenticatedEndpoint,
        path: &str,
    ) -> Result<Vec<u8>, crate::auth::AuthenticatedEndpointError> {
        if !authenticated_endpoint_is_safe(client.host()) {
            return Err(AuthenticatedEndpointError::request_failed(endpoint));
        }
        self.send(client, endpoint, path, None, None).await
    }

    async fn send(
        &self,
        client: &Client<Authenticated<Normal>>,
        endpoint: AuthenticatedEndpoint,
        path: &str,
        query: Option<&str>,
        headers: Option<HeaderMap>,
    ) -> Result<Vec<u8>, crate::auth::AuthenticatedEndpointError> {
        let mut last_error = None;
        for _ in 0..self.max_attempts {
            match self
                .send_once(client, endpoint, path, query, headers.clone())
                .await
            {
                Ok(body) => return Ok(body),
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error.unwrap_or_else(|| AuthenticatedEndpointError::request_failed(endpoint)))
    }

    async fn send_once(
        &self,
        client: &Client<Authenticated<Normal>>,
        endpoint: AuthenticatedEndpoint,
        path: &str,
        query: Option<&str>,
        headers: Option<HeaderMap>,
    ) -> Result<Vec<u8>, AuthenticatedEndpointError> {
        let mut url = client.host().clone();
        url.set_path(path);
        url.set_query(query.filter(|query| !query.is_empty()));
        let mut request = self.http.get(url).timeout(self.request_timeout);
        if let Some(headers) = headers {
            request = request.headers(headers);
        }
        let mut response = request.send().await.map_err(|_| {
            AuthenticatedEndpointError::request_failed_as(endpoint, RequestFailureClass::Transport)
        })?;
        // Split deliberately: a non-success status and an over-cap response are
        // different failures with different remedies (rotate a credential vs.
        // raise a byte cap), and folding them into one detail-free error is what
        // made a live-blocking 4xx unreadable in production. Only the status
        // integer is consulted; the body is never read on this path.
        let status = response.status();
        if !status.is_success() {
            return Err(AuthenticatedEndpointError::request_failed_for_status(
                endpoint,
                status.as_u16(),
            ));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
        {
            return Err(AuthenticatedEndpointError::request_failed_as(
                endpoint,
                RequestFailureClass::OversizedResponse,
            ));
        }

        let mut body = Vec::with_capacity(
            response
                .content_length()
                .and_then(|length| usize::try_from(length).ok())
                .unwrap_or_default(),
        );
        while let Some(chunk) = response.chunk().await.map_err(|_| {
            AuthenticatedEndpointError::request_failed_as(endpoint, RequestFailureClass::Transport)
        })? {
            if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                return Err(AuthenticatedEndpointError::request_failed_as(
                    endpoint,
                    RequestFailureClass::OversizedResponse,
                ));
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }
}
