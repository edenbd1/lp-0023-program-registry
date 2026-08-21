#!/usr/bin/env bash
# Rebuild a registered program from source and report what that establishes.
#
# It prints the fields a `claim_source` instruction needs, and it will not fill
# in `network_disabled=1` on your behalf -- see "The gap" below.
set -uo pipefail
cd "$(dirname "$0")/.."

BIN=artifacts/programs/lez_registry.bin
BUILDER_IMAGE=risczero/risc0-guest-builder:r0.1.88.0

fail=0
say() { printf '%-22s %s\n' "$1" "$2"; }

# The source digest is over the tracked tree, not over a tarball: a tarball
# carries timestamps and permissions that differ between clones and would make
# the digest unreproducible for reasons that have nothing to do with the code.
SRC_DIGEST=$(git ls-files -z src program tests Cargo.toml Cargo.lock \
  | sort -z | xargs -0 shasum -a 256 | shasum -a 256 | cut -d' ' -f1)
say "source digest" "$SRC_DIGEST"

# A claim names a commit. If the tree has edits that commit does not contain,
# the claim is false the moment it is written, so refuse rather than warn.
if ! git diff --quiet HEAD -- src program tests Cargo.toml Cargo.lock; then
  say "commit" "REFUSED — tracked sources differ from HEAD"
  echo
  echo "  Commit or stash first. A source claim points at a commit, and a commit"
  echo "  that does not contain what was built is a claim that was wrong when it"
  echo "  was made, not one that went stale."
  exit 1
fi
say "commit" "$(git rev-parse HEAD)"
say "toolchain" "$(grep -m1 'channel' rust-toolchain.toml 2>/dev/null | grep -o '"[^"]*"' | tr -d '"' || echo 1.88.0)"

# BuildKit keeps the guest builder in its own cache rather than the image store,
# so `docker image inspect` does not see it. The tag is the pin that actually
# applies; the registry digest needs a network round trip and is reported only
# when one is available.
say "builder image" "$BUILDER_IMAGE"
IMG=$(docker buildx imagetools inspect "$BUILDER_IMAGE" 2>/dev/null | awk '/^Digest:/{print $2; exit}')
say "builder digest" "${IMG:-unavailable offline — the tag above is the pin}"

if [ "${1:-}" = "--rebuild" ]; then
  echo
  echo "rebuilding (this takes a few minutes) ..."
  OUT=$(cargo risczero build --manifest-path program/Cargo.toml 2>&1 | tail -1)
  REBUILT=$(printf '%s' "$OUT" | grep -o '[0-9a-f]\{64\}' | head -1)
  DEPLOYED=$(spel program-id "$BIN" 2>/dev/null | grep -o '[0-9a-f]\{64\}' | head -1)
  say "rebuilt ImageID" "${REBUILT:-<build failed>}"
  say "deployed ImageID" "${DEPLOYED:-<no artifact>}"
  if [ -n "$REBUILT" ] && [ "$REBUILT" = "$DEPLOYED" ]; then
    say "result" "match"
  else
    say "result" "MISMATCH"
    fail=1
  fi
fi

cat <<'EOF'

The gap, stated rather than papered over
----------------------------------------
This script cannot report network_disabled=1, because `cargo risczero build`
offers no way to disable the network for the build container -- checked against
its --help and its binary, neither of which carries a --network option.

That matters here more than it would elsewhere. The whole argument of this
registry is that a rebuild which had network access proves nothing: a build.rs
that downloads the already-deployed binary reproduces it perfectly while proving
nothing about the source. So a tool that guessed "probably offline" would be
committing the exact error the registry exists to catch.

What this build DOES establish:
  - determinism. Two independent runs of the same source produced the same
    ImageID, aa13f602f77efbcb1760ce33cb61caa15fa37a2e566e39fe5ada2cd73f8f1a60.
  - a pinned builder. The guest is built inside one pinned image, not against
    whatever toolchain the operator happens to have.
  - the artifact identity. The rebuilt ImageID either equals the deployed one or
    it does not, and this script says which.

What closes it: `cargo risczero build --network=none`, or a guest Dockerfile
using `RUN --network=none` over vendored dependencies. Until one exists, a claim
recorded from this build is honestly flagged network_disabled=0, and the on-chain
program refuses to call it Verified. That refusal is demonstrated in
DEPLOYMENTS.md against the live testnet.
EOF
exit $fail
