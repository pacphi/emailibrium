# BUILD CONTEXT IS THE REPO ROOT — not backend/.
#
# backend/Cargo.toml declares path dependencies on the vendored ruvector
# submodule (../ruvector/crates/ruvector-core, ruvector-gnn). Those live OUTSIDE
# backend/, so a `context: ./backend` build cannot resolve them and the release
# compile fails at manifest load. That failure stayed invisible because CI built
# the last stage (`development`), which never runs cargo build.
#
# Every caller must therefore pass `context: .` + `dockerfile: backend/Dockerfile`:
# docker-compose.yml, .github/workflows/docker.yml and .github/workflows/release.yml
# are all wired that way. Checkouts must also fetch submodules, or ruvector/ is an
# empty directory and the COPY below silently yields nothing.
#
# See also /.dockerignore — with a repo-root context it is load-bearing, not
# cosmetic (backend/target alone is tens of GB on a dev machine).

# Single source for the Rust version across all stages. docker-compose.yml
# already passed a RUST_VERSION build arg; before this it was silently ignored,
# because no ARG was declared and the tag was hardcoded.
ARG RUST_VERSION=1.97

# ─── Stage 1: Build ────────────────────────────────────────────────────────
# The Debian release is pinned EXPLICITLY on both the builder and the runtime
# stage, and the two MUST stay on the same release. Getting this wrong does not
# fail the build — it produces an image whose binary dies instantly at startup:
#
#   builder trixie + runtime bookworm -> builds clean, then
#       "/app/emailibrium: version `GLIBC_2.39' not found" (also GLIBC_2.38,
#       CXXABI_1.3.15). Observed; this is how the image looked "fine" in CI.
#
# Trixie is the floor, not a preference: ort_sys (ONNX Runtime, via fastembed)
# ships PREBUILT C++ objects compiled with GCC 13+, so a bookworm builder cannot
# even link them — `undefined reference to __cxa_call_terminate`. The runtime
# therefore has to come up to trixie rather than the builder going down.
#
# Bump these two lines together or not at all.
FROM rust:${RUST_VERSION}-slim-trixie AS builder

# rust:*-slim ships neither of these, and the release build needs both. This gap
# was latent until now: the release build had never actually run in CI, because
# the workflows built the last stage (`development`), which compiles nothing.
#   pkg-config + libssl-dev — openssl-sys, pulled in via async-native-tls.
#   g++ (libstdc++)         — the final link needs -lstdc++ for the C/C++ deps
#                             (onig_sys, simsimd, ort/ONNX).
RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config libssl-dev g++ \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# The vendored path dependencies. This is the whole point of the repo-root
# context — without it cargo cannot resolve ruvector-core / ruvector-gnn.
COPY ruvector/ ruvector/

# backend/ is copied piecewise rather than wholesale so the image never picks up
# local dev state (target/, *.db) or backend/.cargo/config.toml, whose
# rustc-wrapper would also require backend/scripts/ to be present.
COPY backend/rust-toolchain.toml backend/
COPY backend/Cargo.toml backend/Cargo.lock backend/
COPY backend/src/ backend/src/
# benches/ is needed at manifest load: Cargo.toml declares [[bench]]
# vector_benchmarks, and cargo errors if a declared target's file is missing.
COPY backend/benches/ backend/benches/
COPY backend/migrations/ backend/migrations/

WORKDIR /app/backend
RUN cargo build --release

# ─── Stage 2: Runtime ──────────────────────────────────────────────────────
# MUST match the builder's Debian release — see the note on the builder stage.
FROM debian:trixie-slim AS runtime
# The binary dynamically links -lssl -lcrypto -lstdc++, so those runtime libs
# must be present. Declared explicitly rather than relying on whatever the base
# image happens to pull in, so a base-image change fails at build time here
# instead of as a missing-shared-object crash on first start.
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates libssl3 libstdc++6 \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/backend/target/release/emailibrium /app/emailibrium
COPY --from=builder /app/backend/migrations /app/migrations
COPY backend/entrypoint.sh /app/entrypoint.sh
RUN chmod +x /app/entrypoint.sh
# /app/data must exist AND be owned by the runtime uid before USER drops
# privileges. docker-compose.yml mounts backend_data:/app/data; if the mountpoint
# is absent from the image, Docker creates it root:root, and the container — which
# runs read_only as 1000:1000 — then dies with "Permission denied (os error 13)"
# the first time it writes there.
RUN mkdir -p /app/data && chown -R 1000:1000 /app/data
WORKDIR /app
USER 1000:1000
ENTRYPOINT ["/app/entrypoint.sh"]
CMD ["/app/emailibrium"]

# ─── Stage 3: Development (docker-compose.dev.yml) ─────────────────────────
# Same trixie pin and same build deps as the builder stage — this stage actually
# COMPILES (cargo watch -x run), so bookworm here would hit the identical
# ort_sys/GCC-13 link failure, and without pkg-config/libssl-dev/g++ it cannot
# link at all.
FROM rust:${RUST_VERSION}-slim-trixie AS development
RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config libssl-dev g++ \
    && rm -rf /var/lib/apt/lists/* \
    && cargo install cargo-watch
# Matches the compose dev mount: docker-compose.dev.yml binds the repo root at
# /app, so the crate is at /app/backend and ../ruvector resolves correctly.
WORKDIR /app/backend
CMD ["cargo", "watch", "-x", "run"]
