# Positions taken, and why

The prize leaves four questions open and says the reasoning is part of what is
evaluated. Two more had to be answered before any of those made sense, because
LEZ does not have the thing the questions assume.

## 0. There is no deploy authority on LEZ, so "canonical" cannot mean what it means elsewhere

The criterion asks for canonical entries "creatable only by the upgrade
authority", following Solana's `program-metadata`. That construction has no
counterpart here, and pretending otherwise would be the worst possible answer.

LEZ deployment is **content-addressed and permissionless**. A program's id is the
RISC0 ImageID of its bytecode. Anyone may deploy any bytes; deploying the same
bytes twice yields the same id; there is no upgrade authority because there is no
upgrade — different bytecode is a different program at a different address. Nobody
owns a program id, and no on-chain fact says who first published one.

So the honest options were:

- **First-come canonical.** The prize names this as broken, and it is: an
  attacker registers a popular program id under a plausible name before its
  author does, and permanently owns the entry wallets will display.
- **Bind canonical to the deploy transaction's signer.** Attractive, but a guest
  cannot read historical transactions; there is no on-chain record it can reach
  that links a program id to whoever submitted its deployment.
- **Bind canonical to who can reproduce the bytecode from source.** This is the
  position taken.

On a content-addressed chain, the party who can produce the bytecode from source
in a sealed build is the closest thing to an authority that exists — and it is a
stronger claim than a deploy signature, which only proves someone pressed the
button. Front-running gains nothing durable: a squatter can occupy the canonical
address, but cannot make it *verified*, because they cannot produce a matching
offline rebuild without the real source.

**Current state, stated plainly:** the two derivations exist and the program
re-derives the seed it is given, so an entry cannot be placed at an address the
derivation does not produce. The authority binding above is designed and **not
yet implemented** — today a canonical entry is first-come, and its trust comes
from the verification state next to it rather than from who holds it. Until the
binding lands, a canonical entry that is not `Verified` should be read as "someone
registered this", nothing more, and the UI says so. See
[criteria-evidence.md](criteria-evidence.md) #3.

## 0b. Staleness, and why there is no `Stale` state

The criterion asks that verification be version-bound, that a prior verified
status be invalidated rather than carried over, and that the staleness window be
documented.

**The staleness window for an upgrade is zero, structurally.** An entry is keyed
by the ImageID, so the bytecode *is* the key. There is no operation that changes
the bytecode under an entry; different bytecode is a different key, hence a
different entry, which starts `Unclaimed` like every other. A verified entry can
never be showing a verdict about code that is no longer deployed, because the code
under it cannot change.

**The staleness window for a claim change is also zero**, and that one needed
code: `claim_source` replaces the whole `Verification`, so re-pointing an entry at
different source resets it to `Claimed`. The author cannot verify something benign
and then quietly re-point the claim.

What *can* age is the world outside the chain — a repository can be deleted, a
commit force-pushed. That is why every verdict carries `last_checked` (block and
millisecond, from the chain's clock) and why `source_digest` is stored: a digest
over the source tree survives its host, so any mirror reproducing it is as good as
the original.

## 1. What lives on chain?

**The entry, its metadata and its verification state — on chain. The IDL — on
Logos Storage, by content identifier.**

The entry is small, bounded, and read on every lookup; putting it on chain keeps
resolution to one account read with no second system in the path. An IDL is
unbounded and read rarely, and [CYCLES.md](../CYCLES.md) makes the cost concrete:
`deployment_digest` runs at 62.6 cycles/byte, so hashing this registry's own
443 KB artifact on chain would consume **91% of LEZ's entire 32M compute budget**
before any logic ran. Large blobs do not belong in a program's account, and the
number rather than the intuition is what decided it.

## 2. Where does verified source live?

**A repository URL, plus a `source_digest` that outlives it.**

Mirroring every source tree to Logos Storage is the more self-contained answer and
it was not chosen, because it moves the cost to whoever pays for storage and makes
registration expensive enough to suppress the adoption the prize is weighted
toward.

The liveness dependency is real and is mitigated rather than denied: the claim
stores a digest over the source tree *as built*, so a deleted repository does not
make re-verification impossible — it makes it require a mirror, and any mirror
reproducing the digest is as good as the original host. What is lost with the
repository is discovery, not verifiability.

## 3. Is there a trusted-signer overlay?

**No, and deliberately not.**

Every verdict already carries its attester, and the CLI, SDK and Basecamp panel
all surface it. A hardcoded list of blessed verifiers would replace "decide whose
word this is" with "trust whoever we listed", which is the centralised trust root
the rest of this design spends its effort avoiding. `is_self_attested` gives the
one distinction that needs no list: whether the only party vouching for a build is
the party that shipped it.

A consumer who wants a trusted set can keep one — the attester is a plain account
id in the entry, so filtering is a client-side predicate over data the chain
already gives them. That belongs to the consumer, not to the registry.

## 4. Can entries be frozen or closed?

**Frozen, yes. Closed, no.**

`freeze` is one-way and there is no instruction that lowers the flag, which is the
only thing that makes immutability worth anything. A frozen entry is a promise
that the metadata a consumer pinned against will not move under them.

Deletion is deliberately absent. An entry that can disappear is an entry whose
absence carries no information, and "this program was registered and the record is
gone" is precisely the state an attacker would want to be able to create. The
registry is append-and-amend: a wrong entry is corrected in place by its author,
or contradicted by a third-party entry that anyone can read beside it.

That is also why `Mismatched` is stored rather than returned as an error. A
published source that does not rebuild is evidence, and evidence that vanishes on
failure is not evidence at all.
