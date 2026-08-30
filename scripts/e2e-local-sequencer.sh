#!/usr/bin/env bash
# Full registry lifecycle against a REAL LEZ sequencer running locally in
# standalone mode, with RISC0_DEV_MODE=0 — one command, from a clean clone.
#
# WHY THIS EXISTS SEPARATELY FROM demo.sh
#
# `demo.sh` is the read-only tour of the *public* testnet: it derives addresses,
# reads accounts back, and re-checks DEPLOYMENTS.md. It proves the live registry
# is real, but it deploys nothing and it does not touch a local chain.
#
# This script makes the other claim, the one criterion 33 asks for: it starts the
# actual `sequencer_service` binary in standalone mode on a throwaway local port,
# funds a genesis test signer, **deploys this registry's own guest to that local
# chain**, initialises it, **registers the canonical entry**, and reads the result
# straight back off the local sequencer — with RISC0_DEV_MODE=0 exported for every
# process in the pipeline, so no component anywhere takes a dev-mode skip path.
#
#   ./scripts/e2e-local-sequencer.sh            # deploy + register + verify, local
#   KEEP=1 ./scripts/e2e-local-sequencer.sh     # leave the sequencer up to poke at
#
# Env:
#   LEZ_SRC     checkout of logos-execution-zone that carries the prebuilt
#               sequencer_service + wallet (default: ./_external/lez)
#   PORT        sequencer RPC port (default: first free from 3141)
#   WALLET_BIN  override the wallet binary (see the macOS note below)
#   KEEP        set to 1 to leave the sequencer running for inspection
#
# WHAT RISC0_DEV_MODE=0 DOES, AND DOES NOT, BUY HERE — stated rather than glossed
#
# The registry is an all-*public* SPEL program: register/initialise/update are
# public transactions. LEZ executes a public transaction on the sequencer with
# risc0's `default_executor().execute()` — the real guest ELF, the real 32M-cycle
# limit, the real ImageID — but WITHOUT generating a per-transaction STARK
# (`lee/state_machine/src/program/mod.rs`, comment "Execute the program (without
# proving)"). Real STARK proving in LEZ is on the privacy-preserving path (the
# client-side circuit proof), which a registry has no reason to use. So there is
# no minutes-long proof to time here, and a run that finishes in seconds is
# correct rather than suspicious — the opposite of the privacy-path lifecycles
# where a fast approval would mean a faked proof.
#
# What RISC0_DEV_MODE=0 guarantees in this run is the "no skip path" half of the
# criterion: every process — the sequencer, `spel`, the wallet — runs with dev
# mode off, so nothing substitutes a fake receipt or a mocked verifier for the
# real thing. And the real thing is exercised end to end: the program is addressed
# by its true RISC0 ImageID (the sequencer recomputes it from the deployed ELF),
# and `register` is accepted only because the guest, executed for real, agrees
# that the supplied clock account is owned by the pinned LEZ clock ImageID and that
# every derived seed matches. Deploy it under dev mode and the same ImageID and the
# same checks apply; nothing is loosened. That is what makes this a real local
# end-to-end, and this header is where the nuance lives instead of in a PASS line
# that would otherwise overclaim.

set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# No dev-mode skip path, anywhere in the stack. Exported so it reaches the
# sequencer, spel and the wallet identically.
export RISC0_DEV_MODE=0

LEZ_SRC="${LEZ_SRC:-$ROOT/_external/lez}"
KEEP="${KEEP:-0}"

# The signing key of the genesis-funded test account, published in clear in LEZ's
# own standalone config (`supply_account`) and its justfile. A shared test
# account, not a secret, and it only exists on chains whose genesis lists it.
TEST_SIGNER_KEY=7f273098f25b71e6c005a9519f2678da8d1c7f01f6a27778e2d9948abdf901fb
TEST_SIGNER=CbgR6tj5kWx5oziiFptM7jMvrQeYY3Mzaao6ciuhSr2r

# The every-block LEZ clock account. Its address is fixed and identical on every
# LEZ chain (public testnet or a fresh standalone genesis), because the clock
# program is a genesis system program. `register` reads it for the timestamp and
# refuses anything the pinned clock ImageID does not own.
CLOCK=4BdcjoXkq786TMWcBGGHqcxeLYMZmn17rL4eM9ZyRWNU

# The guest deployed here is the committed sealed build — a real, non-dev-mode
# RISC0 guest, reproducible with `scripts/build-sealed.sh`. Its ImageID is
# re-checked against `spel program-id` before it is trusted, and it is deployed
# rather than rebuilt so this one-command demo needs no Docker.
ARTIFACT=artifacts/programs/lez_registry.bin
IDL=idl/lez_registry.idl.json
CLI=cli/target/release/lez-registry

die() { echo "error: $*" >&2; exit 1; }
say() { printf '\n\033[1m%s\033[0m\n' "$*"; }

command -v spel    >/dev/null || die "spel not on PATH"
command -v r0vm    >/dev/null || die "r0vm not on PATH (needed for real risc0 execution)"
command -v python3 >/dev/null || die "python3 not on PATH"
command -v curl    >/dev/null || die "curl not on PATH"
[ -f "$ARTIFACT" ] || die "no guest at $ARTIFACT — build it first with scripts/build-sealed.sh"
[ -f "$IDL" ]      || die "no IDL at $IDL"

[ -d "$LEZ_SRC" ] || die "no LEZ checkout at $LEZ_SRC (holds the standalone sequencer + wallet). Set LEZ_SRC."
CONFIG_SRC="$LEZ_SRC/lez/sequencer/service/configs/debug/sequencer_config.json"
[ -f "$CONFIG_SRC" ] || die "no standalone config at $CONFIG_SRC"

SEQ_BIN="$LEZ_SRC/target/release/sequencer_service"
if [ ! -x "$SEQ_BIN" ]; then
  say "building the standalone sequencer (once; a few minutes)"
  ( cd "$LEZ_SRC" && cargo build --release --features standalone -p sequencer_service ) \
    || die "sequencer build failed"
fi
# WALLET_BIN is overridable because building the LEZ wallet fails on some macOS
# installs (missing Metal toolchain); any wallet from LEZ v0.2.2+ works here.
WALLET_BIN="${WALLET_BIN:-$LEZ_SRC/target/release/wallet}"
if [ ! -x "$WALLET_BIN" ]; then
  ( cd "$LEZ_SRC" && cargo build --release -p wallet ) \
    || die "wallet build failed; point WALLET_BIN at a wallet from LEZ v0.2.2+"
fi

# The wallet links Python 3.9 and dies without this. Exporting it is not enough:
# macOS SIP strips DYLD_* whenever bash execs another script, so it is set on the
# wallet's own exec. `spel` is a plain binary and does not need it.
WALLET_ENV=()
if [ "$(uname)" = "Darwin" ]; then
  WALLET_ENV=(env "DYLD_FALLBACK_FRAMEWORK_PATH=${DYLD_FALLBACK_FRAMEWORK_PATH:-/Library/Developer/CommandLineTools/Library/Frameworks}")
fi
wallet_run() { "${WALLET_ENV[@]}" "$WALLET_BIN" "$@"; }

# The CLI is part of the deliverable and builds the register transaction in
# process. Build it if the clone has not yet.
if [ ! -x "$CLI" ]; then
  say "building the registry CLI (once)"
  cargo build --release --manifest-path cli/Cargo.toml || die "CLI build failed"
fi

# A free port, so this never fights a sequencer already up.
PORT="${PORT:-}"
if [ -z "$PORT" ]; then
  for p in $(seq 3141 3200); do
    if ! nc -z localhost "$p" 2>/dev/null; then PORT=$p; break; fi
  done
fi
[ -n "$PORT" ] || die "no free port in 3141-3200"

WORK="$(mktemp -d)"
SEQ_HOME="$WORK/sequencer"; mkdir -p "$SEQ_HOME"
WALLET_HOME="$WORK/wallet"; mkdir -p "$WALLET_HOME"
RPC="http://localhost:$PORT"
SEQ_PID=""

cleanup() {
  if [ -n "$SEQ_PID" ] && [ "$KEEP" != "1" ]; then
    kill "$SEQ_PID" 2>/dev/null
    wait "$SEQ_PID" 2>/dev/null
  fi
  if [ "$KEEP" = "1" ]; then
    echo
    echo "left running: sequencer pid $SEQ_PID on $RPC"
    echo "  wallet home $WALLET_HOME"
    echo "  logs        $WORK/sequencer.log"
  else
    rm -rf "$WORK"
  fi
}
trap cleanup EXIT INT TERM

# ── curl helpers against the LOCAL sequencer ────────────────────────────────
rpc_get_account() { # base58 -> raw JSON result
  curl -s -m 5 -X POST "$RPC" -H 'Content-Type: application/json' \
    -d '{"jsonrpc":"2.0","id":1,"method":"getAccount","params":["'"$1"'"]}'
}
tx_present() { # hash -> "yes"/"no"
  curl -s -m 8 -X POST "$RPC" -H 'Content-Type: application/json' \
    -d '{"jsonrpc":"2.0","id":1,"method":"getTransaction","params":["'"$1"'"]}' \
    | python3 -c 'import sys,json;print("yes" if json.load(sys.stdin).get("result") else "no")' 2>/dev/null
}

say "[1/7] a real sequencer in standalone mode on $RPC (RISC0_DEV_MODE=0)"
python3 - "$CONFIG_SRC" "$SEQ_HOME" <<'PY'
import json, sys
cfg = json.load(open(sys.argv[1]))
cfg["home"] = sys.argv[2]
json.dump(cfg, open(sys.argv[2] + "/sequencer_config.json", "w"), indent=2)
PY
RUST_LOG=info "$SEQ_BIN" --port "$PORT" "$SEQ_HOME/sequencer_config.json" \
  > "$WORK/sequencer.log" 2>&1 &
SEQ_PID=$!
for _ in $(seq 1 60); do
  if rpc_get_account "$TEST_SIGNER" | grep -q '"result"'; then
    echo "  up, pid $SEQ_PID"; break
  fi
  kill -0 "$SEQ_PID" 2>/dev/null || { tail -20 "$WORK/sequencer.log"; die "sequencer died on startup"; }
  sleep 1
done
rpc_get_account "$TEST_SIGNER" | grep -q '"result"' || die "sequencer never answered on $RPC"

# The clock the registry will read must be present and owned by the pinned LEZ
# clock ImageID on this fresh chain, or register cannot land. Assert it now so a
# missing clock fails here with a clear message rather than deep in register.
CLOCK_OWNER=$(rpc_get_account "$CLOCK" \
  | python3 -c 'import sys,json;r=json.load(sys.stdin).get("result") or {};print(",".join(map(str,r.get("program_owner",[]))))' 2>/dev/null)
EXPECT_CLOCK_OWNER="96247601,2082502477,822865082,1048693993,3544189898,772921104,1694408900,4234239033"
[ "$CLOCK_OWNER" = "$EXPECT_CLOCK_OWNER" ] \
  || die "clock account $CLOCK is not owned by the pinned LEZ clock program on this chain (owner=$CLOCK_OWNER)"
echo "  clock $CLOCK present, owned by the pinned LEZ clock ImageID"

say "[2/7] a throwaway wallet pointed at it, signer imported"
cat > "$WALLET_HOME/wallet_config.json" <<EOF
{ "sequencers": [ { "sequencer_addr": "$RPC/" } ],
  "seq_poll_timeout": "30s", "seq_tx_poll_max_blocks": 15,
  "seq_poll_max_retries": 10, "seq_block_poll_max_amount": 100 }
EOF
export LEE_WALLET_HOME_DIR="$WALLET_HOME"
printf 'lp0023\n' | wallet_run account import public --private-key "$TEST_SIGNER_KEY" >/dev/null 2>&1 \
  || die "could not import the test signer"
echo "  imported Public/$TEST_SIGNER"

say "[3/7] fund it from the genesis vault"
wallet_run vault claim --account-id "Public/$TEST_SIGNER" --amount 5000 </dev/null >/dev/null 2>&1
bal=0
for _ in $(seq 1 30); do
  bal=$(rpc_get_account "$TEST_SIGNER" \
    | python3 -c 'import sys,json;print(json.load(sys.stdin).get("result",{}).get("balance",0))' 2>/dev/null)
  [ "${bal:-0}" -gt 0 ] 2>/dev/null && { echo "  balance $bal"; break; }
  sleep 2
done
[ "${bal:-0}" -gt 0 ] 2>/dev/null || die "vault claim did not land"

# The registry program id is its RISC0 ImageID, recomputed from the artifact —
# never written down, so a swapped binary changes this and everything keyed on it.
REGISTRY=$(spel program-id "$ARTIFACT" | grep -oE 'ImageID \(hex bytes\): [0-9a-f]{64}' | grep -oE '[0-9a-f]{64}')
[ -n "$REGISTRY" ] || die "could not read the registry ImageID from $ARTIFACT"
DIGEST=$("$CLI" deployment-digest "$ARTIFACT" | grep -oE '^[0-9a-f]{64}' | head -1)
[ -n "$DIGEST" ] || die "could not compute the deployment digest"
echo "  registry ImageID   $REGISTRY"
echo "  deployment digest  $DIGEST  (== the deploy transaction hash)"

say "[4/7] deploy this registry's guest to the LOCAL sequencer"
# Content-addressed: the deploy transaction hash IS the deployment digest, so the
# only honest test that it landed is getTransaction on that digest.
if [ "$(tx_present "$DIGEST")" = yes ]; then
  echo "  already on chain  $DIGEST"
else
  deployed=no
  for attempt in 1 2 3; do
    wallet_run deploy-program "$ARTIFACT" </dev/null > "$WORK/deploy.out" 2>&1
    for _ in $(seq 1 20); do
      [ "$(tx_present "$DIGEST")" = yes ] && { deployed=yes; break; }
      sleep 2
    done
    [ "$deployed" = yes ] && break
    echo "  attempt $attempt did not land, retrying" >&2
  done
  [ "$deployed" = yes ] || { tail -10 "$WORK/deploy.out" >&2; die "deploy did not land on $RPC"; }
  echo "  deployed  $DIGEST"
fi

say "[5/7] initialise the singleton global state"
GLOBAL=$(python3 scripts/derive.py "$REGISTRY" global)
spel --idl "$IDL" --program "$REGISTRY" -- initialize </dev/null > "$WORK/init.out" 2>&1
if grep -qE '✅' "$WORK/init.out"; then
  echo "  initialized, global state at $GLOBAL"
else
  # Tolerate an already-initialised chain (re-run with KEEP=1), but a genuine
  # failure on a fresh chain must be loud.
  if rpc_get_account "$GLOBAL" | python3 -c 'import sys,json;r=json.load(sys.stdin).get("result") or {};sys.exit(0 if r.get("data") else 1)' 2>/dev/null; then
    echo "  already initialized, global state at $GLOBAL"
  else
    tail -15 "$WORK/init.out" >&2; die "initialize did not land"
  fi
fi

say "[6/7] register the canonical entry (the registry, in itself)"
# 6a. Build the transaction IN PROCESS with the CLI — no spel, no IDL in the
#     construction path. This is the deliverable SDK builder, and it re-derives
#     the entry address from the program id alone, reading the local chain for the
#     directory slot. What it prints is what a wallet would sign.
say "  6a. in-process CLI build (accounts + risc0-serde words, unsigned)"
"$CLI" register "$REGISTRY" \
  --name "lez-program-registry" --version "0.7.0" \
  --description "The registry itself, registered in itself." \
  --author "$TEST_SIGNER" --clock "$CLOCK" \
  --registry "$REGISTRY" --endpoint "$RPC" --seq-index 0 \
  | sed 's/^/     /' || die "CLI could not build the register transaction"

# The entry address the CLI derives — the one the built transaction targets and
# the one a resolve will read. Recomputed here so the assertions below compare
# against a value the tool produced, not a literal.
ENTRY=$("$CLI" entry-address "$REGISTRY" --registry "$REGISTRY" | head -1)
ENTRY_SEED=$("$CLI" entry-address "$REGISTRY" --registry "$REGISTRY" | sed -n 's/^ *seed *//p')
SEQ_SEED=$(python3 -c "import hashlib,struct;print(hashlib.sha256(b'lez-registry:seq:v1'+struct.pack('<Q',0)).hexdigest())")
echo "     entry address $ENTRY"

# 6b. Submit it. The CLI stops at the signature on purpose (a key belongs where an
#     operator sees what they sign), so the signed submission goes through the
#     wallet-backed `spel`, which lands exactly the transaction the CLI built:
#     same entry seed, same program id, same account plan.
say "  6b. sign + submit via the wallet-backed spel"
spel --idl "$IDL" --program "$REGISTRY" -- register \
  --entry-seed "$ENTRY_SEED" --seq-seed "$SEQ_SEED" --program-id "$REGISTRY" \
  --canonical 1 --name "lez-program-registry" --version "0.7.0" \
  --description "The registry itself, registered in itself." \
  --clock "$CLOCK" --author "$TEST_SIGNER" </dev/null > "$WORK/register.out" 2>&1
grep -qE '✅' "$WORK/register.out" || { tail -20 "$WORK/register.out" >&2; die "register did not land"; }
echo "     registered"

say "[7/7] verify end-to-end, against the LOCAL sequencer"
fail=0
ok()   { printf '  OK   %-26s %s\n' "$1" "$2"; }
bad()  { printf '  FAIL %-26s %s\n' "$1" "$2"; fail=1; }

# (a) the deployed program is addressed by the artifact's true ImageID
FROM_ELF=$(spel program-id "$ARTIFACT" | grep -oE 'ImageID \(hex bytes\): [0-9a-f]{64}' | grep -oE '[0-9a-f]{64}')
[ "$FROM_ELF" = "$REGISTRY" ] && ok "program id == artifact ImageID" "${REGISTRY:0:16}..." \
                             || bad "program id" "artifact=$FROM_ELF registry=$REGISTRY"

# (b) the deploy transaction (== deployment digest) exists on THIS local chain
[ "$(tx_present "$DIGEST")" = yes ] && ok "deploy digest is a local tx" "${DIGEST:0:16}..." \
                                    || bad "deploy digest" "$DIGEST not on $RPC"

# (c) the entry resolves locally, and decodes to the right program
ENTRY_HEX=$(rpc_get_account "$ENTRY" \
  | python3 -c 'import sys,json;d=(json.load(sys.stdin).get("result") or {}).get("data");print(bytes(d)[4:].hex() if d else "")' 2>/dev/null)
if [ -n "$ENTRY_HEX" ]; then
  ok "entry account holds data" "$ENTRY"
  DECODED=$("$CLI" explain-entry "$ENTRY_HEX" 2>/dev/null)
  echo "$DECODED" | grep -q "program id   $REGISTRY" \
    && ok "entry program id matches" "self-registered" \
    || bad "entry program id" "decoded entry does not name $REGISTRY"
  echo "$DECODED" | sed 's/^/       /'
else
  bad "entry account" "nothing at $ENTRY on $RPC"
fi

# (d) the resolve path (SDK client) resolves it against the local endpoint
if "$CLI" lookup "$REGISTRY" --registry "$REGISTRY" --endpoint "$RPC" 2>/dev/null \
     | grep -q "lez-program-registry"; then
  ok "CLI lookup resolves locally" "one hash, one read"
else
  bad "CLI lookup" "did not resolve the entry against $RPC"
fi

# (e) the global counter moved
TOTAL=$(rpc_get_account "$GLOBAL" \
  | python3 -c 'import sys,json;d=(json.load(sys.stdin).get("result") or {}).get("data");print(int.from_bytes(bytes(d)[0:8],"little") if d else 0)' 2>/dev/null)
[ "${TOTAL:-0}" -ge 1 ] 2>/dev/null && ok "global total_entries" "$TOTAL" \
                                    || bad "global total_entries" "expected >= 1, got ${TOTAL:-0}"

# (f) controls — hashes never deployed must return null, or the endpoint answers
#     "yes" to anything and the checks above prove nothing.
[ "$(tx_present 0000000000000000000000000000000000000000000000000000000000000000)" = no ] \
  && ok "control (all-zeros tx)" "returns null, as it must" \
  || bad "control (all-zeros tx)" "resolved — endpoint answers yes to anything"
[ "$(tx_present ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff)" = no ] \
  && ok "control (all-ffs tx)" "returns null, as it must" \
  || bad "control (all-ffs tx)" "resolved — endpoint answers yes to anything"

echo
if [ "$fail" = 0 ]; then
  printf '\033[1mlocal-sequencer e2e: PASS\033[0m  (%s, RISC0_DEV_MODE=%s)\n' "$RPC" "$RISC0_DEV_MODE"
  echo "  deployed + registered + resolved the registry on a throwaway local LEZ chain,"
  echo "  with dev mode off in every process — no skip path taken."
else
  echo "local-sequencer e2e: FAIL" >&2
fi
exit $fail
