#!/usr/bin/env bash
# Fill a LOCAL standalone LEZ sequencer with at least 500 registry entries and
# measure lookup + search latency across them — the honest way to close the
# "search across a registry of at least 500 entries" criterion.
#
# WHY THIS IS LOCAL, AND WHY THAT IS THE FAITHFUL MEASUREMENT
#
# LATENCY.md already times 500 *lookups* against the public testnet and shows
# resolution is size-independent by construction. What it could not show is
# *search* at scale, because search walks the sequence directory and the public
# registry holds a handful of entries — a registration is a signed transaction
# that confirms in about a minute on that endpoint, so 500 of them is most of a
# working day of block time and a lot of public traffic for a number a local
# chain answers exactly. So this stands up the same real `sequencer_service` the
# criterion-33 e2e uses, in standalone mode on a throwaway port, registers ≥500
# distinct programs, and measures over them. The chain is local; the code paths
# — the SDK `Client`'s `entry` (resolve) and `walk` (enumerate/search) — are the
# same ones the explorer and the indexer run against the public testnet.
#
#   ./scripts/populate-and-measure-500.sh          # 500 entries, then measure
#   N=520 ./scripts/populate-and-measure-500.sh    # register a different count
#   KEEP=1 ./scripts/populate-and-measure-500.sh   # leave the sequencer up
#
# Env:
#   LEZ_SRC        checkout carrying the prebuilt sequencer_service + wallet
#                  (default: ./_external/lez)
#   N              entries to register (default 500)
#   PORT           sequencer RPC port (default: first free from 3141)
#   BLOCK_TIMEOUT  local block cadence (default 500ms). Registration is
#                  counter-dependent — each register targets the slot the
#                  previous one incremented into — so it is serial and paced by
#                  the block time. This tunes ONLY the local chain's block
#                  cadence to make populating 500 entries take minutes rather
#                  than hours; it changes nothing about how search is measured
#                  over them.
#   WALLET_BIN     override the wallet binary (macOS Metal-toolchain note below)
#   KEEP           set to 1 to leave the sequencer running for inspection
#
# RISC0_DEV_MODE=0 is exported for every process, exactly as the e2e does: no
# component takes a dev-mode skip path. As with the e2e, the registry is an
# all-public program, so the sequencer executes each transaction for real
# (real guest ELF, real 32M-cycle limit) without a per-transaction STARK — a
# register that lands in milliseconds is correct, not suspicious.

set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

export RISC0_DEV_MODE=0

LEZ_SRC="${LEZ_SRC:-$ROOT/_external/lez}"
N="${N:-500}"
KEEP="${KEEP:-0}"
BLOCK_TIMEOUT="${BLOCK_TIMEOUT:-500ms}"

# The genesis-funded standalone test signer, published in clear in LEZ's own
# config. A shared test account, not a secret, present only on a fresh local
# genesis. It authors every entry here.
TEST_SIGNER_KEY=7f273098f25b71e6c005a9519f2678da8d1c7f01f6a27778e2d9948abdf901fb
TEST_SIGNER=CbgR6tj5kWx5oziiFptM7jMvrQeYY3Mzaao6ciuhSr2r
AUTHOR="$TEST_SIGNER"

# The every-block LEZ clock account, fixed on every LEZ chain. register reads it
# for the timestamp and refuses anything the pinned clock ImageID does not own.
CLOCK=4BdcjoXkq786TMWcBGGHqcxeLYMZmn17rL4eM9ZyRWNU

ARTIFACT=artifacts/programs/lez_registry.bin
IDL=idl/lez_registry.idl.json
CLI=cli/target/release/lez-registry

die() { echo "error: $*" >&2; exit 1; }
say() { printf '\n\033[1m%s\033[0m\n' "$*"; }

command -v spel    >/dev/null || die "spel not on PATH"
command -v r0vm    >/dev/null || die "r0vm not on PATH (needed for real risc0 execution)"
command -v python3 >/dev/null || die "python3 not on PATH"
command -v curl    >/dev/null || die "curl not on PATH"
command -v cargo   >/dev/null || die "cargo not on PATH"
[ -f "$ARTIFACT" ] || die "no guest at $ARTIFACT — build it first with scripts/build-sealed.sh"
[ -f "$IDL" ]      || die "no IDL at $IDL"

[ -d "$LEZ_SRC" ] || die "no LEZ checkout at $LEZ_SRC (holds the standalone sequencer + wallet). Set LEZ_SRC."
CONFIG_SRC="$LEZ_SRC/lez/sequencer/service/configs/debug/sequencer_config.json"
[ -f "$CONFIG_SRC" ] || die "no standalone config at $CONFIG_SRC"

SEQ_BIN="$LEZ_SRC/target/release/sequencer_service"
[ -x "$SEQ_BIN" ] || die "no sequencer_service at $SEQ_BIN (build it: cargo build --release --features standalone -p sequencer_service in \$LEZ_SRC)"
WALLET_BIN="${WALLET_BIN:-$LEZ_SRC/target/release/wallet}"
[ -x "$WALLET_BIN" ] || die "no wallet at $WALLET_BIN (set WALLET_BIN to a wallet from LEZ v0.2.2+)"

# The wallet links Python 3.9 and dies without this; macOS SIP strips DYLD_* on
# every exec, so it is set on the wallet's own exec rather than exported.
WALLET_ENV=()
if [ "$(uname)" = "Darwin" ]; then
  WALLET_ENV=(env "DYLD_FALLBACK_FRAMEWORK_PATH=${DYLD_FALLBACK_FRAMEWORK_PATH:-/Library/Developer/CommandLineTools/Library/Frameworks}")
fi
wallet_run() { "${WALLET_ENV[@]}" "$WALLET_BIN" "$@"; }

if [ ! -x "$CLI" ]; then
  say "building the registry CLI (once)"
  cargo build --release --manifest-path cli/Cargo.toml || die "CLI build failed"
fi
say "building the bench_500 measurement example (once)"
cargo build --release --manifest-path sdk/Cargo.toml --example bench_500 || die "bench build failed"
BENCH="sdk/target/release/examples/bench_500"
[ -x "$BENCH" ] || die "bench_500 did not build to $BENCH"

# A free port so this never fights a sequencer already up.
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

rpc_get_account() { # base58 -> raw JSON result
  curl -s -m 5 -X POST "$RPC" -H 'Content-Type: application/json' \
    -d '{"jsonrpc":"2.0","id":1,"method":"getAccount","params":["'"$1"'"]}'
}
tx_present() { # hash -> "yes"/"no"
  curl -s -m 8 -X POST "$RPC" -H 'Content-Type: application/json' \
    -d '{"jsonrpc":"2.0","id":1,"method":"getTransaction","params":["'"$1"'"]}' \
    | python3 -c 'import sys,json;print("yes" if json.load(sys.stdin).get("result") else "no")' 2>/dev/null
}

say "[1/8] a real sequencer in standalone mode on $RPC (RISC0_DEV_MODE=0, block $BLOCK_TIMEOUT)"
# Copy the shipped standalone config, point home at the throwaway dir, and tune
# only the block cadence — see the header on why that does not touch the search
# measurement.
python3 - "$CONFIG_SRC" "$SEQ_HOME" "$BLOCK_TIMEOUT" <<'PY'
import json, sys
cfg = json.load(open(sys.argv[1]))
cfg["home"] = sys.argv[2]
cfg["block_create_timeout"] = sys.argv[3]
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

say "[2/8] a throwaway wallet pointed at it, signer imported"
cat > "$WALLET_HOME/wallet_config.json" <<EOF
{ "sequencers": [ { "sequencer_addr": "$RPC/" } ],
  "seq_poll_timeout": "30s", "seq_tx_poll_max_blocks": 15,
  "seq_poll_max_retries": 10, "seq_block_poll_max_amount": 100 }
EOF
export LEE_WALLET_HOME_DIR="$WALLET_HOME"
printf 'lp0023\n' | wallet_run account import public --private-key "$TEST_SIGNER_KEY" >/dev/null 2>&1 \
  || die "could not import the test signer"
echo "  imported Public/$TEST_SIGNER"

say "[3/8] fund it from the genesis vault"
wallet_run vault claim --account-id "Public/$TEST_SIGNER" --amount 5000 </dev/null >/dev/null 2>&1
bal=0
for _ in $(seq 1 30); do
  bal=$(rpc_get_account "$TEST_SIGNER" \
    | python3 -c 'import sys,json;print(json.load(sys.stdin).get("result",{}).get("balance",0))' 2>/dev/null)
  [ "${bal:-0}" -gt 0 ] 2>/dev/null && { echo "  balance $bal"; break; }
  sleep 1
done
[ "${bal:-0}" -gt 0 ] 2>/dev/null || die "vault claim did not land"

REGISTRY=$(spel program-id "$ARTIFACT" | grep -oE 'ImageID \(hex bytes\): [0-9a-f]{64}' | grep -oE '[0-9a-f]{64}')
[ -n "$REGISTRY" ] || die "could not read the registry ImageID from $ARTIFACT"
DIGEST=$("$CLI" deployment-digest "$ARTIFACT" | grep -oE '^[0-9a-f]{64}' | head -1)
[ -n "$DIGEST" ] || die "could not compute the deployment digest"
echo "  registry ImageID  $REGISTRY"

say "[4/8] deploy this registry's guest to the LOCAL sequencer"
if [ "$(tx_present "$DIGEST")" = yes ]; then
  echo "  already on chain  $DIGEST"
else
  deployed=no
  for attempt in 1 2 3; do
    wallet_run deploy-program "$ARTIFACT" </dev/null > "$WORK/deploy.out" 2>&1
    for _ in $(seq 1 30); do
      [ "$(tx_present "$DIGEST")" = yes ] && { deployed=yes; break; }
      sleep 1
    done
    [ "$deployed" = yes ] && break
    echo "  attempt $attempt did not land, retrying" >&2
  done
  [ "$deployed" = yes ] || { tail -10 "$WORK/deploy.out" >&2; die "deploy did not land on $RPC"; }
  echo "  deployed  $DIGEST"
fi

say "[5/8] initialise the singleton global state"
GLOBAL=$(python3 scripts/derive.py "$REGISTRY" global)
spel --idl "$IDL" --program "$REGISTRY" -- initialize </dev/null > "$WORK/init.out" 2>&1
if grep -qE '✅' "$WORK/init.out"; then
  echo "  initialized, global state at $GLOBAL"
else
  if rpc_get_account "$GLOBAL" | python3 -c 'import sys,json;r=json.load(sys.stdin).get("result") or {};sys.exit(0 if r.get("data") else 1)' 2>/dev/null; then
    echo "  already initialized, global state at $GLOBAL"
  else
    tail -15 "$WORK/init.out" >&2; die "initialize did not land"
  fi
fi

total_entries() { # -> current total_entries off the global account
  rpc_get_account "$GLOBAL" \
    | python3 -c 'import sys,json;d=(json.load(sys.stdin).get("result") or {}).get("data");print(int.from_bytes(bytes(d)[0:8],"little") if d else 0)' 2>/dev/null
}

say "[6/8] precompute $N distinct entries (program id, seeds, name, description)"
# Each program id is sha256("lp0023-500-bench:" ‖ u64_le(i)) — the same rule
# bench_500 uses, so a measured "hit" reads an address really written here. The
# entry seed and the sequence seed are the program's own derivations, computed
# here so the register loop makes no per-entry CLI call.
SEEDS="$WORK/seeds.tsv"
python3 - "$N" > "$SEEDS" <<'PY'
import sys, hashlib, struct
N = int(sys.argv[1])
ENTRY = b'lez-registry:entry:v1'
SEQ = b'lez-registry:seq:v1'
for i in range(N):
    pid = hashlib.sha256(b'lp0023-500-bench:' + struct.pack('<Q', i)).digest()
    entry_seed = hashlib.sha256(ENTRY + pid).hexdigest()
    seq_seed = hashlib.sha256(SEQ + struct.pack('<Q', i)).hexdigest()
    name = 'bench-prog-%04d' % i
    desc = 'Synthetic entry %d in the 500-entry local search benchmark.' % i
    print('\t'.join([str(i), pid.hex(), entry_seed, seq_seed, name, desc]))
PY
[ "$(wc -l < "$SEEDS")" -eq "$N" ] || die "seed precompute did not produce $N rows"
echo "  $N rows in $SEEDS"

say "[7/8] register $N entries (serial: each targets the slot the last incremented)"
# spel blocks ~30s per tx on its own confirmation poll even though the chain
# includes the transaction in well under a second (measured: 0.7s to land vs
# 30s for spel to return). Registration is serial anyway — each register must
# target the slot the previous one incremented the counter into — so pace on the
# on-chain counter directly: submit in the background, poll total_entries, and
# stop spel's redundant wait the instant the counter moves. The submission is a
# completed network POST by then, so the transaction is already in a block.
START_TOTAL=$(total_entries)
echo "  starting from total_entries=$START_TOTAL"
reg_start=$(date +%s)
registered=0
while IFS=$'\t' read -r idx pid seed seqseed name desc; do
  cur=$(total_entries)
  if [ "${cur:-0}" -gt "$idx" ] 2>/dev/null; then
    continue  # a previous run already filled this slot
  fi
  landed=no
  for attempt in 1 2 3; do
    spel --idl "$IDL" --program "$REGISTRY" -- register \
      --entry-seed "$seed" --seq-seed "$seqseed" --program-id "$pid" \
      --canonical 1 --name "$name" --version "1.0.0" --description "$desc" \
      --clock "$CLOCK" --author "$AUTHOR" </dev/null > "$WORK/reg.out" 2>&1 &
    spid=$!
    for _ in $(seq 1 300); do
      cur=$(total_entries)
      [ "${cur:-0}" -ge "$((idx + 1))" ] 2>/dev/null && { landed=yes; break; }
      kill -0 "$spid" 2>/dev/null || break  # spel exited without landing → retry
      sleep 0.1
    done
    kill "$spid" 2>/dev/null; wait "$spid" 2>/dev/null
    [ "$landed" = yes ] && break
    echo "  slot $idx attempt $attempt did not land, retrying" >&2
  done
  [ "$landed" = yes ] || { tail -15 "$WORK/reg.out" >&2; die "register for slot $idx did not land"; }
  registered=$((registered + 1))
  if [ $(( (idx + 1) % 25 )) -eq 0 ]; then
    now=$(date +%s)
    printf '  %4d/%d registered  (%ds elapsed)\n' "$((idx + 1))" "$N" "$((now - reg_start))"
  fi
done < "$SEEDS"
reg_end=$(date +%s)
FINAL_TOTAL=$(total_entries)
echo "  registered $registered this run; total_entries=$FINAL_TOTAL in $((reg_end - reg_start))s"
[ "${FINAL_TOTAL:-0}" -ge 500 ] 2>/dev/null \
  || die "total_entries is $FINAL_TOTAL (< 500) — cannot claim search across ≥500 entries"

say "[8/8] measure lookup + search across $FINAL_TOTAL on-chain entries"
BENCH_ENDPOINT="$RPC" BENCH_REGISTRY="$REGISTRY" BENCH_N="$N" "$BENCH"

echo
printf '\033[1mpopulate-and-measure: total_entries=%s on %s (RISC0_DEV_MODE=%s)\033[0m\n' \
  "$FINAL_TOTAL" "$RPC" "$RISC0_DEV_MODE"
