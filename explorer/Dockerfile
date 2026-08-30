# syntax=docker/dockerfile:1
#
# One-command, reproducible, sealed self-host for the LEZ Registry Explorer.
#
# Build FROM THE REPO ROOT so the path dependencies (`..` = lez-registry-core and
# `../sdk`) resolve. BuildKit is required (the sealed build uses `--network=none`);
# modern Docker enables it by default.
#
#   docker build -f explorer/Dockerfile -t lez-registry-explorer .
#   docker run --rm -p 8080:8080 lez-registry-explorer
#   # then open http://localhost:8080
#
# That is the whole install. No database, no secret, no account: it comes up
# pointed at the documented public testnet and the public sequencer and serves.
#
# Reproducible: the same commit built on the same pinned base image produces a
# byte-identical binary, so its sha256 is the same for everyone. Two ways to see
# the digest of what you built:
#
#   docker run --rm --entrypoint sha256sum lez-registry-explorer \
#     /usr/local/bin/lez-registry-explorer
#   # and it is printed by the build stage, and shown in-product on /about.
#
# Stamp the commit so /about can name it (optional but recommended):
#
#   docker build -f explorer/Dockerfile \
#     --build-arg SOURCE_COMMIT=$(git rev-parse HEAD) \
#     -t lez-registry-explorer .
#
# The build is SEALED in the same spirit as scripts/build-sealed.sh — two phases,
# deliberately separate:
#   1. FETCH, with the network. `cargo vendor` resolves every crate and checks it
#      against explorer/Cargo.lock. Nothing here can introduce a crate the
#      lockfile did not pin.
#   2. BUILD, with no network at all (`RUN --network=none`). A build script cannot
#      reach out, so the binary is a function of the pinned sources alone.

# Pin the toolchain. Override at build time with --build-arg RUST_VERSION=…; the
# digest is only reproducible against a fixed base, so pin it when you publish one.
ARG RUST_VERSION=1.90.0
ARG DEBIAN_RELEASE=bookworm

# ---------------------------------------------------------------------------
# 1. FETCH — network ON, vendor every dependency checked against the lockfile.
# ---------------------------------------------------------------------------
FROM rust:${RUST_VERSION}-slim-${DEBIAN_RELEASE} AS fetch
WORKDIR /src
# Only what the explorer's own dependency graph needs: the core crate at the
# repo root, the sdk, and the explorer itself. `cargo vendor` for the explorer
# manifest pulls nothing from the rest of the repo.
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY sdk ./sdk
COPY explorer ./explorer
# Vendor into /src/vendor and write the cargo config that redirects the offline
# build at it. --locked refuses to touch explorer/Cargo.lock: a fetch that had to
# change the lockfile is a fetch that pulled something unpinned, and we want that
# to fail here rather than pass silently.
RUN set -eux; \
    mkdir -p .cargo; \
    cargo vendor --locked --manifest-path explorer/Cargo.toml vendor > .cargo/config.toml

# ---------------------------------------------------------------------------
# 2. BUILD — network OFF. Every crate is already vendored and lockfile-checked.
# ---------------------------------------------------------------------------
FROM rust:${RUST_VERSION}-slim-${DEBIAN_RELEASE} AS build
WORKDIR /src
COPY --from=fetch /src /src
# The commit this binary is built from, shown on /about so a rebuild can be
# compared. build.rs declares the dependency so a changed commit rebuilds.
ARG SOURCE_COMMIT=unpinned
ENV EXPLORER_SOURCE_COMMIT=${SOURCE_COMMIT}
# Determinism: remap the build path out of the binary, drop debuginfo, and turn
# off incremental so the same sources produce the same bytes anywhere.
ENV RUSTFLAGS="--remap-path-prefix=/src=. -Cdebuginfo=0"
ENV CARGO_INCREMENTAL=0
ENV SOURCE_DATE_EPOCH=0
RUN --network=none set -eux; \
    cargo build --release --offline --locked --manifest-path explorer/Cargo.toml; \
    install -Dm0755 explorer/target/release/lez-registry-explorer \
      /usr/local/bin/lez-registry-explorer; \
    echo "sealed build digest:"; \
    sha256sum /usr/local/bin/lez-registry-explorer

# ---------------------------------------------------------------------------
# 3. RUNTIME — a minimal image that holds only the binary. The binary embeds its
#    own assets (CSS/JS via include_str!), so nothing else is copied.
# ---------------------------------------------------------------------------
FROM debian:${DEBIAN_RELEASE}-slim AS runtime
# The explorer makes read-only HTTPS reads to the public sequencer. ureq compiles
# in the Mozilla roots (webpki-roots), so this is belt-and-suspenders for any host
# whose TLS wants system certs; it does not affect the reproducible binary digest.
RUN set -eux; \
    apt-get update; \
    apt-get install -y --no-install-recommends ca-certificates; \
    rm -rf /var/lib/apt/lists/*
COPY --from=build /usr/local/bin/lez-registry-explorer /usr/local/bin/lez-registry-explorer
# A pure view holds nothing and signs nothing; run unprivileged.
USER nobody
EXPOSE 8080
# Serve against the public testnet with no other setup. 0.0.0.0 so the published
# port is reachable from outside the container; every other default is the
# documented deployment. Override any of them by appending flags to `docker run`.
ENTRYPOINT ["/usr/local/bin/lez-registry-explorer"]
CMD ["--host", "0.0.0.0", "--port", "8080"]
