# Changelog

## 0.1.0-unreleased

- bootstrap workspace structure from the blueprint
- add core domain contracts and error taxonomy
- add execution lane and state engine foundation
- add Binance and Bybit linear futures adapters with fixture-backed parsing
- add facade API, tests, examples, and quality gates
- batch recent-history `UnknownExecution` repair per instrument to cut repeated REST history calls
- keep periodic private reconcile snapshot-only unless health or pending commands require recent-history repair
- bound recent-history repair to local timestamp windows instead of broad symbol-level pulls
- resolve pending `UnknownExecution` outcomes against local state before issuing remote repair queries
- prefetch recent execution evidence only for local active/recent instruments when the reconcile trigger indicates stream gap or divergence
- tolerate sparse Binance live account position fields and numeric zero-shapes instead of failing reconcile
- formalize `0.1.x` as a GitHub/source release with `publish = false` workspace crates, release docs, and reproducible source archives
- add live diagnostics snapshots for shared-state lock wait/hold costs and key runtime latencies to guide future perf decisions
- add unified `market().fetch_ohlcv(...)` for Binance and Bybit REST kline history
- allow unified `market().fetch_ohlcv(...)` to batch `1..=30` instruments per call while preserving ccxt-style intervals and per-candle `instrument_id`
- add `market().fetch_ohlcv_window(...)` and `market().fetch_ohlcv_all(...)` to fully paginate bounded OHLCV ranges across batched multi-symbol requests
- add typed `stream().public().watch_ohlcv(...)` for one or many symbols on Binance and Bybit
- normalize OHLCV intervals to ccxt-style values like `1m`, `5m`, `1h`, and `1d` across REST fetches and websocket watches
- fix live Bybit `watch_ohlcv()` parsing when websocket kline payloads omit per-row `symbol` and only surface it in the topic name
- add realistic OHLCV stress harness coverage for multi-symbol live fetch/watch flows and frontend-style `30 symbols x 3 days x 1m` paging
