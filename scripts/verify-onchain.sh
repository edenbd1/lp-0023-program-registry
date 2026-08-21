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
check deploy      d5ce586aa075ae0b7f87c6a0fc3db08b9bd500181b259a0eb4e3b765d7827b96 yes
check initialize  e3eaedd021fa0f49a58867c69cd15fe455dc3c7b5d02ffe4d7c9f2c6befd45ee yes
check register    5dcae30b296b88232a83973d62a329f8a17784094dd221484c129fa4608d37db yes
check prior-build 447263aa8e01b2d3b5cbb4c603b61194d5909b0158f3901e88d282202d9cb604 yes

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
  if [ "$got" = d5ce586aa075ae0b7f87c6a0fc3db08b9bd500181b259a0eb4e3b765d7827b96 ]; then
    printf '  OK   %-22s recomputes to the deploy transaction hash\n' "digest"
  else
    printf '  FAIL %-22s got %s\n' "digest" "$got"
    fail=1
  fi
else
  printf '  SKIP %-22s %s not present\n' "digest" "$BIN"
fi

# Controls. Neither hash was ever deployed, so both must return null; if the
# endpoint answers "yes" to these it is answering "yes" to anything.
check control-zeros 0000000000000000000000000000000000000000000000000000000000000000 no
check control-ffs   ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff no

[ "$fail" = 0 ] && echo "all checks passed" || echo "FAILURES above"
exit $fail
