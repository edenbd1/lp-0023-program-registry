#!/usr/bin/env bash
# Populate a fresh registry with entries that between them exercise every
# verification state, and do it with values that are true.
#
# Nothing here is staged. The Mismatched entry points at a real earlier commit of
# this repository that genuinely builds a different ImageID; the Unreproducible
# one records a build that genuinely had network access. A demo that faked either
# would be demonstrating the opposite of what this registry is for.
#
# Usage: REGISTRY=<program-id-hex> ./scripts/populate-demo.sh
set -uo pipefail
cd "$(dirname "$0")/.."

: "${REGISTRY:?set REGISTRY to the deployed registry program id (64 hex)}"
: "${LEE_WALLET_HOME_DIR:?set LEE_WALLET_HOME_DIR to a wallet directory}"
AUTHOR="${AUTHOR:-DumJ4LCBnHE9jUu2yxPfqdL14g3v756Gzby6LuT9hE51}"
CLOCK="${CLOCK:-4BdcjoXkq786TMWcBGGHqcxeLYMZmn17rL4eM9ZyRWNU}"
IDL=idl/lez_registry.idl.json
CLI=cli/target/release/lez-registry
BUILDER_DIGEST=3e12f71bacd27527a61dea96fa0e53e468c99aa261d3a1019b593f6dbd943eb3

command -v spel > /dev/null || { echo "spel not on PATH" >&2; exit 1; }
[ -x "$CLI" ] || { echo "build the CLI first: cargo build --release --manifest-path cli/Cargo.toml" >&2; exit 1; }

# The seed, not the account address: `spel --entry-seed` takes the seed and
# derives the address itself. These parsed line 1 back when line 1 was the seed;
# it is now the account, which is the more useful thing to lead with and would
# have silently sent every registration to an address the program refuses.
seed()     { "$CLI" entry-address "$1" | sed -n 's/^ *seed *//p'; }
tp_seed()  { "$CLI" entry-address "$1" --publisher "$2" | sed -n 's/^ *seed *//p'; }
seq_seed() { python3 -c "import hashlib,struct;print(hashlib.sha256(b'lez-registry:seq:v1'+struct.pack('<Q',$1)).hexdigest())"; }
run()      { spel --idl "$IDL" --program "$REGISTRY" -- "$@" 2>&1 | grep -qE '✅'; }

# This repository, as it stands. Not hardcoded: a hardcoded commit is a claim
# that goes stale the next time anyone commits.
COMMIT=$(git rev-parse HEAD)
DIGEST=$(git ls-files -z src program tests Cargo.toml Cargo.lock | sort -z \
         | xargs -0 shasum -a 256 | shasum -a 256 | cut -d' ' -f1)
AUTHOR_HEX=$(python3 - "$AUTHOR" <<'PY'
import sys
A='123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz'
n=0
for c in sys.argv[1]: n = n*58 + A.index(c)
print(n.to_bytes(32,'big').hex())
PY
)

CURVE=49db0fc91883668a5ccb85242aa1cdf923a557a7327f7c0613ad526fa56fc510
LBP=51f28557602ae6daff97e51127ba6dbc5ddfe6048ea7cd89ca5e170ab6c7a82d
VEST=4c6e62a585934f87ef919c63a252fb33faef14d9ed1af2bd17da62deaf93ea7f
CLOCKP=319fbc054d77207cbaec0b31e9cc813eca0b40d310d7112ec4a0fe64395c61fc

# A real earlier commit of this repository, and the ImageID it really produces.
# The pair is what makes the Mismatched entry honest rather than staged.
STALE_COMMIT=8dfa2f01302722ed49a3062f55a38aca9ea879bb
STALE_DIGEST=ae6f6022512f7820a57297ccbad8f864d0a245b624cb805371ad231458c19c39
STALE_BUILDS=e1484f1447edd7669d1443154841160f4ddfe4c5d93d410527e5cf99ab9aacba

step() { printf '  %-34s ' "$1"; }
ok()   { echo "$1"; }

echo "populating registry $REGISTRY"

step "initialize"
run initialize && ok "done" || ok "already initialized"

reg() { # seed seq program name version description canonical
  run register --entry-seed "$1" --seq-seed "$(seq_seed "$2")" --program-id "$3" \
      --canonical "$7" --name "$4" --version "$5" --description "$6" \
      --clock "$CLOCK" --author "$AUTHOR"
}

S0=$(seed "$REGISTRY")
step "0 registry (canonical)"
reg "$S0" 0 "$REGISTRY" "lez-program-registry" "0.7.0" \
    "The registry itself, registered in itself." 1 && ok "registered" || ok "FAILED"

step "0 claim + verify"
run claim-source --entry-seed "$S0" \
    --repository "https://github.com/edenbd1/lp-0023-program-registry" \
    --commit "$COMMIT" --source-digest "$DIGEST" --image-digest "$BUILDER_DIGEST" \
    --toolchain "1.88.0" --network-disabled 1 --author "$AUTHOR" \
  && run record-rebuild --entry-seed "$S0" --rebuilt "$REGISTRY" \
       --clock "$CLOCK" --attester "$AUTHOR" \
  && ok "VERIFIED" || ok "FAILED"

S1=$(seed "$CURVE")
step "1 antumbra-curve"
reg "$S1" 1 "$CURVE" "antumbra-curve" "0.1.0" \
    "Constant-product bonding curve on LEZ." 1 && ok "UNCLAIMED" || ok "FAILED"

S2=$(seed "$LBP")
step "2 antumbra-lbp"
reg "$S2" 2 "$LBP" "antumbra-lbp" "0.1.0" "Weighted LBP pool on LEZ." 1 || true
A_COMMIT=$(git -C ../antumbra-lez rev-parse HEAD 2>/dev/null || echo "")
if [ -n "$A_COMMIT" ]; then
  # Computed *inside* that repository. `git -C` prints paths relative to the
  # other repo while shasum resolves them against this one, so running the two
  # in different directories silently digests a list of missing files -- which is
  # exactly what the first run of this script recorded on chain.
  A_DIGEST=$(cd ../antumbra-lez && git ls-files -z src programs Cargo.toml Cargo.lock \
             | sort -z | xargs -0 shasum -a 256 | shasum -a 256 | cut -d' ' -f1)
  # network-disabled 0 because that build genuinely had network access. The
  # registry refuses to call it verified, which is the point of the entry.
  run claim-source --entry-seed "$S2" \
      --repository "https://github.com/edenbd1/antumbra-lez" \
      --commit "$A_COMMIT" --source-digest "$A_DIGEST" --image-digest "$BUILDER_DIGEST" \
      --toolchain "1.88.0" --network-disabled 0 --author "$AUTHOR" \
    && run record-rebuild --entry-seed "$S2" --rebuilt "$LBP" \
         --clock "$CLOCK" --attester "$AUTHOR" \
    && ok "UNREPRODUCIBLE" || ok "FAILED"
else
  ok "registered (antumbra-lez not beside this repo, claim skipped)"
fi

S3=$(seed "$VEST")
step "3 antumbra-vesting"
reg "$S3" 3 "$VEST" "antumbra-vesting" "0.1.0" "Vesting schedules on LEZ." 1 || true
if [ -n "$A_COMMIT" ]; then
  run claim-source --entry-seed "$S3" \
      --repository "https://github.com/edenbd1/antumbra-lez" \
      --commit "$A_COMMIT" --source-digest "$A_DIGEST" --image-digest "$BUILDER_DIGEST" \
      --toolchain "1.88.0" --network-disabled 1 --author "$AUTHOR" \
    && ok "CLAIMED (nobody has rebuilt it)" || ok "FAILED"
else
  ok "registered"
fi

S4=$(seed "$CLOCKP")
step "4 lez-clock"
reg "$S4" 4 "$CLOCKP" "lez-clock" "0.2.4" \
    "The LEZ clock program this registry reads its timestamps from." 1 \
  && ok "UNCLAIMED" || ok "FAILED"

S5=$(tp_seed "$REGISTRY" "$AUTHOR_HEX")
step "5 third-party, stale claim"
reg "$S5" 5 "$REGISTRY" "registry (stale source claim)" "0.7.0" \
    "A third-party entry asserting an older commit as this program's source." 0 || true
run claim-source --entry-seed "$S5" \
    --repository "https://github.com/edenbd1/lp-0023-program-registry" \
    --commit "$STALE_COMMIT" --source-digest "$STALE_DIGEST" \
    --image-digest "$BUILDER_DIGEST" --toolchain "1.88.0" --network-disabled 1 \
    --author "$AUTHOR" \
  && run record-rebuild --entry-seed "$S5" --rebuilt "$STALE_BUILDS" \
       --clock "$CLOCK" --attester "$AUTHOR" \
  && ok "MISMATCHED" || ok "FAILED"

cat <<'EOF'

Six entries, five verification states, every one of them true. Read them back:

  ./indexer/target/release/lez-registry-indexer $REGISTRY
  curl -s localhost:8787/search?q= | python3 -m json.tool
EOF
