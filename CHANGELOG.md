# CHANGELOG

An active record detailing the various changes for a corresponding version.

---

## `v0.0.0`

### Changed

- Separated prediction-market outcome identity from CLOB trade action. `Side::{Yes, No}` now represents only the selected outcome token, while `ClobSide::{Buy, Sell}` represents only the action applied to that token.
- Changed `OrderReq::side` and `OrderReq::new` to use `ClobSide`.
- Changed `ClobClient::submit_fak` to accept `ClobSide`; the SDK-side conversion now occurs only inside RSPM and preserves BUY/SELL exactly.
- Changed `Fill` to carry both `outcome: Side` and `action: ClobSide`. Its direction-neutral transaction value is now named `notional` instead of `cost`.
- Changed `ClobSubmitContext` to carry `outcome: Side` and `direction: ClobSide`; dead-letter JSON now uses `outcome` and `direction` instead of the ambiguous `side` key.

### Removed

- Removed all direct `Side` to SDK BUY/SELL conversions and the reverse SDK action to outcome conversion. SDK `Unknown` can no longer silently become `Side::No`.
- Removed BUY/SELL aliases from outcome parsing and removed outcome-level `is_buy`/`is_sell` helpers.
- Removed `Fill::default` and basis-free fill PnL helpers. A SELL fill's realized PnL requires position cost basis and cannot be inferred from one fill.

### Migration

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
