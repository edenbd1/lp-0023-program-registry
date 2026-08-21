# Deployments

Public LEZ testnet, `https://testnet.lez.logos.co`.

## The live registry

Built **sealed** — every build container had its network disabled.

| | |
|---|---|
| ProgramId (RISC0 ImageID) | `e1484f1447edd7669d1443154841160f4ddfe4c5d93d410527e5cf99ab9aacba` |
| deployment digest | `c1223c7240dc6d7a6d51b80ae3ab96d281e10c1556a54829e05a0079adac2a9d` |
| deploy transaction | [`c1223c72…adac2a9d`](https://explorer.testnet.lez.logos.co/transaction/c1223c7240dc6d7a6d51b80ae3ab96d281e10c1556a54829e05a0079adac2a9d) · block 17474 |
| `claim_source` | [`222a7c1b…161fcbc0`](https://explorer.testnet.lez.logos.co/transaction/222a7c1baa0e5fd56f37c1e5be3e414b14373a42073d49e20ac53c91161fcbc0) |
| `record_rebuild` | [`5823f762…f3c966e4`](https://explorer.testnet.lez.logos.co/transaction/5823f762239e7a6717ade9e8549ac0aa7b2a66e8a0b463de001bf7c7f3c966e4) |
| global state | `Bqu9zbwXpKA7QjcpZzr9zxjXNDeYPCFB4v9wLexi5oLu` |
| canonical entry | `7XJmqUmu7YxeDZUKUbmJwRSFdRJPGxg23HHKgd1fCCBK` |

### What it holds

| slot | name | state |
|---|---|---|
| 0 | `lez-program-registry` | **verified**, self-attested |
| 1 | `antumbra-curve` | unclaimed |
| 2 | `antumbra-lbp` | unclaimed |
| 3 | `antumbra-vesting` | unclaimed |

`total_entries` 4, `total_verified` 1. The three Antumbra programs are
registered as the prize requires and left `Unclaimed` rather than dressed up:
their binaries were built with the network available, so there is nothing
truthful to claim about them yet.

`initialize`, `register` and `claim_source` also ran. Their transaction hashes are
not the evidence and are not quoted: **the account contents are**. A hash proves a
transaction was included, not that it did what you think — the two are different
claims, and this project has been bitten by the difference twice. Read the state.

## Verified, and still not a tick

```
$ lez-registry explain-entry "$(spel inspect 7XJmqUmu7YxeDZUKUbmJwRSFdRJPGxg23HHKgd1fCCBK --type EntryAccount ...)"
lez-program-registry 0.5.0
  program id   e1484f1447edd7669d1443154841160f4ddfe4c5d93d410527e5cf99ab9aacba
  author       bfd087578945b8a9434004c679e6225f1934f5ebf27b96fbec5a6ef2167b9e3c
  publisher    canonical
  verification verified — BY ITS OWN AUTHOR
               The only party vouching for this build is the party
               that shipped it. Useful, and not an independent check.
  repository   https://github.com/edenbd1/lez-program-registry
  toolchain    1.88.0
  offline      true
```

`offline true` is the fact that earns the state, and it was earned rather than
asserted: `scripts/build-sealed.sh` disables the network for every build
container, so `claim_source` could record `network_disabled=1` without lying.

The registry's own entry does not get a green check, because the only party
vouching for this build is the party that shipped it. That is worth something and
it is not an independent rebuild, and a UI that rendered both identically would
hide the one difference that matters.

## The same mechanism refusing, at block 17428

An earlier deployment shows the other half, and it is worth keeping:

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

## How the sealed build works, and what it cost to get there

`cargo risczero build` has no `--network` option — checked against its `--help`
and against its binary. But it shells out to `docker`, so `scripts/sealed/docker`
intercepts the build call and adds `--network none`, and that directory goes first
on `PATH` for the build.

The first attempt failed exactly where it should: at
`cargo +risc0 fetch --locked`, the step that reaches the network. Vendoring turns
that fetch into a no-op, which is what makes an offline build possible at all.

Two phases, deliberately separate:

1. **Fetch, with the network.** `cargo vendor` resolves every dependency and
   checks it against hashes already in `Cargo.lock`, so it cannot introduce a
   crate the lockfile did not pin.
2. **Build, with no network at all.** The container cannot reach anything, so a
   `build.rs` cannot download the artifact it is meant to reproduce.

The sealed build is **deterministic** — repeated runs produce the same ImageID —
and runs inside one pinned builder image,
`risczero/risc0-guest-builder:r0.1.88.0`,
`sha256:3e12f71bacd27527a61dea96fa0e53e468c99aa261d3a1019b593f6dbd943eb3`.

It is **not** the same ImageID as a networked build of the same source, because
vendoring changes the paths compiled into the ELF. So the sealed build is the
canonical one: a binary nobody can reproduce offline is not worth deploying.

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
