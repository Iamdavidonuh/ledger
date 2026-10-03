# Build stage. sqlx with postgres uses rustls (statically linked), so no
# C toolchain is needed. The only system dep is the linker already in the
# slim image.
FROM rust:1-slim-bookworm AS builder
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY core ./core
COPY api ./api
COPY importer ./importer
RUN cargo build --release -p ledger-api

# BankB reads a PDF by shelling out to `pdftotext` (poppler), which the
# final distroless image has no package manager to install. Resolved here
# instead: install it from Debian's own package on the same base as the
# final image (bookworm, matching distroless/cc-debian12), then copy the
# binary and every shared library `ldd` actually reports for it -- rather
# than a hand-picked list of library paths, which would be architecture-
# specific and silently stale the next time poppler's own dependencies
# change. `/poppler-libs` mirrors real absolute paths (e.g.
# /lib/aarch64-linux-gnu/libpoppler.so.126) so a straight COPY of that
# directory into the distroless image needs no path rewriting.
FROM debian:bookworm-slim AS poppler
RUN apt-get update && apt-get install -y --no-install-recommends poppler-utils && rm -rf /var/lib/apt/lists/*
RUN set -eu; \
    mkdir -p /poppler-libs/usr/bin; \
    cp "$(command -v pdftotext)" /poppler-libs/usr/bin/pdftotext; \
    for lib in $(ldd "$(command -v pdftotext)" | awk '{print $3}' | grep '^/'); do \
        mkdir -p "/poppler-libs$(dirname "$lib")"; \
        cp "$lib" "/poppler-libs$lib"; \
    done; \
    linker=$(ldd "$(command -v pdftotext)" | grep 'ld-linux' | awk '{print $1}'); \
    mkdir -p "/poppler-libs$(dirname "$linker")"; \
    cp "$linker" "/poppler-libs$linker"

# distroless/cc: glibc and libgcc (both linked by a typical Rust binary)
# with no shell or package manager, much smaller attack surface than a
# debian-slim runtime. :nonroot runs as uid 65532, not root.
#
# distroless/cc-debian12 includes /etc/ssl/certs/ca-certificates.crt, so
# rustls TLS connections to Postgres with sslmode=require work without
# any extra CA bundle.
#
# Required environment variables at runtime:
#   DATABASE_URL  Postgres connection string, e.g.
#                 postgres://user:pass@host:5432/ledger?sslmode=require
FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=builder /build/target/release/ledger-api /usr/local/bin/ledger-api
COPY --from=poppler /poppler-libs /
ENV PATH=/usr/local/bin:/usr/bin
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/ledger-api"]
