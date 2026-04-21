# Capability Matrix

This matrix documents the current `0.1.x` futures-first surface after live transport integration and the WS-first command/runtime pass.

## Unified Surface

| Area | Binance Linear Futures | Bybit Linear Futures | Notes |
| --- | --- | --- | --- |
| metadata bootstrap | yes | yes | `build_live().await` refreshes `InstrumentSpec` from venue snapshots |
| server time / clock skew | yes | yes | health snapshot stores the latest observed skew |
| shared public websocket hub | yes | yes | exposed through `stream().public().watch_*()` and `subscribe_fast(...)` |
| shared private websocket hub | yes | yes | exposed through `stream().private().watch_*()` |
| transport watermark / gap detection | foundation | foundation | native sequence or monotonic watermarks trip reconnect and divergence handling |
| manual private reconcile | yes | yes | exposed through `stream().private().reconcile().await` |
| health snapshot | yes | yes | cheap synchronous snapshot |
| health subscriptions | yes | yes | watch + broadcast notifications on structural health transitions |
| ticker fetch | yes | yes | REST-backed latest ticker snapshot via `market().fetch_ticker(...)` |
| tickers fetch | yes | yes | batched latest ticker snapshots via `market().fetch_tickers(...)` |
| mark price fetch | yes | yes | REST-backed mark price via `market().fetch_mark_price(...)` |
| funding rate fetch | yes | yes | REST-backed funding rate via `market().fetch_funding_rate(...)` |
| recent trades fetch | yes | yes | REST-backed recent public trades via `market().fetch_trades(...)` |
| book top fetch | yes | yes | REST-backed best bid/ask snapshot via `market().fetch_book_top(...)` |
| focused order book fetch | yes | yes | REST-backed depth snapshot via `market().fetch_order_book(...)` |
| liquidation fetch | yes | yes | cache-backed via `market().fetch_liquidations(...)` after live liquidation flow warms the cache |
| OHLCV fetch | yes | yes | REST-backed unified candles via `market().fetch_ohlcv(...)`; intervals use ccxt-style strings such as `1m`, `5m`, `1h`, `1d`, and each call can batch `1..=30` instruments |
| OHLCV full-window fetch | yes | yes | `market().fetch_ohlcv(...)` fully paginates a bounded `start_time..end_time` range across the requested symbol batch; `fetch_ohlcv_window(...)` / `fetch_ohlcv_all(...)` remain compatibility aliases |
| ticker watch | yes | yes | typed live ticker snapshots via `stream().public().watch_ticker(...)` |
| fast multi-topic feed | yes | yes | compact shared-feed surface via `stream().public().subscribe_fast(...)` / `watch_fast(...)` |
| trades watch | yes | yes | typed live trades via `stream().public().watch_trades(...)` |
| book top watch | yes | yes | typed live best bid/ask via `stream().public().watch_book_top(...)` |
| mark price watch | yes | yes | typed live mark price via `stream().public().watch_mark_prices(...)` |
| funding rate watch | yes | yes | typed live funding-rate updates via `stream().public().watch_funding_rates(...)` |
| open interest watch | yes | yes | typed live open-interest updates via `stream().public().watch_open_interest(...)` |
| focused order book watch | yes | yes | typed focused-symbol depth via `stream().public().watch_order_book(...)` |
| liquidations watch | yes | yes | typed liquidation flow via `stream().public().watch_liquidations(...)` |
| OHLCV watch | yes | yes | typed live candles via `stream().public().watch_ohlcv(...)`; one or many symbols per watcher, same ccxt-style interval surface |
| orders watch | yes | yes | typed private order updates via `stream().private().watch_orders()` |
| executions watch | yes | yes | typed private execution updates via `stream().private().watch_executions()` |
| positions watch | yes | yes | typed private position updates via `stream().private().watch_positions()` |
| balances watch | yes | yes | typed private balance updates via `stream().private().watch_balances()` |
| account watch | yes | yes | typed private account-summary updates via `stream().private().watch_account()` |
| account refresh | yes | yes | REST snapshot-backed |
| position refresh | yes | yes | REST snapshot-backed |
| open orders refresh | yes | yes | REST snapshot-backed |
| execution history refresh | yes | yes | exposed through `trade().refresh_executions(...)` |
| get order | yes | yes | REST-backed unified order snapshot |
| create order | yes | yes | `trade()` compatibility surface returns receipts; `entry()` returns low-latency handles |
| create orders | yes | yes | batch create through `entry().create_orders(...)` |
| amend order | yes | yes | `entry().amend_order(...)` |
| amend orders | yes | yes | `entry().amend_orders(...)` |
| cancel order | yes | yes | `trade()` compatibility surface returns receipts; `entry()` returns low-latency handles |
| cancel orders | yes | yes | batch cancel through `entry().cancel_orders(...)` |
| cancel all orders | yes | yes | `entry().cancel_all_orders(...)` |
| close position | yes | yes | `entry().close_position(...)` |
| validate order | yes | yes | `entry().validate_order(...)` |
| set leverage | yes | yes | venue-native REST flows |
| set margin mode | yes | yes | Binance symbol-level, Bybit account-level |
| set position mode | yes | yes | `entry().set_position_mode(...)` and compatibility path through `position()` |
| command lifecycle bus | yes | yes | `entry().subscribe()` and `PendingCommandHandle::next_lifecycle()` |
| periodic reconcile / metadata maintenance | yes | yes | live stream runners perform background health checks and periodic repair/metadata refresh |
| reconcile after reconnect / unknown execution | foundation+ | foundation+ | snapshots plus order/execution history are used where venue allows it; unresolved outcomes stay explicit |

## Native / Venue-Specific Notes

| Topic | Binance | Bybit |
| --- | --- | --- |
| private stream auth | listen key REST bootstrap + websocket | websocket auth frame using signed `GET/realtime` payload |
| command transport | websocket `order.place/order.modify/order.cancel` for hot path; REST fallback for algo orders, validation, settings, and cancel-all | trade websocket supports single-order and batch command flows; REST fallback remains available |
| open interest live refresh | public REST `/fapi/v1/openInterest` | public REST `/v5/market/tickers` or ticker stream |
| margin mode semantics | per-symbol margin type | account-level margin mode |
| metadata source | `exchangeInfo` | `instruments-info` |

## Honest Limits

- Command writes do not pretend transport errors are harmless: they return `UnknownExecution` receipts and trigger reconcile attempts.
- `entry()` is the hot-path command surface; `trade()` remains for compatibility and read-side workflows.
- Reconcile now repairs balances, positions, open orders, and recent execution evidence; it first resolves pending `UnknownExecution` outcomes from local state, then recent-history repair batches the remaining checks per instrument instead of repeating identical REST calls.
- Periodic private reconcile now stays snapshot-only for simple freshness maintenance and escalates to recent-history repair only when uncertainty or divergence signals are present.
- Heavy reconcile prefetches recent execution history only for local active/recent instruments when the trigger or health state points to a private gap or divergence.
- Recent-history repair now uses bounded time windows derived from local private-state timestamps and pending uncertainty instead of unbounded symbol-level history pulls.
- Reconcile still does not rebuild a full historical ledger.
- Live sandbox tests are env-gated and write flows require an explicit manual gate.
- The repository harness also contains env-gated mainnet/operator write checks for approved testing subaccounts; they are intentionally manual and opt-in.
- Binance private order and execution streams are realtime in live runs, but exact account/position/balance panels may still require refresh-backed fallback when `ACCOUNT_UPDATE` is not delivered in the observation window.
- Liquidation fetches are populated from the live feed cache rather than a dedicated exchange snapshot endpoint.
- OHLCV live stress coverage is opt-in through `BAT_MARKETS_ENABLE_MAINNET_OHLCV_STRESS`; the harness validates paged `fetch_ohlcv()` and multi-symbol `watch_ohlcv()` against a frontend-style `30 symbols x 3 days x 1m` read pattern.
