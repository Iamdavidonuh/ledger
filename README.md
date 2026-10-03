# ledger

A personal, multi-currency money tracker. Accounts, pots, loans, investments,
and bank statement imports, running as a small self-hosted service.

This project is a work in progress.

## Releases

Commit titles on `main` use [Conventional Commits](https://www.conventionalcommits.org/)
(`feat: ...`, `fix: ...`, `chore: ...`, etc.) -- release-please reads these to
decide whether a change ships a release, and whether it's a patch or a minor
bump. The commit body stays free-form prose as before; only the title needs
the prefix. `fix:`/`feat:` ship a release; `chore:`/`docs:`/`refactor:`/
`test:` don't, on their own.

Backend (`.`) and frontend (`frontend/`) release independently -- a commit
only touching one only bumps that one. `.github/workflows/release.yml`
builds and pushes to `ghcr.io/iamdavidonuh/ledger-api` and `ledger-frontend`
only once a release-please PR for that package is actually merged, never on
an ordinary push.

A `Release-As: X.Y.Z` footer forces a specific version, bypassing the
normal bump-type detection -- useful for bootstrapping a package's first
release, since no commit can be a conventional "fix"/"feat" against a
version that doesn't exist yet. **It is not path-scoped**: it overrides
every configured package's next version, not just the one the commit
actually touches (confirmed live -- a `Release-As: 0.0.1` meant only for
`frontend/` kept silently resetting the already-released backend's next
version to 0.0.1 too, across multiple release-please runs, until a newer
`Release-As` on a later commit superseded it). Only ever use it on a
commit by itself, immediately merged, never combined with unrelated
changes to another package in the same push.
