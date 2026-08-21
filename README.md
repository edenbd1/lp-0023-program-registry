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

### Two digests, and the registry says which is which

A program on LEZ has two 32-byte numbers attached to it, and they are not the
same number:

| | what it is | how you get it |
|---|---|---|
| **ProgramId** | the RISC0 ImageID, derived from the guest ELF's own structure | `spel program-id`, or risc0's `binfmt` |
| **deployment digest** | `SHA256(u32_le(len) ‖ bytes)` over the packaged `.bin` | `lez-registry deployment-digest` |

For this registry's own program those are
`f6a765c2a316da6344de3cccfb39868732a4102f93ff9f0cc8d55be2d7c9e577` and
`447263aa8e01b2d3b5cbb4c603b61194d5909b0158f3901e88d282202d9cb604`.

The second one is also the hash of the transaction that deployed it — which is
the useful thing about it, and the trap. It is tempting to read
`SHA256(len ‖ bytes)` as "LEZ's content addressing" and key a registry on it, so
that an entry's hash and the program's address are one number instead of two
that usually agree. That is wrong, and it was wrong here first: the CLI printed
the deploy transaction hash under the heading `program-id` until it was run
against a real deployment and disagreed with `spel`. A registry keyed that way
resolves nothing, because nothing ever calls a program by that number.

So the crate names them apart, and never computes the ImageID itself — that
definition lives in risc0, and a second implementation of it would be a second
thing that can drift. What the registry stores is the ProgramId as the key and
the deployment digest as the artifact, and `verify` reports which of the two it
checked.

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

**There is deliberately no `Stale`.** Registries for account-addressed chains
need one: code at an address can be replaced under a verification that was
honestly earned, so the badge has to decay, and the decay has to be distinguished
from failure — "failed" reads as *someone tried to cheat*, while an upgraded
program has simply moved on.

None of that applies here, and it took removing a working implementation of it to
see why. An entry is keyed by its ImageID, so the bytecode *is* the key. Nothing
can be swapped beneath an entry. The only way to have different bytecode is to be
a different entry, which starts `Unclaimed` like every other.

So verification is still version-bound — an author cannot verify something benign,
upgrade to anything, and keep the badge — but the binding lives in the addressing
rather than in a state transition, and costs no state machine at all. What is left
is four states a UI can actually render, none of which is unreachable.

`record_rebuild` takes the entry's own key as its subject and has no third
"currently deployed" argument, because such an argument could only ever hold one
value, and a parameter with one legal value is somewhere for a caller to be wrong.

### The badge carries a name, because the chain cannot compile

A registry cannot verify a build. It has no compiler, and a rebuild is a claim
made *about* the world rather than a computation the runtime can redo. Every
registry that prints "Verified" is therefore relaying somebody's word, and the
only real choice is whether it tells you whose.

`record_rebuild` is callable by anyone and records the signer as the entry's
`attester`. Restricting it to the author would make `Verified` mean "the author
says so", which is the thing the state exists to improve on; opening it without
recording who called would make the badge anonymous, which is worse. So the badge
always comes with a name, and `is_self_attested` answers the question a reader
actually has — is the only party vouching for this build the party that shipped
it?

That is not an accusation. An author who rebuilds their own release reproducibly
has done something useful; it is simply weaker evidence than an independent
rebuild, and rendering both as one tick would hide the difference that matters
most. A failed attempt is recorded too: a rebuild claiming a networked build
lands on `Unreproducible` *and* keeps the attester, so a stalled claim and a
tried-and-failed one are distinguishable.

What the chain does enforce is the one condition that makes a matching rebuild
mean anything: the claimed environment must have had the network disabled. That
check is cheap, and it is the check the Solana incident turned on.

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
