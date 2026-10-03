# ledger frontend

SvelteKit PWA for the ledger app: accounts, entries, pots, bank statement
imports, and stats. Talks to the Rust API in `../api`.

## Developing

```sh
npm install
npm run dev
```

The dev server proxies `/api/*` to `http://localhost:8080` (see
`vite.config.ts`), so run the API locally alongside it.

## Building

```sh
npm run build
```

Static output (adapter-static), served in production by the
`static-web-server` image built from `Dockerfile`. The deployed API's
address is a runtime value, not baked into the build -- see
`static/config.js` and `src/lib/api/client.ts`.

## Checking

```sh
npm run check
```

## Releases

Same convention as the backend (see the repo root README): commit titles
need a Conventional Commits prefix (`feat:`, `fix:`, `chore:`, etc.) for
release-please to pick them up. This package releases independently from
the backend -- a commit only touching `frontend/` only bumps this one.
