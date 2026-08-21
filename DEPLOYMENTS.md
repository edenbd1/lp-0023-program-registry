# Deployments

Public LEZ testnet, `https://testnet.lez.logos.co`.

## The live registry

| | |
|---|---|
| ProgramId (RISC0 ImageID) | `145c111d0e8195f83483c10130221397fc11ef629efc89dbb414bcc9ee3cc8fd` |
| deployment digest | `0fe6f57a0dee2fb41adced5ba59c420bbc22aa425a6714f806aad4d087a92899` |
| deploy transaction | [`0fe6f57a…d087a92899`](https://explorer.testnet.lez.logos.co/transaction/0fe6f57a0dee2fb41adced5ba59c420bbc22aa425a6714f806aad4d087a92899) · block 17428 |
| `record_rebuild` | [`a4d2b101…219ae1ca`](https://explorer.testnet.lez.logos.co/transaction/a4d2b10132dafd789615b24dc7be9ac21e7b2cccc53b78f326d6a667219ae1ca) |
| global state | `CLAEqmgWsrYPPwja3Bwvc5pgN64yBKtwKv8b2KGQRgoF` |
| canonical entry | `84NG6dyqNiARW9K1LvK1cE9CZWYNWj9H179yWLNihB1q` |

`initialize`, `register` and `claim_source` also ran. Their transaction hashes are
not the evidence and are not quoted: **the account contents are**. A hash proves a
transaction was included, not that it did what you think — the two are different
claims, and this project has been bitten by the difference twice. Read the state.

## The refusal, on chain

This is the thesis of the registry, executed rather than asserted:

```
$ lez-registry explain-entry "$(spel inspect 84NG6dyqNiARW9K1LvK1cE9CZWYNWj9H179yWLNihB1q --type EntryAccount ...)"
lez-program-registry 0.4.0
  program id   145c111d0e8195f83483c10130221397fc11ef629efc89dbb414bcc9ee3cc8fd
  author       bfd087578945b8a9434004c679e6225f1934f5ebf27b96fbec5a6ef2167b9e3c
  publisher    canonical
  verification unreproducible
               A rebuild was recorded but the claimed build had network
               access, so a match would prove nothing about the source.
               attempted by bfd087578945b8a9434004c679e6225f1934f5ebf27b96fbec5a6ef2167b9e3c
  repository   https://github.com/edenbd1/lez-program-registry
  toolchain    1.88.0
  offline      false
```

**The rebuild matched.** `record_rebuild` was called with exactly the deployed
ImageID; the bytecode reproduced to the byte. The registry still refuses to call
it verified, because the claimed build had network access, and a build that could
have downloaded the artifact it was meant to reproduce proves nothing about the
source. `total_verified` reads **0**, next to `total_entries` **1**.

That zero is the deliverable. Any registry can print a tick.

## Why nothing here is Verified yet

`cargo risczero build` offers no way to disable the build container's network —
checked against its `--help` and against its binary, neither of which carries a
`--network` option. So no rebuild we can currently perform is provably offline,
so `claim_source` is recorded honestly with `network_disabled=0`, and the program
does the rest.

`scripts/verify-source.sh` reports exactly this rather than guessing. What the
build does establish: it is **deterministic** — two independent runs over
identical source produced the same ImageID — and it runs inside one pinned
builder image, `risczero/risc0-guest-builder:r0.1.88.0`,
`sha256:3e12f71bacd27527a61dea96fa0e53e468c99aa261d3a1019b593f6dbd943eb3`.

What closes the gap: `cargo risczero build --network=none`, or a guest Dockerfile
using `RUN --network=none` over vendored dependencies.

## The digest predicted the transaction hash, four times

For every program deployed here, `SHA256(u32_le(len) ‖ bytecode)` was computed
**before** the deploy was sent, and the chain returned that same value as the
transaction hash:

| ImageID | digest = deploy tx | block |
|---|---|---|
| `447263aa…` *(pre-correction build)* | `447263aa8e01b2d3b5cbb4c603b61194d5909b0158f3901e88d282202d9cb604` | 17342 |
| `fabf1f34…` | `d5ce586aa075ae0b7f87c6a0fc3db08b9bd500181b259a0eb4e3b765d7827b96` | 17358 |
| `aa13f602…` | `7b1bc3ca7748c0069ebbb90b52863cc9fa219a1155295f5ecf5046df97f9bf06` | 17398 |
| `c5073817…` | `e4ef3af04bea59c969019fa2831278983709bd3d33a5163004a4fdd950d44f81` | 17414 |
| `145c111d…` | `0fe6f57a0dee2fb41adced5ba59c420bbc22aa425a6714f806aad4d087a92899` | 17428 |

Predicted, then confirmed — not noticed afterwards. This is what
`lez-registry deployment-digest` checks, and it is why the tool can tell you the
file you hold is the file that was deployed without an explorer vouching for it.

The ImageID is a different number in every row. Both are printed because a
registry that blurred them would resolve nothing.

## Every earlier build is still on chain

None of the rows above supersedes another. Each is a distinct program at a
distinct address with its own global account and its own entries, which is the
version-binding argument made concrete: changing one field of one struct changed
the ImageID, so it changed the address, so the new build inherited nothing.

## Reproduce

```bash
cargo risczero build --manifest-path program/Cargo.toml
spel program-id artifacts/programs/lez_registry.bin        # the ImageID
cargo run --manifest-path cli/Cargo.toml --release -- \
  deployment-digest artifacts/programs/lez_registry.bin    # the deploy tx hash
./scripts/verify-source.sh --rebuild                       # both, plus the claim fields
./scripts/verify-onchain.sh                                # re-check this file
```

Run from the repository root: the Docker build context is taken from there, and
the guest depends on `lez-registry-core` by path. Needs Docker (the guest builder
image is `linux/amd64` and runs under emulation on Apple silicon),
`cargo risczero` 3.0.5, and `spel`.
