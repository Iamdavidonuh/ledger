# Handover

Working directory for a new session: `/Users/davidonuh/Sandbox/rust/ledger`
Branch: `core-ledger-engine` (local only, nothing pushed, no remote configured)

## What this is

A personal, multi-currency money tracker (Rust backend + Svelte PWA frontend,
self-hosted on David's home k3s cluster over Tailscale). Replaces a
previous Notion/Excel tracker.

UX design artifact (Svelte PWA look and feel, decided earlier in this
project): https://claude.ai/artifact/8r5MDnio7M12jMcFZKuWV5

## Where things stand

Backend (`ledger-core` + `ledger-api`) is implemented through three plans:

1. Core ledger engine (`docs/superpowers/plans/2026-09-27-ledger-core-engine.md`)
2. SQLite storage (`docs/superpowers/plans/2026-09-27-sqlite-storage.md`)
3. Axum API (`docs/superpowers/plans/2026-09-27-axum-api.md`)

Each was self-reviewed, implemented, then hostile-reviewed by a subagent,
with real findings fixed. Note: the plan docs' embedded code blocks show
signatures from before the Result-everywhere refactor below and have not
been updated to match — treat the actual source as ground truth, not the
plan docs' code samples.

Most recent work: David pushed back hard on `#[allow(clippy::expect_used
/panic/unwrap_used)]` suppressions ("why do we need to allow anything?"),
so `LedgerStore` and `Ledger` were made genuinely fallible end to end
(`Result`-returning throughout, `.optional()` replacing an error-swallowing
`.ok()` pattern, a new `LedgerError::Storage` variant). A hostile review of
that refactor caught one real bug (`Storage` errors were mapping to HTTP
422 instead of 500) which is fixed. Both are committed locally:

```
4866c94 Map LedgerError::Storage to 500, not the domain-rule 422 bucket
7a23c00 Make LedgerStore genuinely fallible instead of allowing panics
c6331cb Add the Axum API: accounts, entries, pots, transfers and valuations
```

Current verified state: `cargo build --workspace`, `cargo test --workspace`
(103 tests), and `cargo clippy --workspace --all-targets -- -D warnings`
all pass clean. Zero `#[allow(clippy::...)]` remain in production code
except one unrelated `#[allow(clippy::too_many_arguments)]` on
`Ledger::transfer`.

Frontend (Svelte PWA): not started yet.

## Standing rules for this repo (do not relitigate these)

- Never push to a remote or merge to a default branch without being asked
  explicitly, even though everything else can proceed autonomously.
- Never add commit or PR attribution lines.
- No real bank names anywhere in the repo, generic naming only
  (`BankA`/`BankB` etc).
- No em-dashes in any file, check after every edit.
- Once a plan/spec is settled, drive implementation fully end to end
  (write code, run tests, fix review findings, commit) without asking
  permission on things already covered by the discussion. Only stop for
  genuinely new decisions the plan doesn't answer.

## Next likely steps

- Decide whether to update the three plan docs' code samples to match the
  final Result-returning signatures (not yet asked/decided).
- Start the Svelte PWA frontend against the existing Axum API.
- Bank statement PDF import (mentioned in the README, not yet designed).
