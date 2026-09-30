# Port plan: Provenance on LEZ v0.3.0

Target: LEZ `v0.3.0` (`db66590ab`, logos-blockchain/logos-execution-zone). The
public zone for testnet v0.3 is `https://testnet.lez.logos.co`.

## What v0.3 changes for this registry

- A program is a **header account** the deployer chose and signed for. Its
  `program_loader` shard (loader id `[0xFE; 32]`) holds
  `ProgramHeader { image_id, program_first_segment, immutable }`. Calls, PDAs
  and registry keys all use that account id, not the ImageID.
- Guests are two-phase: `Plan(PlanInput, instruction)` sees account ids and
  `is_authorized` only, never contents; `Apply(ApplyInput)` sees one shard and
  one effect. A program may emit a read-only effect on another program's shard
  (`Plan::inspect`), which is how a guest checks a header on chain.
- PDAs: `AccountId::for_public_pda(program_account_id, seed)`, same prefix as
  v0.2.4, but keyed on the header account.
- Public transactions carry `Option<FeeDeclaration>`; a charged transaction needs
  a payer holding LGO that co-signs. 10M execution gas per transaction.
- `ApplyOutput.chained_calls` is new in the release and must stay empty
  (`ApplyOutput::new`). All built-in ImageIDs changed from rc1.
- SPEL has no v0.3 release (v0.7.0-rc.2 still pins LEZ v0.2.4).

## Decisions

1. **Guest written against `lee_core` v0.3.0 directly.** Seven instructions:
   `initialize`, `register`, `update`, `claim_source`, `record_rebuild`,
   `sync_header`, `freeze`. `claim_canonical` is removed: on v0.3 the deployer
   is provable, so the canonical slot no longer has to be won by rebuilding.
2. **Deployer entry = signed by the program's own header account.** `register`
   with `deployer = true` requires the header account to be an authorized input,
   and the entry is keyed by the program id alone. Third-party entries are keyed
   by their publisher, as before.
3. **Image and frozen flag checked on chain.** `register`, `record_rebuild` and
   `sync_header` carry the header the client observed; the guest emits a
   read-only effect on the program's loader shard that fails unless the header
   there is exactly that `(image_id, immutable)`. The entry records both.
4. **Rebuilds are judged against the recorded header image**, not a
   caller-typed subject. A header that later changes is detected by
   `sync_header` (permissionless) and a verdict about the old image becomes
   `Stale`.
5. **Timestamps via validity windows.** The client reads the clock account and
   passes `Stamp { block, millis }`; the plan sets block window
   `[block, block + 600)` and timestamp window `[millis, millis + 600000)`, which
   the runtime enforces. A stamp can therefore be at most ten minutes older than
   the block that includes it, and never in the future.
6. **Cross-shard consistency by declared values.** Each apply sees one shard, so
   `register` passes the expected directory index; the global apply refuses
   unless the counter equals it, then increments. `total_verified` is dropped
   from the global account (an apply cannot see another shard's verdict); the
   indexer counts verified entries from the walk.
7. **Instruction enum lives in the core crate** (borsh), shared by guest, SDK and
   CLI. The IDL is hand-maintained in SPEL's JSON format, and a test fails if it
   drifts from the enum.
8. **SDK signs.** BIP-340 over `sha256(PREFIX || borsh(Message))`, fee payer
   co-signs, `sendTransaction` takes base64 borsh of `LeeTransaction::Public`.
   Checked end to end against a local v0.3.0 standalone sequencer.

## Order of work

core types and instruction enum, guest, SDK read path (shards), SDK tx builder
and signer, CLI, indexer/explorer parsing, Basecamp module, IDL and drift test,
local sequencer e2e with transcript, docs (README, deploy-v03.md), CI.
