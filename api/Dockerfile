# Build stage: rusqlite's "bundled" feature compiles SQLite from C source,
# so the builder needs a C toolchain even though the final binary is
# statically linked against it and needs no libsqlite3 at runtime.
FROM rust:1-slim-bookworm AS builder
RUN apt-get update && apt-get install -y --no-install-recommends build-essential && rm -rf /var/lib/apt/lists/*
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
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
# The volume mounted at /data must be writable by uid 65532, which neither
# Docker nor Kubernetes gives it by default (a fresh volume is root-owned):
#   - k3s: set `securityContext.fsGroup: 65532` on the Pod so the mounted
#     PVC is group-owned and group-writable on attach.
#   - plain `docker run -v`: chown the volume first, e.g.
#     `docker run --rm -v <volume>:/data busybox chown -R 65532:65532 /data`
#     before the first run against it.
# Skipping this fails at startup with "unable to open database file".
FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=builder /build/target/release/ledger-api /usr/local/bin/ledger-api
COPY --from=poppler /poppler-libs /
ENV LEDGER_DB_PATH=/data/ledger.sqlite3
ENV PATH=/usr/bin
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/ledger-api"]
