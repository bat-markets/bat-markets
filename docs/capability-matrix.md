# Capability Matrix

This matrix documents the current `0.1.x` futures-first surface after live transport integration.

## Unified Surface

| Area | Binance Linear Futures | Bybit Linear Futures | Notes |
| --- | --- | --- | --- |
| metadata bootstrap | yes | yes | `build_live().await` refreshes `InstrumentSpec` from venue snapshots |
| server time / clock skew | yes | yes | health snapshot stores the latest observed skew |
| public websocket stream | yes | yes | exposed through `stream().public().spawn_live(...)` |
| private websocket stream | yes | yes | exposed through `stream().private().spawn_live()` |
| transport watermark / gap detection | foundation | foundation | native sequence or monotonic watermarks trip reconnect and divergence handling |
| manual private reconcile | yes | yes | exposed through `stream().private().reconcile().await` |
| health snapshot | yes | yes | cheap synchronous snapshot |
| health subscriptions | yes | yes | watch + broadcast notifications on structural health transitions |
| account refresh | yes | yes | REST snapshot-backed |
| position refresh | yes | yes | REST snapshot-backed |
| open orders refresh | yes | yes | REST snapshot-backed |
| execution history refresh | yes | yes | exposed through `trade().refresh_executions(...)` |
| get order | yes | yes | REST-backed unified order snapshot |
| create order | yes | yes | command receipt with explicit `UnknownExecution` path |
| cancel order | yes | yes | command receipt with explicit `UnknownExecution` path |
| set leverage | yes | yes | venue-native REST flows |
| set margin mode | yes | yes | Binance symbol-level, Bybit account-level |
| periodic reconcile / metadata maintenance | yes | yes | live stream runners perform background health checks and periodic repair/metadata refresh |
| reconcile after reconnect / unknown execution | foundation+ | foundation+ | snapshots plus order/execution history are used where venue allows it; unresolved outcomes stay explicit |

## Native / Venue-Specific Notes

| Topic | Binance | Bybit |
| --- | --- | --- |
| private stream auth | listen key REST bootstrap + websocket | websocket auth frame using signed `GET/realtime` payload |
| open interest live refresh | public REST `/fapi/v1/openInterest` | public REST `/v5/market/tickers` or ticker stream |
| margin mode semantics | per-symbol margin type | account-level margin mode |
| metadata source | `exchangeInfo` | `instruments-info` |

## Honest Limits

- Command writes do not pretend transport errors are harmless: they return `UnknownExecution` receipts and trigger reconcile attempts.
- Reconcile now repairs balances, positions, open orders, and recent execution evidence, and recent-history repair batches pending `UnknownExecution` checks per instrument instead of repeating identical REST calls.
- Periodic private reconcile now stays snapshot-only for simple freshness maintenance and escalates to recent-history repair only when uncertainty or divergence signals are present.
- Recent-history repair now uses bounded time windows derived from local private-state timestamps and pending uncertainty instead of unbounded symbol-level history pulls.
- Reconcile still does not rebuild a full historical ledger.
- Live sandbox tests are env-gated and write flows require an explicit manual gate.
- Mainnet production-key coverage is intentionally read-only in the repo test harness; private write tests remain sandbox-only by design.
