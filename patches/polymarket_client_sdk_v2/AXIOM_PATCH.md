# Axiom patch provenance

- Upstream: `https://github.com/Polymarket/rs-clob-client-v2`
- Package: `polymarket_client_sdk_v2` `0.7.0`
- Upstream commit: `222143d321eba97d5711a848265eb9aab3bc7ff4`
- Published crate SHA-256: `ba212e0641f178c274af266772de15962ac7e76da550a0f79f47b49349b1138a`
- Last verified: `2026-07-28`
- Upstream lifecycle tracking:
  [issue #39](https://github.com/Polymarket/rs-clob-client-v2/issues/39),
  [PR #40](https://github.com/Polymarket/rs-clob-client-v2/pull/40), and
  [issue #86](https://github.com/Polymarket/rs-clob-client-v2/issues/86)

## Why this patch exists

The upstream CLOB WebSocket reconnection task owns an `Arc<SubscriptionManager>`.
That ownership keeps the connection sender alive after every public client
handle is dropped. The connection loop also treats a closed outgoing channel
as a disabled `select!` branch instead of a shutdown signal. Axiom replaces its
public orderbook client whenever the active token set changes, so each
replacement otherwise leaves the old socket, subscription state, and
background tasks alive.

The Axiom patch adds an idempotent cancellation and shutdown contract for the
connection loop, reconnection handler, and client. `Drop` triggers cancellation
as a fallback; callers that need an ordered replacement use
`Client::shutdown().await`.

The patch also exposes `Client::post_order_initial`. RSPM uses that narrower
primitive so submission authority ends at the POST response and never performs
an unowned follow-up private trade read. The SDK's ordinary `post_order` method
continues to enrich responses with settlement hashes for other callers.

Remove this path patch only after a non-yanked upstream release provides all
five tested contracts: lifecycle cancellation, ordered idempotent shutdown,
stream keepalive ownership, typed recoverable lag, and a distinct initial POST
response primitive. PR #40 alone is insufficient. The Axiom 40-to-48-to-32
subscription churn, lifecycle gates, and RSPM initial-response contract tests
must pass against the published release before removing the override.
