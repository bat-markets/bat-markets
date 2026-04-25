# Capability Matrix

This matrix documents the current `0.2.x` CCXT-style root surface after live transport integration and the WS-first command/runtime pass.

## Unified Surface

| Area | Binance Linear Futures | Bybit Linear Futures | Notes |
| --- | --- | --- | --- |
| metadata bootstrap | yes | yes | `build_live().await` refreshes `InstrumentSpec` from venue snapshots |
| server time / clock skew | yes | yes | health snapshot stores the latest observed skew |
| shared public websocket hub | yes | yes | exposed through root `watch_*` methods |
| shared private websocket hub | yes | yes | exposed through root private `watch_*` methods |
| transport watermark / gap detection | foundation | foundation | native sequence or monotonic watermarks trip reconnect and divergence handling |
| manual private reconcile | yes | yes | exposed through `advanced().reconcile().await` |
| health snapshot | yes | yes | cheap synchronous `status()` snapshot |
| health subscriptions | yes | yes | `watch_status()` plus advanced broadcast notifications |
| ticker fetch | yes | yes | REST-backed latest ticker snapshot via `fetch_ticker(...)` |
| tickers fetch | yes | yes | batched latest ticker snapshots via `fetch_tickers(...)` |
| mark price fetch | yes | yes | REST-backed mark price via `fetch_mark_price(...)` |
| funding rate fetch | yes | yes | REST-backed funding rate via `fetch_funding_rate(...)` |
| recent trades fetch | yes | yes | REST-backed recent public trades via `fetch_trades(...)` |
| book top fetch | yes | yes | compatibility REST-backed best bid/ask snapshot remains available on `market().fetch_book_top(...)` |
| focused order book fetch | yes | yes | REST-backed depth snapshot via `fetch_order_book(...)` |
| liquidation fetch | yes | yes | cache-backed via `fetch_liquidations(...)` after live liquidation flow warms the cache |
| OHLCV fetch | yes | yes | REST-backed unified candles via `fetch_ohlcv(...)`; intervals use ccxt-style strings such as `1m`, `5m`, `1h`, `1d`, and each call can batch `1..=30` instruments |
| OHLCV full-window fetch | yes | yes | `fetch_ohlcv(...)` fully paginates a bounded `start_time..end_time` range across the requested symbol batch |
| ticker watch | yes | yes | typed live ticker snapshots via `watch_ticker(...)` / `watch_tickers(...)` |
| fast multi-topic feed | yes | yes | advanced compact shared-feed surface remains available on `stream().public().subscribe_fast(...)` / `watch_fast(...)` |
| trades watch | yes | yes | typed live trades via `watch_trades(...)` / `watch_trades_for_symbols(...)` |
| mark price watch | yes | yes | typed live mark price via `watch_mark_price(...)` |
| funding rate watch | yes | yes | typed live funding-rate updates via `watch_funding_rate(...)` |
| open interest watch | yes | yes | typed live open-interest updates via `watch_open_interest(...)` |
| focused order book watch | yes | yes | typed focused-symbol depth via `watch_order_book(...)` |
| liquidations watch | yes | yes | typed liquidation flow via `watch_liquidations(...)` |
| OHLCV watch | yes | yes | typed live candles via `watch_ohlcv(...)` / `watch_ohlcv_for_symbols(...)`; one or many symbols per watcher, same ccxt-style interval surface |
| orders watch | yes | yes | typed private order updates via `watch_orders()` |
| executions watch | yes | yes | typed private execution updates via `watch_my_trades()` |
| positions watch | yes | yes | typed private position updates via `watch_positions()` |
| balances watch | yes | yes | typed private balance updates via `watch_balance()` |
| account watch | yes | yes | advanced typed account-summary updates remain available on `stream().private().watch_account()` |
| account fetch | yes | yes | REST snapshot-backed `fetch_balance()` returns balances and summary |
| position fetch | yes | yes | REST snapshot-backed `fetch_positions()` |
| open orders fetch | yes | yes | REST snapshot-backed `fetch_open_orders(...)` |
| execution history fetch | yes | yes | exposed through `fetch_my_trades(...)` |
| get order | yes | yes | REST-backed unified order snapshot via `fetch_order(...)` |
| create order | yes | yes | `create_order(...)` returns lifecycle handles |
| create orders | yes | yes | batch create through `create_orders(...)` |
| edit order | yes | yes | `edit_order(...)` |
| edit orders | yes | yes | `edit_orders(...)` |
| cancel order | yes | yes | `cancel_order(...)` returns lifecycle handles |
| cancel orders | yes | yes | batch cancel through `cancel_orders(...)` |
| cancel all orders | yes | yes | `cancel_all_orders(...)` |
| close position | yes | yes | `close_position(...)` |
| validate order | yes | yes | `validate_order(...)` |
| set leverage | yes | yes | `set_leverage(...)`; venue-native REST flow |
| set margin mode | yes | yes | `set_margin_mode(...)`; Binance symbol-level, Bybit account-level |
| set position mode | yes | yes | `set_position_mode(...)` |
| command lifecycle bus | yes | yes | `PendingCommandHandle::next_lifecycle()` and `advanced().subscribe_command_events()` |
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
- Root command methods are the hot-path command surface; nested clients remain for compatibility and low-level workflows.
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
