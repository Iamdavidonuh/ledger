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
