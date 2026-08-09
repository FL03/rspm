# CHANGELOG

An active record detailing the various changes for a corresponding version.

---

## `v0.0.0`

### Added

- Added the temporary, `sdk`-gated `rspm::polymarket` public crate export for callers removing a direct `polymarket_client_sdk_v2` dependency. It preserves exact upstream type and macro identity but is not part of RSPM's canonical native API.

### Changed

- Separated prediction-market outcome identity from CLOB trade action. `Side::{Yes, No}` now represents only the selected outcome token, while `ClobSide::{Buy, Sell}` represents only the action applied to that token.
- Changed `OrderReq::side` and `OrderReq::new` to use `ClobSide`.
- Changed `ClobClient::submit_fak` to accept `ClobSide`; the SDK-side conversion now occurs only inside RSPM and preserves BUY/SELL exactly.
- Changed `Fill` to carry both `outcome: Side` and `action: ClobSide`. Its direction-neutral transaction value is now named `notional` instead of `cost`.
- Changed `ClobSubmitContext` to carry `outcome: Side` and `direction: ClobSide`; dead-letter JSON now uses `outcome` and `direction` instead of the ambiguous `side` key.
- Moved `OrderType` into its focused implementation module without changing the crate-root export or wire tags. New code can use its fallible parser; the legacy `From<&str>` and `From<String>` defaults remain available.
- Changed `std` to include the SDK-free `alloc` primitive closure so isolated `sdk` builds have the allocation types they require.
- Changed RSPM's `tracing` feature to weak-forward SDK tracing when the optional SDK is active, preserving SDK telemetry without making tracing activate the SDK.
- Changed `clob` to activate `sha2` directly for its credential and protocol-identity hashing. The SDK-only, Gamma-only, and market-primitives profiles remain independent of that RSPM dependency.

### Removed

- Removed all direct `Side` to SDK BUY/SELL conversions and the reverse SDK action to outcome conversion. SDK `Unknown` can no longer silently become `Side::No`.
- Removed BUY/SELL aliases from outcome parsing and removed outcome-level `is_buy`/`is_sell` helpers.
- Removed `Fill::default` and basis-free fill PnL helpers. A SELL fill's realized PnL requires position cost basis and cannot be inferred from one fill.
- Removed the `frame-identity` feature, Axiom-specific private-frame evidence key and payload digest types/functions/root exports, and `AuthenticatedUserRawFrame::private_frame_evidence_key_v1`. RSPM retains only the exact raw payload and authenticated transport receipt getters; downstream contracts own evidence composition.

### Migration

New code should use the canonical native `rspm::{clob, auth, gamma, types, ...}`
surface. Do not introduce new SDK coupling through `rspm::polymarket`; existing
direct SDK call sites may mechanically move to the exact-identity alias as a
staged migration while RSPM-owned replacements are built. It stays outside the
prelude and exists only with `sdk`; `alloc,std` and native `gamma` remain
SDK-free. Alias removal requires zero direct SDK dependencies outside
RSPM, zero `rspm::polymarket` uses, native replacements for every supported
SDK-typed public escape, and an announced versioned breaking change. Removing
the vendored SDK patch is a separate decision governed by
`patches/polymarket_client_sdk_v2/AXIOM_PATCH.md`.

Rust callers must supply outcome-token identity and trade action independently:

```rust
use rspm::{ClobSide, OrderReq, Side};

let outcome = Side::No;
let request = OrderReq::new("no-token", 0.45, 10.0, ClobSide::Buy);
assert_eq!(outcome, Side::No);
assert_eq!(request.side, ClobSide::Buy);
```

`OrderReq` JSON changes from an outcome-shaped action field:

```json
{"token_id":"no-token","price":0.45,"size":10.0,"side":"BUY"}
```

`Fill` JSON now names both axes explicitly:

```json
{"outcome":"YES","action":"SELL","notional":4.5}
```

Dead-letter payloads likewise use `outcome` plus `direction`. Consumers must not infer either field from the other.
