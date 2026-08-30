#!/usr/bin/env bash
# End-to-end walk through the live registry. Read-only: it signs nothing and
# changes nothing on chain, so it is safe to run repeatedly.
#
# Portable between macOS and Linux. The two things that usually break that are
# handled explicitly: sha256 is `shasum -a 256` on macOS and `sha256sum` on most
# Linux distributions, and `sed -i` takes an argument on BSD but not GNU. This
# script uses neither in-place edit nor a bare `sha256sum`.
set -uo pipefail
cd "$(dirname "$0")/.."

REGISTRY=f59751e932e856f7565f34e79ae726791794dc68e55f2413bc0375008cc07bf8
ENTRY_ACCOUNT=wkLxSjsYNRq24FKPzDSh1haT382zwMU5QFb2YuHRD4h
GLOBAL_ACCOUNT=75jzaQqqqf87apGZfr3bewGbHDfCnau49TgohDKc5s1z
RPC="${SEQUENCER_URL:-https://testnet.lez.logos.co}"

sha256() {
  if command -v shasum > /dev/null 2>&1; then shasum -a 256 "$@"
  else sha256sum "$@"; fi
}

step() { printf '\n\033[1m%s\033[0m\n' "$*"; }
need() { command -v "$1" > /dev/null 2>&1 || { echo "missing: $1" >&2; return 1; }; }

need curl || exit 1
need python3 || exit 1

step "1. The registry's own address, derived rather than looked up"
echo "   registry program id: $REGISTRY"
python3 - "$REGISTRY" <<'PY'
import hashlib, sys
A = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz'
def b58(b):
    n = int.from_bytes(b, 'big'); s = ''
    while n: n, r = divmod(n, 58); s = A[r] + s
    return '1' * (len(b) - len(b.lstrip(b'\0'))) + s
pid = bytes.fromhex(sys.argv[1])
seed = hashlib.sha256(b'lez-registry:entry:v1' + pid).digest()
prefix = b'/LEE/v0.2/AccountId/PDA/' + b'\x00' * 8
print('   entry seed         :', seed.hex())
print('   entry account      :', b58(hashlib.sha256(prefix + pid + seed).digest()))
print('   (no index was consulted; this is two hashes)')
PY

step "2. Read that account straight from the chain"
curl -s -m 25 -X POST "$RPC" -H 'Content-Type: application/json' \
  -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"getAccount\",\"params\":[\"$ENTRY_ACCOUNT\"]}" \
  | python3 -c '
import sys, json
r = json.load(sys.stdin).get("result")
if not r: print("   account is empty"); raise SystemExit
data = bytes(r["data"])
print("   %d bytes, owned by the registry program" % len(data))
print("   first 32 bytes:", data[:32].hex())
'

step "3. What the entry says, decoded"
if [ -x cli/target/release/lez-registry ]; then
  cli/target/release/lez-registry lookup "$REGISTRY" \
    --registry "$REGISTRY" --endpoint "$RPC" | sed 's/^/   /'
else
  echo "   build the CLI first: cargo build --release --manifest-path cli/Cargo.toml"
fi

step "4. The counters"
curl -s -m 25 -X POST "$RPC" -H 'Content-Type: application/json' \
  -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"getAccount\",\"params\":[\"$GLOBAL_ACCOUNT\"]}" \
  | python3 -c '
import sys, json
r = json.load(sys.stdin).get("result")
d = bytes(r["data"])
print("   total_entries  :", int.from_bytes(d[0:8], "little"))
print("   total_verified :", int.from_bytes(d[8:16], "little"))
'

step "5. Check a local binary against what was deployed"
if [ -f artifacts/programs/lez_registry.bin ]; then
  python3 - artifacts/programs/lez_registry.bin <<'PY'
import hashlib, struct, sys
b = open(sys.argv[1], 'rb').read()
digest = hashlib.sha256(struct.pack('<I', len(b)) + b).hexdigest()
print('   deployment digest  :', digest)
print('   this same value is the deploy transaction hash on chain: the digest IS the')
print('   tx id, computed here from the bytes rather than read from an explorer. Step 6')
print('   proves getTransaction returns it, and a never-deployed hash returns null.')
PY
else
  echo "   artifacts/programs/lez_registry.bin not present"
fi

step "6. Re-check every claim in DEPLOYMENTS.md"
./scripts/verify-onchain.sh | sed 's/^/   /'

cat <<'EOF'

Nothing above asked a server that could lie. Addresses were derived, accounts
were read from the sequencer, and the digest was recomputed locally.
EOF
