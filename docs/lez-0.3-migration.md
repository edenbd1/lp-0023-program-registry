# Moving the registry to LEZ 0.3

The prize targets **Logos testnet 0.3**. The public zone still runs LEZ v0.2.4;
v0.3.0-rc1 was tagged on 2026-09-26. This page records what 0.3 changes for a
program registry, read from the v0.3.0-rc1 source (`f7fda38`), and how this
registry adopts it. Nothing here is deployed yet.

## What 0.3 changes

**A program is an account, not a hash.** In v0.2.4 a program *was* its
ImageID: deploying bytecode produced an identity no one owned. In 0.3 a program
lives at a **header account** chosen by the deployer
(`lee/state_machine/core/src/program/mod.rs`, `ProgramHeader`), which points at a
linked chain of bytecode segments and records the `image_id` the runtime
recomputes from that chain — never a value the caller supplies. Calls address
programs by that account id.

**The header account must sign.** `program_loader`'s `CreateHeader` and
`UpdateHeader` require the header target to be `is_authorized`
(`lez/programs/program_loader/core/src/lib.rs`), so a header cannot be squatted
at an address someone else controls. The official wallet never generates the
header key on its own: the deployer creates it with `wallet account new public`
and keeps it (`lez/wallet/src/cli/programs/program_loader.rs`).

**Headers can change, unless frozen.** `UpdateHeader` repoints a mutable header
at a new segment chain, so the bytecode behind a program id can change. A header
created or updated with `immutable: true` can never be updated again.

**The program model is new.** Guests no longer return pre/post account states;
they return a `PlanOutput` of `ShardEffect`s over per-program account shards,
applied afterwards. SPEL — which the prize requires — does not target this model
yet: its latest release (v0.7.0, 2026-09-17) pins LEZ v0.2.4.

## What that means for the registry

**The deployer entry becomes provable on chain.** In v0.2.4 there was no deploy
authority, so the canonical slot went to whoever could reproduce the deployed
bytecode (`claim_canonical`). In 0.3 the deployer holds the header key, so the
deployer entry is simply **the entry signed by the program's own header
account**: the registry refuses a deployer entry whose signer is not the program
id it names. A third-party entry stays keyed by its publisher, as today.

**Verification compares against the header.** A rebuild is matched against the
`image_id` stored in the program's header account, read from chain, rather than
against a deploy transaction hash. The entry records the header's `immutable`
flag, and a client shows it next to the verdict: *verified and frozen* is a
stronger statement than *verified as of block N*, because a mutable header can
be repointed after verification.

**Staleness gets a trigger.** Because an update rewrites the header, a client
re-checks a verified entry when the header's `image_id` differs from the one the
verification recorded, instead of relying on elapsed time.

## What waits, and what does not

Waits for SPEL on 0.3: the program itself, its IDL, the executor tests and the
deployment. Porting SPEL ahead of the upstream release would produce a framework
the prize does not recognise.

Does not wait: the explorer, the indexer and the SDK's resolution and display
logic, which only need the program id to become an account id and the verdict to
carry the `immutable` flag; the adoption groundwork; and the criteria tracker.
