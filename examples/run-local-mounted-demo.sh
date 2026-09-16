#!/bin/sh
set -eu

repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
image=${MESH_DEMO_IMAGE:-mesh-capture:sqlite-0.37-ed25519-2.1.1}

if ! docker image inspect "$image" >/dev/null 2>&1; then
    printf '%s\n' "Preparing $image from the pinned Rust toolchain image..."
    docker build --tag "$image" - <<'DOCKERFILE'
FROM rust:1.97-bookworm
RUN apt-get update \
    && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/*
# The runtime remains network-disabled. Cache the exact production SQLite driver's source while
# constructing the image so Cargo can prove the copied workspace builds fully offline below.
RUN mkdir -p /opt/mesh-deps/src \
    && printf '%s\n' \
       '[package]' \
       'name = "mesh-demo-dependency-cache"' \
       'version = "0.0.0"' \
       'edition = "2021"' \
       '[dependencies]' \
       'rusqlite = { version = "=0.37.0", default-features = false, features = ["bundled"] }' \
       'ed25519-dalek = { version = "=2.1.1", default-features = false, features = ["std", "zeroize"] }' \
       > /opt/mesh-deps/Cargo.toml \
    && printf 'fn main() {}\n' > /opt/mesh-deps/src/main.rs \
    && cargo fetch --manifest-path /opt/mesh-deps/Cargo.toml
WORKDIR /work
DOCKERFILE
fi

exec docker run --rm \
    --network none \
    --privileged \
    -v "$repo:/work" \
    -w /work \
    "$image" \
    sh examples/local-mounted-demo-inner.sh
