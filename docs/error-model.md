# Error Model

The error taxonomy follows the blueprint and must remain machine-handleable.

## Core Categories

- `ConfigError`
- `TransportError`
- `Timeout`
- `RateLimited`
- `AuthError`
- `PermissionDenied`
- `Unsupported`
- `DecodeError`
- `ExchangeReject`
- `UnknownExecution`
- `StateDivergence`
- `ComplianceRequired`
- `TemporaryUnavailable`

## Required Behavior

- errors carry safe context when available
- retriable vs terminal stays explicit
- exchange-native codes are preserved as data
- `UnknownExecution` is never collapsed into `Timeout`

## UnknownExecution

`UnknownExecution` means the command path cannot truthfully assert final exchange outcome.

Typical triggers:

- timeout after send,
- lost acknowledgement,
- reconnect during order placement,
- contradictory REST vs stream evidence before reconcile.

The caller must reconcile or explicitly degrade behavior.

