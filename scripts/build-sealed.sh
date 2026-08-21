#!/usr/bin/env bash
# Build the guest with the build container's network disabled.
#
# Two phases, deliberately separate:
#
#   1. FETCH, with the network. `cargo vendor` resolves every dependency and
#      checks it against the hashes already in Cargo.lock. Nothing here can
#      introduce a crate the lockfile did not pin.
#   2. BUILD, with no network at all. The container cannot reach anything, so a
#      build.rs cannot download the artifact it is supposed to reproduce -- the
#      exact attack that made Solana's verified builds meaningless.
#
# Only a build produced this way may be claimed with network_disabled=1.
set -euo pipefail
cd "$(dirname "$0")/.."

VENDOR=vendor-guest
CONFIG=.cargo/config.toml

# The vendor config is installed for the build and removed afterwards. Left in
# place it would redirect *every* crate in the tree at a directory that only
# holds the guest's dependencies, and unrelated builds would fail looking for
# crates that were never vendored.
cleanup() { rm -f "$CONFIG"; rmdir .cargo 2>/dev/null || true; }
trap cleanup EXIT

echo "[1/2] fetching dependencies (network), checked against Cargo.lock ..."
# cargo vendor writes the crates and prints the config stanza that points at
# them; capture the stanza from the same run rather than vendoring twice.
mkdir -p .cargo
cargo vendor --manifest-path program/Cargo.toml "$VENDOR" > "$CONFIG"
sed -i.bak "s|directory = \".*\"|directory = \"$VENDOR\"|" "$CONFIG" && rm -f "$CONFIG.bak"

echo "[2/2] building with --network none ..."

OUT=$(PATH="$PWD/scripts/sealed:$PATH" cargo risczero build --manifest-path program/Cargo.toml 2>&1)
echo "$OUT" | tail -2

IMAGE_ID=$(printf '%s' "$OUT" | grep -o 'ImageID: [0-9a-f]\{64\}' | tail -1 | cut -d' ' -f2)
if [ -z "$IMAGE_ID" ]; then
  echo "sealed build failed -- no ImageID produced" >&2
  exit 1
fi
echo
echo "sealed ImageID: $IMAGE_ID"
echo "network:        disabled for every build container"
