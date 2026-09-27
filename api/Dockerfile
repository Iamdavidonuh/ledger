# Build stage: rusqlite's "bundled" feature compiles SQLite from C source,
# so the builder needs a C toolchain even though the final binary is
# statically linked against it and needs no libsqlite3 at runtime.
FROM rust:1-slim-bookworm AS builder
RUN apt-get update && apt-get install -y --no-install-recommends build-essential && rm -rf /var/lib/apt/lists/*
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY core ./core
COPY api ./api
RUN cargo build --release -p ledger-api

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
ENV LEDGER_DB_PATH=/data/ledger.sqlite3
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/ledger-api"]
