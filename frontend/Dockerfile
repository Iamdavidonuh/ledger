# Build the static SvelteKit output.
FROM node:22-slim AS builder
WORKDIR /build
COPY package.json package-lock.json ./
RUN npm ci
COPY . .
RUN npm run build

# static-web-server: a single Rust binary on a scratch base (no shell, no
# package manager), a few MB total, chosen over nginx/caddy for size. It
# just serves the files adapter-static already wrote; SERVER_FALLBACK_PAGE
# gives every unmatched path index.html so the SPA's client-side router
# handles it.
FROM ghcr.io/static-web-server/static-web-server:2
COPY --from=builder /build/build /public
ENV SERVER_ROOT=/public
ENV SERVER_FALLBACK_PAGE=/public/index.html
# The base image listens on 80 by default despite the binary's own
# documented default of 8080; pinned explicitly rather than relied on.
ENV SERVER_PORT=8080
EXPOSE 8080
