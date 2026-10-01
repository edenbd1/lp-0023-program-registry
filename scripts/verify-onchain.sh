#!/usr/bin/env bash
# Re-check a deployed registry against the chain, read-only.
#
#   PROVENANCE_REGISTRY=<registry account> [LEZ_RPC=https://testnet.lez.logos.co] \
#     ./scripts/verify-onchain.sh [artifacts/programs/lez_registry.bin]
#
# Checks, without trusting any file in this repository for the answers:
#   - the registry account holds a program header, and its ImageID is the one
#     the given artifact computes to (so the committed binary is what runs);
#   - the registry has a deployer entry for itself, signed by its own header
#     account, recording that same ImageID;
#   - the directory walk visits exactly as many entries as the counter says,
#     and every entry names an account that holds a program (the SDK e2e tests).
#
# Testnet v0.3 deployment is pending funding; until DEPLOYMENTS.md names a
# registry account this script has nothing to check and says so.
set -euo pipefail
cd "$(dirname "$0")/.."

: "${LEZ_RPC:=https://testnet.lez.logos.co}"
export LEZ_RPC
if [ -z "${PROVENANCE_REGISTRY:-}" ]; then
  echo "PROVENANCE_REGISTRY is not set: there is no public v0.3 deployment to check yet (see DEPLOYMENTS.md)."
  exit 2
fi
export PROVENANCE_REGISTRY
ARTIFACT="${1:-artifacts/programs/lez_registry.bin}"

cargo build --release -q --manifest-path cli/Cargo.toml
CLI=cli/target/release/lez-registry
fail=0
check() { if "$@"; then echo "ok:   $*"; else echo "FAIL: $*"; fail=1; fi; }

on_chain=$("$CLI" program-header "$PROVENANCE_REGISTRY" | awk '/^image id/{print $3}')
echo "registry $PROVENANCE_REGISTRY on $LEZ_RPC"
echo "  header ImageID $on_chain"
if [ -f "$ARTIFACT" ]; then
  built=$("$CLI" image-id "$ARTIFACT" 2>/dev/null)
  echo "  $ARTIFACT ImageID $built"
  check test "$on_chain" = "$built"
fi
entry=$("$CLI" lookup "$PROVENANCE_REGISTRY")
echo "$entry" | sed 's/^/  /'
check grep -q "publisher    deployer" <<<"$entry"
check grep -q "author       $PROVENANCE_REGISTRY" <<<"$entry"
check grep -q "image id     $on_chain" <<<"$entry"
check cargo test --release -q --manifest-path sdk/Cargo.toml --test e2e -- --ignored
exit $fail
