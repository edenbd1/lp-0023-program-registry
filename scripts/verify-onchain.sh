#!/usr/bin/env bash
# Re-check every claim DEPLOYMENTS.md makes, against the public LEZ testnet.
#
# The last two checks are the point of the script: a getTransaction that returns
# data proves nothing until a hash that was never deployed is shown to return
# null. Without that control this only proves the endpoint answers.
#
# Needs curl and python3. Exits non-zero if any expected transaction is missing,
# if a control unexpectedly resolves, or if the locally computed deployment
# digest disagrees with the deploy transaction hash.
set -uo pipefail
RPC="${SEQUENCER_URL:-https://testnet.lez.logos.co}"
fail=0

fetch() {
  curl -s -m 25 -X POST "$RPC" -H 'Content-Type: application/json' \
    -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"getTransaction\",\"params\":[\"$1\"]}"
}

check() { # label tx expect_present
  local body present
  body=$(fetch "$2")
  present=$(printf '%s' "$body" | python3 -c 'import sys,json;print("yes" if json.load(sys.stdin).get("result") else "no")' 2>/dev/null)
  if [ "$present" = "$3" ]; then
    if [ "$3" = yes ]; then
      printf '  OK   %-22s present\n' "$1"
    else
      printf '  OK   %-22s returns null, as a never-deployed hash must\n' "$1"
    fi
  else
    printf '  FAIL %-22s expected present=%s, got %s\n' "$1" "$3" "$present"
    fail=1
  fi
}

echo "registry on $RPC"
check deploy         2ff83da81c8c5dfed840c37c95f3ada8b38e281243ffb533a2b74e2bbe7ebb94 yes
check build-17553    10d172fdb23c03fc475583c4bb3c286019f2be43b080900c5ff80cd1607baa26 yes
check build-17474    c1223c7240dc6d7a6d51b80ae3ab96d281e10c1556a54829e05a0079adac2a9d yes
check build-17531    d986fefad93efe52229fc9955de231fe980864ba1749a826209b082e26690e13 yes
check claim-source   222a7c1baa0e5fd56f37c1e5be3e414b14373a42073d49e20ac53c91161fcbc0 yes
check record-rebuild 5823f762239e7a6717ade9e8549ac0aa7b2a66e8a0b463de001bf7c7f3c966e4 yes
check refusal-17428  a4d2b10132dafd789615b24dc7be9ac21e7b2cccc53b78f326d6a667219ae1ca yes
check build-17428    0fe6f57a0dee2fb41adced5ba59c420bbc22aa425a6714f806aad4d087a92899 yes
# Every earlier build is a different program, not a stale link, so each stays
# checkable. A row that stopped resolving would mean the chain lost it.
check build-17342    447263aa8e01b2d3b5cbb4c603b61194d5909b0158f3901e88d282202d9cb604 yes
check build-17358    d5ce586aa075ae0b7f87c6a0fc3db08b9bd500181b259a0eb4e3b765d7827b96 yes
check build-17398    7b1bc3ca7748c0069ebbb90b52863cc9fa219a1155295f5ecf5046df97f9bf06 yes
check build-17414    e4ef3af04bea59c969019fa2831278983709bd3d33a5163004a4fdd950d44f81 yes

# The deployment digest is not merely *like* the deploy transaction hash; it is
# that hash. Recompute it from the binary in the tree and require equality, so a
# swapped artifact fails here rather than being noticed by nobody.
BIN=artifacts/programs/lez_registry.bin
if [ -f "$BIN" ]; then
  got=$(python3 - "$BIN" <<'PY'
import hashlib,struct,sys
b=open(sys.argv[1],'rb').read()
print(hashlib.sha256(struct.pack('<I',len(b))+b).hexdigest())
PY
)
  if [ "$got" = 2ff83da81c8c5dfed840c37c95f3ada8b38e281243ffb533a2b74e2bbe7ebb94 ]; then
    printf '  OK   %-22s recomputes to the deploy transaction hash\n' "digest"
  else
    printf '  FAIL %-22s got %s\n' "digest" "$got"
    fail=1
  fi
else
  printf '  SKIP %-22s %s not present\n' "digest" "$BIN"
fi

# Account addresses, checked by reading them rather than by trusting this file.
# An address quoted in documentation and never fetched is an address that can be
# wrong for as long as nobody looks -- which is how a fabricated global-state
# address survived one commit here.
account() { # label base58 min_bytes
  local n
  n=$(curl -s -m 25 -X POST "$RPC" -H 'Content-Type: application/json' \
        -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"getAccount\",\"params\":[\"$2\"]}" \
      | python3 -c 'import sys,json;r=json.load(sys.stdin).get("result");print(len(r["data"]) if r and r.get("data") else 0)' 2>/dev/null)
  if [ "${n:-0}" -ge "$3" ]; then
    printf '  OK   %-22s %s bytes at %s\n' "$1" "$n" "${2:0:12}..."
  else
    printf '  FAIL %-22s %s bytes at %s (expected >= %s)\n' "$1" "${n:-0}" "$2" "$3"
    fail=1
  fi
}
account global-state    42MaszdnoXcWkDZBgNJZVyP26TtD47sBxNLkXnmmr5Dj 16
account canonical-entry 2YpaP2aWgAqx9uVHQ4yoAsp7jtt672SR4qyTJrGdq3gR 100

# Controls. Neither hash was ever deployed, so both must return null; if the
# endpoint answers "yes" to these it is answering "yes" to anything.
check control-zeros 0000000000000000000000000000000000000000000000000000000000000000 no
check control-ffs   ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff no

[ "$fail" = 0 ] && echo "all checks passed" || echo "FAILURES above"
exit $fail
