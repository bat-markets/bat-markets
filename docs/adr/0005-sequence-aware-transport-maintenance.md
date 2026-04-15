# ADR 0005: Sequence-Aware Transport Maintenance

## Status

Accepted

## Context

The first live transport milestone brought real REST/WS runners, but reconnect and reconcile behavior still depended mostly on coarse transport failures and manual repair calls.

That was too weak for a futures-first engine that must stay honest about ambiguity:

- native sequence or monotonic watermarks should surface transport gaps when the venue actually provides them;
- private uncertainty should not be cleared just because snapshots succeeded once;
- metadata freshness and stale private state need a periodic maintenance path while live streams are running;
- production-key validation for an open-source package needs a safe read-only harness, not implicit mainnet writes.

## Decision

The live runtime now:

- tracks venue-native sequence or monotonic watermarks where the selected topics provide them;
- converts detected gaps into explicit divergence handling plus reconnect flow;
- performs periodic metadata refresh and private reconcile checks inside live stream runners;
- merges recent execution and order-history evidence during reconcile where venue REST APIs provide it;
- keeps unresolved `UnknownExecution` outcomes explicit when history cannot prove the final state;
- provides a manual read-only mainnet smoke harness, while private write coverage remains sandbox-only.

## Consequences

Positive:

- transport gaps are no longer modeled only in theory; they can be raised by the live runtime;
- reconcile becomes more useful without pretending to guarantee a full ledger rebuild;
- the package remains safe for open-source operators by separating sandbox write validation from mainnet read-only checks.

Negative:

- live runtime complexity increases: more timer branches, more helper logic, and more venue-specific history handling;
- the engine still relies on a single shared state lock and does not yet provide full historical reconciliation.
