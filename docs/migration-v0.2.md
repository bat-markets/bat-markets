# Migration to v0.2

v0.2 makes the public API user-led instead of implementation-led. The internal
public/private/command lane architecture remains, but the documented primary API
now lives directly on `BatMarkets`.

## Mental Model

| Prefix | Meaning |
| --- | --- |
| `fetch_*` | REST/read operation |
| `watch_*` | websocket/live operation using shared hubs |
| `create_*`, `edit_*`, `cancel_*`, `close_*`, `set_*` | command operation |
| `advanced()` | raw lanes, diagnostics, manual reconcile, native adapter access |

Rust does not need a global stop method. Watch handles own their lease: call
`shutdown().await` when you want an explicit stop, or drop the handle.

## Direct Mappings

| v0.1 path | v0.2 path |
| --- | --- |
| `client.market().instrument_specs()` | `client.markets()` |
| `client.market().refresh_metadata().await` | `client.load_markets().await` |
| `client.market().fetch_ticker(&symbol).await` | `client.fetch_ticker(&symbol).await` |
| `client.market().fetch_tickers(&request).await` | `client.fetch_tickers(symbols).await` |
| `client.market().fetch_order_book(&request).await` | `client.fetch_order_book(&symbol, limit).await` |
| `client.market().fetch_ohlcv(&request).await` | `client.fetch_ohlcv(&request).await` |
| `client.market().fetch_trades(&request).await` | `client.fetch_trades(&symbol, limit).await` |
| `client.market().fetch_mark_price(&symbol).await` | `client.fetch_mark_price(&symbol).await` |
| `client.market().fetch_funding_rate(&symbol).await` | `client.fetch_funding_rate(&symbol).await` |
| `client.market().refresh_open_interest(&symbol).await` | `client.fetch_open_interest(&symbol).await` |
| `client.market().fetch_liquidations(&symbol, limit).await` | `client.fetch_liquidations(&symbol, limit).await` |
| `client.account().refresh().await` | `client.fetch_balance().await` |
| `client.position().refresh().await` | `client.fetch_positions().await` |
| `client.trade().refresh_open_orders(request).await` | `client.fetch_open_orders(request).await` |
| `client.trade().get_order(&request).await` | `client.fetch_order(&request).await` |
| `client.trade().refresh_executions(request).await` | `client.fetch_my_trades(request).await` |
| `client.stream().public().watch_ticker(request).await` | `client.watch_ticker(symbol).await` or `client.watch_tickers(symbols).await` |
| `client.stream().public().watch_trades(request).await` | `client.watch_trades(symbol).await` or `client.watch_trades_for_symbols(symbols).await` |
| `client.stream().public().watch_order_book(request).await` | `client.watch_order_book(symbol, limit).await` |
| `client.stream().public().watch_ohlcv(request).await` | `client.watch_ohlcv(symbol, interval).await` or `client.watch_ohlcv_for_symbols(symbols, interval).await` |
| `client.stream().public().watch_mark_prices(request).await` | `client.watch_mark_price(symbol).await` |
| `client.stream().public().watch_funding_rates(request).await` | `client.watch_funding_rate(symbol).await` |
| `client.stream().public().watch_open_interest(request).await` | `client.watch_open_interest(symbol).await` |
| `client.stream().public().watch_liquidations(request).await` | `client.watch_liquidations(symbol).await` |
| `client.stream().private().watch_balances().await` | `client.watch_balance().await` |
| `client.stream().private().watch_orders().await` | `client.watch_orders().await` |
| `client.stream().private().watch_executions().await` | `client.watch_my_trades().await` |
| `client.stream().private().watch_positions().await` | `client.watch_positions().await` |
| `client.entry().create_order(&request).await` | `client.create_order(&request).await` |
| `client.entry().create_orders(&request).await` | `client.create_orders(&request).await` |
| `client.entry().amend_order(&request).await` | `client.edit_order(&request).await` |
| `client.entry().amend_orders(&request).await` | `client.edit_orders(&request).await` |
| `client.entry().cancel_order(&request).await` | `client.cancel_order(&request).await` |
| `client.entry().cancel_orders(&request).await` | `client.cancel_orders(&request).await` |
| `client.entry().cancel_all_orders(&request).await` | `client.cancel_all_orders(&request).await` |
| `client.entry().close_position(&request).await` | `client.close_position(&request).await` |
| `client.entry().validate_order(&request).await` | `client.validate_order(&request).await` |
| `client.entry().set_leverage(&request).await` | `client.set_leverage(&request).await` |
| `client.entry().set_margin_mode(&request).await` | `client.set_margin_mode(&request).await` |
| `client.entry().set_position_mode(&request).await` | `client.set_position_mode(&request).await` |
| `client.health().snapshot()` | `client.status()` |
| `client.health().subscribe()` | `client.watch_status()` |
| `client.stream().public().ingest_json(payload)` | `client.advanced().ingest_public_json(payload)` |
| `client.stream().private().ingest_json(payload)` | `client.advanced().ingest_private_json(payload)` |
| `client.stream().command().classify_json(...)` | `client.advanced().classify_command_json(...)` |
| `client.stream().private().reconcile().await` | `client.advanced().reconcile().await` |
| `client.diagnostics().snapshot()` | `client.advanced().diagnostics()` |
| `client.native()` | `client.advanced().native()` |

## Request Rename

Public docs and root APIs use `edit_*` naming:

| v0.1 name | v0.2 public name |
| --- | --- |
| `AmendOrderRequest` | `EditOrderRequest` |
| `AmendOrdersRequest` | `EditOrdersRequest` |

`bat_markets::types` re-exports the `Edit*` names only. The lower-level core
crate may still use amend terminology internally while the facade keeps the
public contract root-level and edit-oriented.

## Websocket-Only Commands

Use explicit `*_ws` methods when transport choice matters:

- `create_order_ws`
- `create_orders_ws`
- `edit_order_ws`
- `edit_orders_ws`
- `cancel_order_ws`
- `cancel_orders_ws`
- `cancel_all_orders_ws`

These methods force websocket command transport. If the selected venue or
command path cannot provide websocket command transport, the method returns
`Unsupported` and does not silently send the command through REST.
