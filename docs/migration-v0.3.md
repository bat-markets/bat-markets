# Migration to v0.3

v0.3 removes the legacy nested facade that was kept as a hidden compatibility
bridge during the v0.2 transition. New code should use root methods and reserve
`advanced()` for raw/custom integration points.

## Direct Replacements

| Removed v0.2 compatibility path | v0.3 path |
| --- | --- |
| `client.market().instrument_specs()` | `client.markets()` |
| `client.market().require_instrument(symbol)` | `client.advanced().require_instrument(symbol)` |
| `client.market().fetch_ticker(symbol).await` | `client.fetch_ticker(symbol).await` |
| `client.market().fetch_order_book(request).await` | `client.fetch_order_book(symbol, limit).await` |
| `client.market().fetch_ohlcv(request).await` | `client.fetch_ohlcv(request).await` |
| `client.market().fetch_trades(request).await` | `client.fetch_trades(symbol, limit).await` |
| `client.market().refresh_open_interest(symbol).await` | `client.fetch_open_interest(symbol).await` |
| `client.account().refresh().await` | `client.fetch_balance().await` |
| `client.position().refresh().await` | `client.fetch_positions().await` |
| `client.trade().refresh_open_orders(request).await` | `client.fetch_open_orders(request).await` |
| `client.trade().get_order(request).await` | `client.fetch_order(request).await` |
| `client.trade().refresh_executions(request).await` | `client.fetch_my_trades(request).await` |
| `client.stream().public().watch_ticker(request).await` | `client.watch_ticker(symbol).await` or `client.watch_tickers(symbols).await` |
| `client.stream().public().watch_trades(request).await` | `client.watch_trades(symbol).await` or `client.watch_trades_for_symbols(symbols).await` |
| `client.stream().public().watch_order_book(request).await` | `client.watch_order_book(symbol, limit).await` |
| `client.stream().public().watch_ohlcv(request).await` | `client.watch_ohlcv(symbol, interval).await` or `client.watch_ohlcv_for_symbols(symbols, interval).await` |
| `client.stream().private().watch_balances().await` | `client.watch_balance().await` |
| `client.stream().private().watch_orders().await` | `client.watch_orders().await` |
| `client.stream().private().watch_executions().await` | `client.watch_my_trades().await` |
| `client.stream().private().watch_positions().await` | `client.watch_positions().await` |
| `client.entry().create_order(request).await` | `client.create_order(request).await` |
| `client.entry().amend_order(request).await` | `client.edit_order(request).await` |
| `client.entry().cancel_order(request).await` | `client.cancel_order(request).await` |
| `client.health().snapshot()` | `client.status()` |
| `client.health().subscribe()` | `client.watch_status()` |
| `client.diagnostics().snapshot()` | `client.advanced().diagnostics()` |
| `client.native()` | `client.advanced().native()` |

## Cached State

The old `market()`, `account()`, `position()`, and `trade()` accessors mixed
user-facing REST methods with local cache reads. v0.3 keeps live reads at the
root and moves local cache inspection under explicit advanced helpers:

- `advanced().cached_ticker(symbol)`
- `advanced().cached_recent_trades(symbol)`
- `advanced().cached_book_top(symbol)`
- `advanced().cached_balances()`
- `advanced().cached_positions()`
- `advanced().cached_orders()`
- `advanced().cached_open_orders()`
- `advanced().cached_executions()`

## Removed Names

`AmendOrderRequest` and `AmendOrdersRequest` are no longer re-exported by
`bat-markets`. Use `EditOrderRequest` and `EditOrdersRequest` in public code.
Internal runtime code may still use amend terminology while venue adapters are
being consolidated.
