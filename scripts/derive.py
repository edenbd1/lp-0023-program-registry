#!/usr/bin/env python3
"""Derive a registry account address from a program id.

Kept as its own file rather than inlined in the shell script: the shell needed
nested heredocs to do it, and a quoting mistake there produces a well-formed
address for the wrong account, which is the failure mode this whole check exists
to avoid.

    derive.py <program-id-hex> global|entry
"""
import hashlib
import sys

ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
PDA_PREFIX = b"/LEE/v0.2/AccountId/PDA/" + b"\0" * 8
GLOBAL_SEED = b"lez-registry:global:v1"
ENTRY_SEED = b"lez-registry:entry:v1"


def b58(raw: bytes) -> str:
    n = int.from_bytes(raw, "big")
    out = ""
    while n:
        n, r = divmod(n, 58)
        out = ALPHABET[r] + out
    # Leading zero bytes are significant and encode as '1'.
    return "1" * (len(raw) - len(raw.lstrip(b"\0"))) + out


def main() -> int:
    if len(sys.argv) != 3:
        print(__doc__.strip(), file=sys.stderr)
        return 2
    pid = bytes.fromhex(sys.argv[1])
    which = sys.argv[2]
    if which == "global":
        # Right-padded, not hashed: the program declares this account with a
        # literal seed and SPEL pads literals to 32 bytes.
        seed = GLOBAL_SEED + b"\0" * (32 - len(GLOBAL_SEED))
    elif which == "entry":
        seed = hashlib.sha256(ENTRY_SEED + pid).digest()
    else:
        print("expected 'global' or 'entry'", file=sys.stderr)
        return 2
    print(b58(hashlib.sha256(PDA_PREFIX + pid + seed).digest()))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
