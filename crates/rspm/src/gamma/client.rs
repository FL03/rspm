/*
    Appellation: client <module>
    Created At: 2026.08.08:02:44:06
    Contrib: @FL03
*/
use crate::gamma::{GAMMA_BASE, market_row_from_gamma_value};
use crate::{error::Error, types::Market, types::PolymarketQdbRow};

/// Unauthenticated Gamma API client for market discovery.
pub struct GammaClient {
    pub(crate) base_url: String,
    pub(crate) http: reqwest::Client,
}

impl GammaClient {
    /// Construct a `GammaClient` pointed at the default Gamma API.
    pub fn new() -> Self {
        Self::from_url(GAMMA_BASE)
    }
    /// Construct a `GammaClient` pointed at an explicit Gamma API base URL.
    ///
    /// The caller owns configuration and validation. A trailing slash is
    /// removed so endpoint construction never emits a double slash.
    pub fn from_url<T>(base_url: T) -> Self
    where
        T: ToString,
    {
        let base_url = base_url.to_string();
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http: reqwest::Client::new(),
        }
    }

    pub fn with_base_url<T>(self, base_url: T) -> Self
    where
        T: ToString,
    {
        Self {
            base_url: base_url.to_string(),
            ..self
        }
    }

    /// Fetch a market by its URL slug.
    ///
    /// Returns `Err(PmError::NotFound)` if no market matches the slug.
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(level = "info", target = "rspm::gamma", skip_all, fields(slug))
    )]
    pub async fn get_market(&self, slug: &str) -> Result<Market, Error> {
        let url = format!("{}/markets?slug={}", self.base_url, slug);
        let markets: Vec<Market> = self.get_json(&url).await?;
        markets.into_iter().next().ok_or(Error::NotFound)
    }

    /// Search events by keyword via Gamma `q=` param.
    ///
    /// Returns raw event JSON objects (not market objects) because the `/events`
    /// endpoint nests markets under each event. Callers that need a flat list of
    /// markets should flatten `event["markets"]` from each returned element.
    ///
    /// Do NOT combine `q=` with `order=` when calling the Gamma API — the sort
    /// param suppresses the text filter and returns top-volume results instead.
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(level = "info", target = "rspm::gamma", skip_all, fields(query))
    )]
    pub async fn search_markets(&self, query: &str) -> Result<Vec<serde_json::Value>, Error> {
        // Use /events?q= — /markets?search= returns stale 2020 archived data.
        let encoded = query.replace(' ', "+");
        let url = format!(
            "{}/events?q={}&active=true&closed=false",
            self.base_url, encoded,
        );
        self.get_json(&url).await
    }

    /// Fetch markets filtered by active status.
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(level = "info", target = "rspm::gamma", skip_all)
    )]
    pub async fn get_active_markets(&self) -> Result<Vec<Market>, Error> {
        let url = format!("{}/markets?active=true&closed=false", self.base_url);
        self.get_json(&url).await
    }

    /// Fetch the order book summary for a CLOB token ID.
    ///
    /// Returns the raw JSON value since the book schema varies.
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(level = "info", target = "rspm::gamma", skip_all, fields(token_id))
    )]
    pub async fn get_book(&self, token_id: &str) -> Result<serde_json::Value, Error> {
        let url = format!("{}/books/{}", self.base_url, token_id);
        self.get_json(&url).await
    }

    /// Fetch a single market by slug and return it as a fully-populated
    /// [`PolymarketQdbRow`] row, including all 20 extended Gamma / FPMM
    /// fields required by the `polymarkets` DDL (#1114 + #1142).
    ///
    /// The caller must supply the `ts` (write timestamp, Unix epoch
    /// nanoseconds), `exchange`, `market_type`, `asset`, `market_id`,
    /// `scope`, `window_start`, `window_end`, `state`, and `outcome`
    /// fields — these come from the node's sprint-tracking logic and are
    /// not part of the Gamma response.  All other fields are populated from
    /// the Gamma JSON using [`market_row_from_gamma_value`].
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(level = "debug", target = "rspm::gamma", skip_all, fields(slug))
    )]
    pub async fn fetch_market_row(&self, slug: &str) -> Result<PolymarketQdbRow, Error> {
        let url = format!("{}/markets?slug={}", self.base_url, slug);
        let raw: Vec<serde_json::Value> = self.get_json(&url).await?;
        let value = raw.into_iter().next().ok_or(Error::NotFound)?;
        Ok(market_row_from_gamma_value(&value))
    }

    // ─── Internal ─────────────────────────────────────────────────────────────

    async fn get_json<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T, Error> {
        let resp = self
            .http
            .get(url)
            .header("Accept", "application/json")
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let msg = resp.text().await.unwrap_or_default();
            return Err(Error::http(status.as_u16(), msg));
        }

        resp.json::<T>().await.map_err(|e| e.into())
    }
}

impl Default for GammaClient {
    fn default() -> Self {
        Self::new()
    }
}
