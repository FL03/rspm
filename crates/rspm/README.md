# `rspm` — Polymarket Client

> CLOB + Gamma API client for Polymarket. Provides `ClobClient` and
> `GammaClient`. No Axiom crate dependencies.

## Purpose

`rspm` is the Axiom project's interface to the Polymarket exchange APIs:

- **CLOB API** — order placement, cancellation, book queries, fill streaming.
- **Gamma API** — market discovery, sprint market listing, resolution history.

This crate is standalone — it depends only on external crates, never on
`axiom-core` or other `axiom-*` workspace members. The isolation is intentional:
exchange clients must evolve on their own cadence, carry their own error types,
and be auditable without pulling the whole workspace.

**Post-dev.7 note:** The `ChannelConfig` / `ChannelMeta` types that previously
lived adjacent to this client were moved to `axiom-channels` in dev.7 Lane 2.
Client-local types (`MarketPolymarketRow`) stay here because they are QuestDB
sidecar row types specific to the client's data format — they have no business
in the canonical `axiom-types` crate.

## Public Surface

- `ClobClient` — CLOB API client. Order lifecycle: place, cancel, query fills.
  `SharedClob = Arc<ClobClient>` alias for the common shared-ownership pattern.
- `GammaClient` — Gamma API client. Market discovery, sprint market queries,
  resolution history polling.
- `parse_token_id(s) -> Result<U256>` — decode a Polymarket token ID from its
  hex string form.
- `types::MarketPolymarketRow` — QuestDB sidecar row for the `markets_polymarket`
  table. Client-local; not in `axiom-types`.
- `types::*` — `Order`, `Fill`, `Side`, `TimeInForce`, `OrderStatus` and other
  exchange-native types.
- `consts::*` — Polymarket API base URLs, taker fee rate, default timeout.
- `error::PmError` — `thiserror`-derived error type.

## Features

| Feature | Default? | Description |
|---|---|---|
| `default` | yes | `async` + `clob` + `gamma` + `std` |
| `full` | no | `default` + `json` + `stream` + `tracing` |
| `clob` | yes | `ClobClient` and CLOB types |
| `gamma` | yes | `GammaClient` and Gamma types |
| `async` | yes | async/await support via tokio |
| `stream` | no | WebSocket fill/book streaming |
| `json` | no | Extra serde_json convenience methods |
| `tracing` | no | Instrument every API call |

## Quick Start

```rust
use rspm::{ClobClient, GammaClient};

// Discover active sprint markets
let gamma = GammaClient::default();
let markets = gamma.sprint_markets("btc-updown-5m").await?;

// Query the order book for a market
let clob = ClobClient::new(std::env::var("PM_API_KEY")?, std::env::var("PM_PRIVATE_KEY")?);
let book = clob.book(token_id).await?;
```

```bash
cargo build -p rspm --features full
cargo test  -p rspm --features full
```

## Geo-lock reminder

Polymarket geo-fences US users. Calls to `ClobClient` that require authentication
will fail from US IP addresses. The `axiom-node` machine is region-pinned to
`yyz` (Toronto) to maintain access. The gateway and worker are in `dfw` and are
forbidden from importing this crate.

## Cross-references

- `axiom-channels::ClobBookChannel` — uses `ClobClient` for book snapshots
- `axiom-channels::ChannelConfig` — channel configuration (MOVED from `axiom-config`
  in dev.7; previously adjacent to `rspm`, now lives in `crates/channels/`)
- `axiom-types::pm::*` — canonical Polymarket-specific types (`MarketPolymarket`,
  `SprintMarketRow`, `pm::Order`, etc.) — distinct from the client-local types here
- `bin/node/` — the only binary allowed to import this client
- `bin/mcp/` — MCP server that exposes Gamma market tools to AI agents
- `.artifacts/ctx/canonical-types.md` — type → home crate map (see `MarketPolymarketRow`)
- GH Milestone v0.3.0 — current sprint
