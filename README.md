# ledger

A personal, multi-currency money tracker. Accounts, pots, loans, investments,
and bank statement imports, running as a small self-hosted service.

This project is a work in progress.

## Configuration

The API reads these environment variables:

| Variable | Default | Meaning |
| --- | --- | --- |
| `DATABASE_URL` | `postgres://localhost/ledger` | Postgres connection string. Migrations run at startup. |
| `LEDGER_CORS_ALLOWED_ORIGINS` | unset (no CORS layer) | Comma-separated list of browser origins allowed to call the API cross-origin. |

### CORS

The API has no authentication. With no CORS headers, a browser's same-origin
policy stops a page on another origin from reading a response from the API.
The default is no CORS headers.

Development does not need this set: the frontend calls `/api` on its own
origin and Vite proxies it to the API.

A deployment running the frontend and the API as separate services does need
it. The frontend reads the API's address from `/config.js` at runtime, so a
different host or port there is a different origin, and every call it makes is
cross-origin. Set this to the origin the frontend is served from, port
included:

```
LEDGER_CORS_ALLOWED_ORIGINS=http://10.0.0.5:8091
```

List every origin the app is opened from. A Tailscale IP and a MagicDNS name
for the same host are two different origins.

An origin is a scheme, host and optional port, with no trailing slash and no
path, matching the form a browser sends in the `Origin` header. An unparseable
value fails startup.

`*` allows any origin and must appear alone. There is no authentication, so
that lets any website read everything in the ledger.

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
