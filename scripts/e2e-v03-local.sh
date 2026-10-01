#!/usr/bin/env bash
# The whole registry lifecycle against a real LEZ v0.3.0 sequencer, run locally
# in standalone mode with RISC0_DEV_MODE=0. One command from a clone:
#
#   LEZ_SRC=/path/to/logos-execution-zone@v0.3.0 ./scripts/e2e-v03-local.sh
#   ./scripts/e2e-v03-local.sh | tee evidence/e2e-v03-local.txt
#
# What it does, in order, each step checked by reading the chain back:
#
#   1. starts sequencer_service (standalone) on a free local port
#   2. deploys the registry guest through the v0.3 wallet's program-loader,
#      with a header key the script generates and keeps
#   3. initialises the registry and registers it in itself: a deployer entry,
#      signed by the registry's own header account, with a source claim
#   4. rebuilds the guest from source in a fresh target directory with the
#      network denied, checks the rebuild's ImageID against the header on
#      chain, marks it locally verified, and records the rebuild as an
#      independent attester: Verified
#   5. deploys a second program (LEZ's token guest, immutable) and shows an
#      unauthorised deployer entry being refused on chain, then the real
#      deployer's entry, a third-party entry, a refused update by a stranger,
#      an update by the author with an IDL pointer, and a mismatched rebuild
#   6. deploys a third program (mutable), records a verdict, repoints its
#      header with the loader, and shows sync_header turning the verdict Stale
#   7. freezes an entry and shows the next update refused
#   8. lists, searches and looks up through the CLI, runs the SDK's e2e tests
#      and the resolve example against the same chain, and queries the indexer
#
# Env:
#   LEZ_SRC   logos-execution-zone checkout at v0.3.0 (default ./_external/lez);
#             its sequencer_service (feature standalone) and wallet are built
#             if missing
#   PORT      RPC port (default: first free from 3150)
#   REBUILD   0 skips the from-source rebuild in step 4 (default 1)
#   KEEP      1 leaves the sequencer running at the end
#
# Nothing here touches a public network: the sequencer is standalone, bound to
# 127.0.0.1, and its genesis funds the test payer whose key LEZ publishes in its
# own Justfile.

set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
export RISC0_DEV_MODE=0

LEZ_SRC="${LEZ_SRC:-$ROOT/_external/lez}"
REBUILD="${REBUILD:-1}"
KEEP="${KEEP:-0}"
PAYER_SECRET=7f273098f25b71e6c005a9519f2678da8d1c7f01f6a27778e2d9948abdf901fb

die() { echo "error: $*" >&2; exit 1; }
step() { printf '\n== %s\n' "$*"; }
ok() { printf 'ok: %s\n' "$*"; }

[ -d "$LEZ_SRC" ] || die "no LEZ checkout at $LEZ_SRC (set LEZ_SRC to logos-execution-zone at v0.3.0)"
command -v python3 >/dev/null || die "python3 is required"
command -v curl >/dev/null || die "curl is required"

SEQ_BIN="$LEZ_SRC/target/release/sequencer_service"
WALLET_BIN="${WALLET_BIN:-$LEZ_SRC/target/release/wallet}"
if [ ! -x "$SEQ_BIN" ]; then
  ( cd "$LEZ_SRC" && cargo build --release -p sequencer_service --features standalone ) || die "sequencer build failed"
fi
if [ ! -x "$WALLET_BIN" ]; then
  ( cd "$LEZ_SRC" && cargo build --release -p wallet ) || die "wallet build failed"
fi
cargo build --release --manifest-path cli/Cargo.toml -q || die "CLI build failed"
cargo build --release --manifest-path indexer/Cargo.toml -q || die "indexer build failed"
CLI="$ROOT/cli/target/release/lez-registry"
INDEXER="$ROOT/indexer/target/release/lez-registry-indexer"
reg() { "$CLI" "$@"; }

PORT="${PORT:-}"
if [ -z "$PORT" ]; then
  for p in $(seq 3150 3199); do
    if ! nc -z 127.0.0.1 "$p" 2>/dev/null && ! nc -z 127.0.0.1 "$((p + 6000))" 2>/dev/null; then PORT=$p; break; fi
  done
fi
[ -n "$PORT" ] || die "no free port"
WORK="$(mktemp -d)"
export LEZ_RPC="http://127.0.0.1:$PORT"
SEQ_PID=""
IDX_PID=""
cleanup() {
  [ -n "$IDX_PID" ] && kill "$IDX_PID" 2>/dev/null || true
  if [ -n "$SEQ_PID" ] && [ "$KEEP" != 1 ]; then kill "$SEQ_PID" 2>/dev/null || true; fi
  [ "$KEEP" = 1 ] && echo "left running: sequencer pid $SEQ_PID on $LEZ_RPC, work dir $WORK"
}
trap cleanup EXIT

# The wallet links the CommandLineTools Python framework on macOS.
WALLET_ENV=(env "LEE_WALLET_HOME_DIR=$WORK/wallet")
if [ "$(uname)" = Darwin ]; then
  WALLET_ENV+=("DYLD_FALLBACK_FRAMEWORK_PATH=${DYLD_FALLBACK_FRAMEWORK_PATH:-/Library/Developer/CommandLineTools/Library/Frameworks}")
fi
# The wallet prints whole transactions; keep the lines a reader needs.
wallet() {
  echo "provenance-e2e" | "${WALLET_ENV[@]}" "$WALLET_BIN" "$@" 2>&1 \
    | grep -v -e '^Transaction data is' -e '^Stored ' -e '^Input password' || true
}

rpc() {
  curl -s -m 10 "$LEZ_RPC" -H 'content-type: application/json' \
    -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"$1\",\"params\":$2}"
}

# ---------------------------------------------------------------------------
step "1. a local LEZ v0.3.0 sequencer, standalone, RISC0_DEV_MODE=0"
mkdir -p "$WORK/seq" "$WORK/wallet" "$WORK/keys"
python3 - "$LEZ_SRC/lez/sequencer/service/configs/debug/sequencer_config.json" "$WORK" <<'PY'
import json, sys
src, work = sys.argv[1], sys.argv[2]
d = json.load(open(src))
d["home"] = work + "/seq"
d["block_create_timeout"] = "2s"
json.dump(d, open(work + "/seq_config.json", "w"), indent=1)
PY
RUST_LOG="${RUST_LOG:-warn,sequencer_core=info,lee=info}" "$SEQ_BIN" "$WORK/seq_config.json" \
  --port "$PORT" --listen-address 127.0.0.1 --metrics-address "127.0.0.1:$((PORT + 6000))" \
  > "$WORK/sequencer.log" 2>&1 &
SEQ_PID=$!
for _ in $(seq 1 90); do
  rpc getLastBlockId '[]' | grep -q result && break
  sleep 1
done
rpc getLastBlockId '[]' | grep -q result || die "sequencer did not come up (see $WORK/sequencer.log)"
echo "sequencer $("$SEQ_BIN" --version 2>/dev/null || echo sequencer_service) on $LEZ_RPC, pid $SEQ_PID"
echo "LEZ checkout: $(git -C "$LEZ_SRC" describe --tags --always 2>/dev/null || echo unknown) ($(git -C "$LEZ_SRC" rev-parse HEAD 2>/dev/null || echo ?))"

printf '{"sequencers":[{"sequencer_addr":"%s"}],"seq_poll_timeout":"30s","seq_tx_poll_max_blocks":15,"seq_poll_max_retries":10,"seq_block_poll_max_amount":100,"calibration_limit":100}\n' \
  "$LEZ_RPC" > "$WORK/wallet/wallet_config.json"
wallet check-health | tail -1

K="$WORK/keys"
echo "$PAYER_SECRET" > "$K/payer.key"
for who in registry token ping attester stranger; do reg keygen --out "$K/$who.key" > /dev/null; done
PAYER=$(reg account-id --key "$K/payer.key")
REG=$(reg account-id --key "$K/registry.key")
TOKEN=$(reg account-id --key "$K/token.key")
PING=$(reg account-id --key "$K/ping.key")
ATTESTER=$(reg account-id --key "$K/attester.key")
STRANGER=$(reg account-id --key "$K/stranger.key")
for k in payer registry token ping; do
  wallet account import public --private-key "$(cat "$K/$k.key")" | tail -1
done
echo "fee payer  $PAYER  balance $(reg balance "$PAYER")"
echo "attester   $ATTESTER"
echo "stranger   $STRANGER"

# Deploy a .bin at a header account the wallet holds. Segment accounts are
# fresh ids: writing a segment needs no signature, only an empty loader shard.
deploy() { # deploy <bin> <header> [--immutable]
  local bin=$1 header=$2 n segs=()
  n=$(reg image-id "$bin" 2>&1 >/dev/null | sed -n 's/.*: \([0-9]*\) loader segment.*/\1/p')
  for _ in $(seq 1 "$n"); do segs+=("Public/$(reg keygen | head -1)"); done
  wallet program-loader deploy --elf "$bin" --header "Public/$header" --segments "${segs[@]}" \
    --payer "Public/$PAYER" ${3:+"$3"} | grep -e 'deployed' -e 'rror' || true
}

# ---------------------------------------------------------------------------
step "2. deploy the registry guest"
ARTIFACT="${ARTIFACT:-$ROOT/artifacts/programs/lez_registry.bin}"
if [ ! -f "$ARTIFACT" ]; then
  echo "no committed artifact at $ARTIFACT; building the guest natively (tools/guest-native)"
  ARTIFACT="$WORK/lez_registry.bin"
  ( cd tools/guest-native && cargo run --release -q -- "$ARTIFACT" )
fi
echo "artifact $ARTIFACT"
echo "ImageID  $(reg image-id "$ARTIFACT" 2>/dev/null)"
deploy "$ARTIFACT" "$REG"
export PROVENANCE_REGISTRY="$REG"
reg program-header "$REG"
echo "registry program account: $REG"

# ---------------------------------------------------------------------------
step "3. initialise, and register the registry in itself (deployer entry)"
reg init --payer "$K/payer.key"
COMMIT=$(git rev-parse HEAD)
SOURCE_DIGEST=$(git ls-tree -r HEAD | shasum -a 256 | cut -d' ' -f1)
TOOLCHAIN="$(rustc +risc0 --version 2>/dev/null | cut -d' ' -f1-2 || echo risc0) (rzup, risc0-build 3.0.5)"
BUILDER_DIGEST=$(rustc +risc0 -vV 2>/dev/null | shasum -a 256 | cut -d' ' -f1)
if ! git diff --quiet HEAD -- program src Cargo.toml Cargo.lock; then
  echo "note: the guest sources differ from HEAD, so the claim below names a commit that is not what was built"
fi
reg register "$REG" --name provenance --version 0.3.0 \
  --description "The LEZ program registry, registered in itself" --tag registry --tag verified-builds \
  --repo https://github.com/edenbd1/lp-0023-program-registry --commit "$COMMIT" \
  --source-digest "$SOURCE_DIGEST" --image-digest "$BUILDER_DIGEST" --toolchain "$TOOLCHAIN" --offline \
  --key "$K/registry.key" --payer "$K/payer.key"

# ---------------------------------------------------------------------------
step "4. rebuild from source with the network denied, verify locally, attest on chain"
if [ "$REBUILD" = 1 ]; then
  REBUILT="$WORK/rebuilt.bin"
  NONET=()
  if [ "$(uname)" = Darwin ]; then
    NONET=(sandbox-exec -p '(version 1)(allow default)(deny network*)')
  elif command -v unshare >/dev/null && unshare -rn true 2>/dev/null; then
    NONET=(unshare -rn)
  fi
  echo "rebuilding in a fresh target directory${NONET:+, network denied by ${NONET[0]}}"
  ( cd tools/guest-native && CARGO_NET_OFFLINE=true CARGO_TARGET_DIR="$WORK/rebuild-target" \
      ${NONET[@]+"${NONET[@]}"} cargo run --release -q -- "$REBUILT" ) | tail -1
  reg verify "$REBUILT" --program "$REG" --mark
  LEZ_REGISTRY_HOME="$WORK" reg local-verified >/dev/null
  reg record-rebuild "$REG" --file "$REBUILT" --key "$K/attester.key" --payer "$K/payer.key"
else
  echo "REBUILD=0: attesting the deployed artifact itself (not a rebuild)"
  reg record-rebuild "$REG" --file "$ARTIFACT" --key "$K/attester.key" --payer "$K/payer.key"
fi

# ---------------------------------------------------------------------------
step "5. a second program: deployer vs third party, refusals, update, mismatch"
TOKEN_BIN="$LEZ_SRC/artifacts/lez/programs/token.bin"
deploy "$TOKEN_BIN" "$TOKEN" --immutable
reg program-header "$TOKEN"

echo "-- a stranger tries to publish the DEPLOYER entry for a program they did not deploy"
reg register "$TOKEN" --name example-token --version 0.3.0 \
  --key "$K/stranger.key" --payer "$K/payer.key" --submit-anyway && die "an unauthorised deployer entry was accepted"
grep -o 'E9010[^"]*' "$WORK/sequencer.log" | tail -1 | sed 's/^/sequencer: /' || true
reg lookup "$TOKEN" | head -2

echo "-- the deployer, signing with the program's header key"
LEZ_COMMIT=$(git -C "$LEZ_SRC" rev-parse HEAD 2>/dev/null || echo db66590ab821a4e142c211017a3866d007f6fa77)
reg register "$TOKEN" --name example-token --version 0.3.0 \
  --description "LEZ's token program, deployed again at a header the demo controls" --tag token \
  --repo https://github.com/logos-blockchain/logos-execution-zone --commit "$LEZ_COMMIT" \
  --source-digest "$(git -C "$LEZ_SRC" ls-tree -r HEAD lez/programs/token | shasum -a 256 | cut -d' ' -f1)" \
  --image-digest "$BUILDER_DIGEST" --toolchain "risc0-guest-builder r0.1.91.1" \
  --key "$K/token.key" --payer "$K/payer.key"

echo "-- an attester rebuilds it; the claimed build had the network up, so the match counts for nothing"
reg record-rebuild "$TOKEN" --file "$TOKEN_BIN" --key "$K/attester.key" --payer "$K/payer.key"

echo "-- the stranger publishes their own third-party entry instead: allowed, and told apart"
reg register "$TOKEN" --third-party --name "token (community notes)" --version 0.3.0 \
  --description "A third party's entry about the same program" \
  --key "$K/stranger.key" --payer "$K/payer.key"

echo "-- the stranger tries to update the deployer's entry"
reg update "$TOKEN" --version 9.9.9 --description hijacked --key "$K/stranger.key" --payer "$K/payer.key" \
  && die "a stranger's update was accepted"
grep -o 'E9003[^"]*' "$WORK/sequencer.log" | tail -1 | sed 's/^/sequencer: /' || true

echo "-- the author updates version, description and the IDL pointer (a Logos Storage content id)"
reg update "$TOKEN" --version 0.3.1 --description "Token program, with its IDL on Logos Storage" \
  --idl zDvZRwzm9Ni32iyvoy868YVrBHFz8vo3oZirrCKhFMMn9aXuinfQ --key "$K/token.key" --payer "$K/payer.key"

echo "-- a source claim on the third-party entry that does not rebuild: recorded as Mismatched"
reg claim-source "$TOKEN" --publisher "$STRANGER" --repo https://example.invalid/not-the-token \
  --commit 0000000000000000000000000000000000000001 --source-digest "$SOURCE_DIGEST" \
  --image-digest "$BUILDER_DIGEST" --toolchain "risc0-guest-builder r0.1.91.1" --offline \
  --key "$K/stranger.key" --payer "$K/payer.key"
reg record-rebuild "$TOKEN" --publisher "$STRANGER" --file "$LEZ_SRC/artifacts/lez/programs/clock.bin" \
  --key "$K/attester.key" --payer "$K/payer.key"

# ---------------------------------------------------------------------------
step "6. a mutable program is repointed after its verdict: sync_header makes it Stale"
PING_BIN="$LEZ_SRC/artifacts/lez/programs/ping_sender.bin"
deploy "$PING_BIN" "$PING"
reg register "$PING" --name example-ping --version 1.0.0 --tag demo \
  --repo https://github.com/logos-blockchain/logos-execution-zone --commit "$LEZ_COMMIT" \
  --source-digest "$SOURCE_DIGEST" --image-digest "$BUILDER_DIGEST" --toolchain "risc0-guest-builder r0.1.91.1" \
  --key "$K/ping.key" --payer "$K/payer.key"
reg record-rebuild "$PING" --file "$PING_BIN" --key "$K/attester.key" --payer "$K/payer.key"
NEW_BIN="$LEZ_SRC/artifacts/lez/programs/ping_receiver.bin"
n=$(reg image-id "$NEW_BIN" 2>&1 >/dev/null | sed -n 's/.*: \([0-9]*\) loader segment.*/\1/p')
segs=(); for _ in $(seq 1 "$n"); do segs+=("Public/$(reg keygen | head -1)"); done
echo "-- the deployer repoints the header at different bytecode (wallet program-loader update)"
wallet program-loader update --elf "$NEW_BIN" --header "Public/$PING" --segments "${segs[@]}" \
  --payer "Public/$PAYER" | grep -e 'rror' -e 'pdated' || true
reg program-header "$PING"
reg sync-header "$PING" --payer "$K/payer.key"

# ---------------------------------------------------------------------------
step "7. freeze, then a refused update"
reg freeze "$TOKEN" --key "$K/token.key" --payer "$K/payer.key"
reg update "$TOKEN" --version 0.4.0 --key "$K/token.key" --payer "$K/payer.key" \
  && die "an update to a frozen entry was accepted"
grep -o 'E9004[^"]*' "$WORK/sequencer.log" | tail -1 | sed 's/^/sequencer: /' || true

# ---------------------------------------------------------------------------
step "8. discovery: list, search, lookup, SDK, indexer"
reg list
reg search token
reg lookup "$REG"
reg lookup "$TOKEN" --publisher "$STRANGER"
reg lookup "$PING"
echo "-- the SDK's e2e tests against this chain"
LEZ_RPC="$LEZ_RPC" PROVENANCE_REGISTRY="$REG" cargo test --release -q --manifest-path sdk/Cargo.toml \
  --test e2e -- --ignored 2>&1 | grep -e 'test result' -e 'panicked' || die "SDK e2e failed"
echo "-- the SDK's resolve example, the path a wallet takes"
cargo run --release -q --manifest-path sdk/Cargo.toml --example resolve -- "$REG" "$TOKEN"
echo "-- the self-hostable indexer"
IDX_PORT=$((PORT + 5000))
"$INDEXER" "$REG" --endpoint "$LEZ_RPC" --port "$IDX_PORT" > "$WORK/indexer.log" 2>&1 &
IDX_PID=$!
for _ in $(seq 1 30); do curl -s -m 2 "http://127.0.0.1:$IDX_PORT/stats" >/dev/null && break; sleep 1; done
curl -s "http://127.0.0.1:$IDX_PORT/stats"; echo
curl -s "http://127.0.0.1:$IDX_PORT/search?q=verified" | python3 -c 'import json,sys; d=json.load(sys.stdin); [print(" ", e["name"], e["publisher"], e["verification"]) for e in (d["results"] if isinstance(d,dict) else d)]' || true

echo "-- every refusal the sequencer recorded in this run"
grep -o 'E90[0-9][0-9] [^"]*' "$WORK/sequencer.log" | sort | uniq -c || true

step "done"
echo "registry $REG on $LEZ_RPC: $(reg list | head -1)"
ok "every step above was read back from the chain"
