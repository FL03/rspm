/*
    Appellation: balance <module>
    Created At: 2026.08.08:08:20:29
    Contrib: @FL03
*/

use polymarket::types::{Address, U256};
use hashbrown::HashMap;

#[cfg_attr(
    feature = "serde",
    derive(serde::Deserialize, serde::Serialize),
    serde(deny_unknown_fields)
)]
pub struct RawBalanceAllowanceResponse {
    #[serde(deserialize_with = "crate::utils::deserialize_u256")]
    pub(crate) balance: U256,
    #[serde(deserialize_with = "crate::utils::deserialize_allowances")]
    pub(crate) allowances: HashMap<Address, U256>,
}

impl core::fmt::Debug for RawBalanceAllowanceResponse {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("RawBalanceAllowanceResponse")
            .field("allowance_count", &self.allowances.len())
            .finish_non_exhaustive()
    }
}
