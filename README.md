# LEZ Program Registry

A program ID on the Logos Execution Zone is an opaque 32 bytes. Nothing in it
says what the program does, who wrote it, what its interface looks like, or
whether the source you are reading is the source that is running.

This is a registry that answers those questions on chain, and a verification
tool that answers the last one *honestly*.

> **Status: early.** The core types and verification logic are here and tested.
> The SPEL program, CLI, SDK and Basecamp app follow.

## What "verified" means here, and what it does not

Verification proves exactly one thing: **rebuilding the stated source, at the
stated commit, with the stated toolchain, produces the bytecode that is
deployed.** It does not prove the program is safe, that the author is who they
say they are, or that the code does what its README claims. Anywhere this
registry shows verification status it will say so, because a green tick teaches
users the opposite.

### A hash match is not provenance

Solana's verified builds were subverted by a repository whose `build.rs`
downloaded the already-deployed binary at compile time and emitted it. The
rebuild matched perfectly. The source proved nothing.

Any registry that only compares hashes inherits that hole, so this one does not
only compare hashes. A build records whether it ran **with the network
disabled**, and a claim rebuilt with network access **cannot reach `Verified`** —
it stops at `Unreproducible`, which is a different word on screen and a
different state in the type system.

### Four states, and none of them is a tick

| state | meaning |
|---|---|
| `Unclaimed` | no source asserted — the honest default |
| `Claimed` | a source is asserted and nobody has rebuilt it |
| `Verified` | rebuilt offline, and the bytecode matched what is deployed |
| `Unreproducible` | rebuilt, but with the network up, so the match proves nothing |
| `Stale { verified_artifact }` | was verified, of a build that is no longer deployed — and it names which |

`Stale` exists because "failed" reads as *someone tried to cheat* while an
upgraded program is simply a program that moved on. Conflating them either
slanders honest authors or excuses dishonest ones.

**Verification is version-bound.** An upgrade cannot inherit the previous
badge — otherwise an author verifies something benign, upgrades to anything, and
keeps it. `on_artifact_changed` is what enforces that, and a test asserts it.

## The design decisions, taken and justified

The prize leaves four questions open. Our positions:

**What lives on chain?** The entry, its metadata and its verification state. The
IDL JSON lives on **Logos Storage**, with only its content identifier on chain,
because an IDL is kilobytes and an on-chain kilobyte is paid for by everyone who
syncs the zone.

**Where does verified source live?** A repository URL is a *liveness dependency
on a third party* — GitHub can disappear and the claim becomes uncheckable. So a
claim also carries `source_digest`, a hash over the source tree as built. Any
mirror that reproduces that digest is as good as the original host, which turns
the URL from a dependency into a hint.

**Is there a trusted-signer overlay?** No. The registry records whether an entry
came from the program's own deploy authority (`Canonical`) or from anyone else
(`ThirdParty`) and **refuses to rank them**. Deciding which strangers to trust is
curation, and curation is explicitly out of scope. Recording who said it is not.

**Can entries be frozen?** Yes, one-way. `freeze` sets a flag no call in this
crate lowers, and frozen beats authorship — after freezing, even the author is
refused. An entry that can always change and an entry that can never change have
different trust properties, and an author should be able to choose the second.

## Addresses are derivable without an index

An entry's address is a function of the program ID alone, so a client resolving
a program computes the address it needs rather than asking a service. An index
required for lookup would be an external dependency, and *no mandatory external
services* is a requirement rather than a preference.

Third-party entries are keyed by their publisher, so **a stranger can never
occupy the canonical address** — by construction, not by a check that could be
forgotten. Seeds are domain-separated so one kind of address cannot collide with
another.

## Layout

- `src/lib.rs` — entry types, address derivation, the verification state
  machine. `no_std`, no I/O, so the on-chain program and the CLI link the *same*
  rules rather than two implementations that agree until they do not.

## Licence

MIT OR Apache-2.0.
