# bat-markets Realtime Web Demo

Simple Bun operator panel for running the real `bat-markets` examples from a browser and streaming their logs in realtime.

## Run

From the repository root:

```bash
cd examples/realtime-web
bun run dev
```

Then open `http://127.0.0.1:3107`.

## Credentials

The Bun server inherits your current shell environment and also tries to load:

- `EXCHANGE_API_EXPERT_CREDENTIALS_FILE`
- `BAT_MARKETS_CREDENTIALS_FILE`
- `/Users/kirillovdigital/.codex/skills/exchange-api-expert/credentials.env`

if the file exists.

## What It Runs

The panel does not fake exchange behavior. It launches the real Rust examples, for example:

- `live_realtime_monitor`
- `live_public_multiwatch`
- `live_trade_probe`
- `live_entry_validate`
- `live_binance_trade_cycle`
- `live_binance_extended_stress`
- local fixture probes

Live trading scenarios can place real orders on your approved testing subaccounts.
