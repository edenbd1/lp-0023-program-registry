# Deployments

Public LEZ testnet, `https://testnet.lez.logos.co`.

## The live registry

Built **sealed** — every build container had its network disabled.

| | |
|---|---|
| ProgramId (RISC0 ImageID) | `f59751e932e856f7565f34e79ae726791794dc68e55f2413bc0375008cc07bf8` |
| deployment digest | `2ff83da81c8c5dfed840c37c95f3ada8b38e281243ffb533a2b74e2bbe7ebb94` |
| deploy transaction | [`2ff83da8…be7ebb94`](https://explorer.testnet.lez.logos.co/transaction/2ff83da81c8c5dfed840c37c95f3ada8b38e281243ffb533a2b74e2bbe7ebb94) · block 17589 |
| global state | `42MaszdnoXcWkDZBgNJZVyP26TtD47sBxNLkXnmmr5Dj` |
| canonical entry (itself) | `2YpaP2aWgAqx9uVHQ4yoAsp7jtt672SR4qyTJrGdq3gR` |

### All five verification states, live, and each one true

Reproduce the whole set with
`REGISTRY=f59751e932e856f7565f34e79ae726791794dc68e55f2413bc0375008cc07bf8 ./scripts/populate-demo.sh`.

| slot | name | state | why it is that state |
|---|---|---|---|
| 0 | `lez-program-registry` | **verified**, self-attested | sealed rebuild reproduced the deployed ImageID |
| 1 | `antumbra-curve` | **unclaimed** | nobody has said where its source is |
| 2 | `antumbra-lbp` | **unreproducible** | the claim honestly records a build that had network access |
| 3 | `antumbra-vesting` | **claimed** | a source is asserted and nobody has rebuilt it |
| 4 | `lez-clock` | **unclaimed** | the LEZ clock program this registry reads its timestamps from |
| 5 | `registry (stale source claim)` | **mismatched** | a third-party entry naming commit `8dfa2f01`, which really builds `e1484f14…` and not this program |

Nothing there is staged. The mismatched entry points at a real earlier commit of
this repository that genuinely produces a different ImageID; the unreproducible
one records a build that genuinely had the network up. A demo that faked either
would be demonstrating the opposite of what this registry is for.

Slot 5 is also why enumeration stores the entry seed rather than the program id:
a walker deriving from the program id alone resolves every third-party entry to
the canonical one and drops it from the listing — found by reading the listing
back, not by reading the code.

### Timestamps come from the chain

Entries carry `registered_at` and verdicts carry `last_checked`, both read from
the sequencer's clock accounts rather than supplied by the caller. The Adoption
criteria are counted in registrations per month and checked from chain data, so a
caller-chosen date would be worse than none. See
[docs/criteria-evidence.md](docs/criteria-evidence.md).

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
