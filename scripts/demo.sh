#!/usr/bin/env bash
# The demo, from a clean clone, on macOS (Apple Silicon) or Linux (x86_64).
#
# It needs a LEZ v0.3.0 checkout for the standalone sequencer and the wallet,
# and fetches one into _external/lez if LEZ_SRC does not point at one. Then it
# runs the full lifecycle against that local sequencer: deploy, register the
# registry in itself, rebuild and verify, the refused unauthorised deployer
# entry, third-party entries, updates, mismatch, stale, freeze, and discovery.
#
#   ./scripts/demo.sh
#
# Once the public testnet v0.3 deployment exists, the same CLI commands run
# against it with LEZ_RPC=https://testnet.lez.logos.co and PROVENANCE_REGISTRY
# set; see docs/deploy-v03.md and scripts/verify-onchain.sh.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

LEZ_TAG_COMMIT=db66590ab821a4e142c211017a3866d007f6fa77   # v0.3.0
export LEZ_SRC="${LEZ_SRC:-$ROOT/_external/lez}"
if [ ! -d "$LEZ_SRC/.git" ]; then
  echo "fetching LEZ v0.3.0 into $LEZ_SRC"
  git clone --quiet https://github.com/logos-blockchain/logos-execution-zone "$LEZ_SRC"
  git -C "$LEZ_SRC" checkout --quiet "$LEZ_TAG_COMMIT"
fi
have=$(git -C "$LEZ_SRC" rev-parse HEAD)
[ "$have" = "$LEZ_TAG_COMMIT" ] || echo "note: $LEZ_SRC is at $have, not v0.3.0 ($LEZ_TAG_COMMIT)"

exec "$ROOT/scripts/e2e-v03-local.sh"
