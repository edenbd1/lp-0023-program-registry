# Positions taken, and why

The prize leaves three questions open and says the reasoning is part of what is
evaluated. Two more had to be answered first, about what LEZ v0.3 makes
provable.

## 0. The deployer is the header key

On LEZ v0.3 a program lives at a header account its deployer chose, and the
loader refuses to create or update that header unless the header account signs
(`program_loader_core::create_header`, `update_header`). So the header key is
the program's deploy authority, the counterpart of Solana's upgrade authority.

The registry binds the deployer entry to it directly: `register` without a
publisher requires the author to be the program's own header account, signing,
and the entry's address is a function of the program id alone, so there is
exactly one deployer entry and nobody else can occupy it. Everyone else
publishes a third-party entry, keyed by their own account, beside it. Clients
tell the two apart from the entry's `publisher` field, which only the program
could have caused to read `Canonical`.

What this does not say: that the key holder wrote the code, or that the code is
benign. It says the entry was published by whoever controls the program, which
is the claim a wallet needs before showing a name next to a signature request.

The v0.2.4 design had no deploy authority to bind to, and made the canonical
slot takeable by whoever could rebuild the deployed bytecode
(`claim_canonical`). v0.3 makes that unnecessary, and it is gone.

## 0b. Staleness is real on v0.3, and has a trigger

A v0.3 header can be repointed at new bytecode unless it is immutable. So a
verdict is about a specific ImageID, and the entry records that ImageID and the
header's `immutable` flag, both checked on chain against the loader's shard
whenever they are written.

When the header moves, `sync_header` (callable by anyone) records the new
ImageID and turns a verdict about the old one into `Stale`. The claim survives,
so a rebuild of the new bytecode can be recorded without the author re-claiming.
Clients do not need to wait for anyone to call it: comparing the entry's
`image_id` with a fresh read of the header (`Client::program_header`) gives the
same answer immediately, and the SDK example does exactly that. A verdict about
an immutable header can never go stale, which is why the flag is shown next to
it.

## 1. What lives on chain?

**The entry, its metadata, the header ImageID it is about, and its verification
state: on chain. The IDL: on Logos Storage, by content identifier.**

The entry is small, bounded, and read on every lookup; putting it on chain keeps
resolution to one account read with no second system in the path. An IDL is
unbounded and read rarely; on chain it would be paid for by every node that
syncs the zone. The bytecode check needs no blob on chain either: the loader
already recorded the ImageID in the header, and the registry compares 32 bytes
against it ([CYCLES.md](../CYCLES.md): a `register` is about 1% of the 10M
execution-gas ceiling).

## 2. Where does verified source live?

**A repository URL, plus a `source_digest` that outlives it.**

Mirroring every source tree to Logos Storage is the more self-contained answer and
it was not chosen, because it moves the cost to whoever pays for storage and makes
registration expensive enough to suppress the adoption the prize is weighted
toward.

The liveness dependency is real and is mitigated rather than denied: the claim
stores a digest over the source tree *as built*, so a deleted repository does not
make re-verification impossible - it makes it require a mirror, and any mirror
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

A consumer who wants a trusted set can keep one - the attester is a plain account
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
