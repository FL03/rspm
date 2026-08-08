/*
    Appellation: consts <module>
    Created At: 2026.08.08:06:48:11
    Contrib: @FL03
*/
use core::time::Duration;

pub(crate) const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

pub(crate) const MAX_ATTEMPTS: usize = 2;

pub(crate) const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

pub(crate) const POLY_ADDRESS: &str = "poly_address";

pub(crate) const POLY_API_KEY: &str = "poly_api_key";

pub(crate) const POLY_PASSPHRASE: &str = "poly_passphrase";

pub(crate) const POLY_SIGNATURE: &str = "poly_signature";

pub(crate) const POLY_TIMESTAMP: &str = "poly_timestamp";

pub(crate) const TERMINAL_CURSOR: &str = "LTE=";

pub(crate) const MAX_PAGE_LIMIT: u64 = 1_000;

pub(crate) const MAX_VENUE_IDENTIFIER_BYTES: usize = 512;

pub(crate) const VERSION_PATH: &str = "/version";

pub(crate) const BALANCE_ALLOWANCE_PATH: &str = "/balance-allowance";

pub(crate) const BALANCE_ALLOWANCE_UPDATE_PATH: &str = "/balance-allowance/update";

pub(crate) const COLLATERAL_SCALE: u64 = 1_000_000;
