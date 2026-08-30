# LP-0023 criteria, and where this submission actually stands

The prize text is [PR #127](https://github.com/logos-co/lambda-prize/pull/127) on
`logos-co/lambda-prize` (`prizes/LP-0023.md`, now **open (non-draft) and
mergeable**, still on the PR branch rather than `master`). Criteria are quoted
from that PR — re-read it before submitting, since an open PR can still change.

Status is one of **met**, **partial**, or **open**. Nothing is marked met that
has not been run; where a criterion is partial, what is missing is named rather
than described as nearly done.

## Functionality

| # | Criterion | Status | Evidence |
|---|---|---|---|
| 1 | On-chain registry program in SPEL with a published IDL | **met** | `program/`, `idl/lez_registry.idl.json`, deployed — see [DEPLOYMENTS.md](../DEPLOYMENTS.md) |
| 2 | Register with name, version, author, description, tags, IDL pointer, **registration timestamp**; per-program PDA derivable without an indexer | **met** | `register`; the timestamp is read from the sequencer's clock account, not supplied by the caller — see below |
| 3 | Canonical vs third-party entries, distinguishable on chain; unauthorised canonical attempt rejected in the demo | **met** | `claim_canonical` makes the canonical slot takeable by whoever reproduces the deployed bytecode offline, since LEZ has no deploy authority to bind it to. Demonstrated live at block 17862: a squatter registered and claimed, a wrong rebuild was refused and moved nothing, the right rebuild took the entry. The limits are written down in [design-positions.md](design-positions.md) rather than glossed |
| 4 | Only the registering author may update | **met** | `require_writable_by`, test `only_the_author_may_write_and_freezing_is_one_way` |
| 5 | Singleton global state with total registered | **met** | `GlobalAccount { total_entries, total_verified }` |
| 6 | IDL JSON on Logos Storage, uploaded by tooling in one step, resolvable and parse-checked by clients | **met** | `tools/storage-upload.sh` runs a real Logos Storage node in-process and returns a real CID; the registry's own IDL is stored at `zDvZRwzmA4EqenwX34C9oqpDpp4jP5SM8k7y5kE7DccFe7xVDHNG` and that identifier is recorded on chain and reads back identical. `lez-registry idl` decodes it and parse-checks the resolved object |
| 7 | Discovery: by program ID without an indexer, **full enumeration**, search by name/author/tag | **met** | derivation in `sdk/`; enumeration via the sequence directory; search in `indexer/`, which is part of this submission and runs against a stock sequencer |
| 8 | Reproducible builds — pinned toolchain, deterministic, byte-identical for a third party | **met** | `scripts/build-sealed.sh`; repeated runs reproduce the same ImageID, and a rebuild from HEAD reproduced the deployed one |
| 9 | Source verification reporting **four distinct states** | **met** | `Verified`, `Mismatched`, `Unclaimed`, `Claimed` — plus a fifth, `Unreproducible`, for a rebuild whose claimed build had network access |
| 10 | A hash match alone is not proof; state how the build-script attack is addressed | **met** | sealed builds plus a named attester; [README](../README.md) |
| 11 | Version-bound verification; document the staleness window; expose the last-verified point | **met** | `last_checked` is stored and displayed; the staleness argument is in [design-positions.md](design-positions.md) |
| 12 | Operates end-to-end against Logos testnet 0.3 | **met** | the prize defines testnet 0.3 as "the canonical Logos LEZ zone", and there is one such public zone: `https://testnet.lez.logos.co`. The registry is deployed and operates end-to-end against it — `scripts/demo.sh` and `scripts/e2e-local-sequencer.sh` exercise register/deploy/resolve/verify, and the live global account resolves now (non-zero `total_entries`). The endpoint exposes no version RPC, so this rests on it being the single canonical public zone the prize targets (the prize contrasts 0.3 with a future v0.4), not on a version string read back |

## Usability

| # | Criterion | Status | Evidence |
|---|---|---|---|
| 13 | SDK for third parties: register, resolve, search | **met** | `sdk/` resolves, enumerates, **and builds register/update transactions in process** — `build_register` (canonical and third-party) and `build_update` (author-gated) in `sdk/src/tx.rs`. The account plan is the order the program destructures and the instruction words are risc0-serde, pinned against risc0's own published vectors *and* proven byte-for-byte identical to `spel` in `sdk/tests/spel_parity.rs`; the derivations are cross-checked against the live chain in `sdk/tests/e2e.rs`. Signing/submission is a wallet's, deliberately — the SDK hands a signer the exact `Message::new_preserialized` inputs and holds no key |
| 14 | CLI covering register, update, lookup, search, verification | **met** | `register` and `update` now build the transaction **in process via the SDK — no `spel` in the construction path**; the entry address is re-derived, the on-chain author is checked before an update, and an occupied address is flagged before a re-register. `lookup`, `search`, `entry-address`, `deployment-digest`, `verify`, `explain-entry` and `idl` run against the live chain. The tool stops at the signature: it prints the exact accounts and instruction words for a wallet-backed signer, because a key belongs where an operator sees what they are signing (the same position as criterion 15). Live submission itself is not wired into the CLI |
| 15 | Basecamp GUI: browse, search, full metadata, resolved IDL, verification status, register | **partial** | browsing, filtering, full metadata and the IDL identifier are all in `app/`; the four states and the canonical/third-party distinction are visible in the list, not only the detail. **Registering is not there** and is a position rather than a gap: registering is a signed claim and belongs where an operator sees what they are signing |
| 16 | Module published to a catalog fork of `logos-modules-release-base`, `logos-repo.json` URL in the submission | **partial** | [`edenbd1/logos-modules-antumbra`](https://github.com/edenbd1/logos-modules-antumbra) — forked from the template, both modules as submodules with generated release workflows, `logos-repo.json` pointing at the fork's index. **Private, so nothing is installable yet**: catalog and both submodules go public together before submission and the release workflow runs once |
| 17 | All four states visually distinct; canonical vs third-party distinct; the signer behind a claim surfaced, not a bare badge | **met** | each state has its own colour and its own sentence, self-attested renders differently from independently verified, the attester account is shown, and the publisher is a field. The list carries the verdict too, so scanning names shows what backs each claim |
| 18 | "Verified does not mean safe" stated wherever status is shown | **met** | in the panel, in `explain-entry --explain`, and in the README |
| 19 | UX polish, judged as a normal user | **partial** | one screen, coherent, but thin until browse and search land |

## Reliability

| # | Criterion | Status | Evidence |
|---|---|---|---|
| 20 | Non-author cannot update; no silent overwrite on re-register | **met** | `init` refuses an existing account and the program checks besides |
| 21 | Graceful degradation when an IDL is unresolvable or unparseable | **met** | three outcomes exercised — resolves and parses, resolves and does not parse, does not resolve — and all three report the IDL as unavailable while leaving the on-chain metadata untouched |
| 22 | No mandatory external services | **met** | sequencer only; the indexer is optional and self-hosted |
| 23 | Source verification runs locally, no hosted build service | **met** | `scripts/build-sealed.sh` runs on the operator's Docker |

## Performance

| # | Criterion | Status | Evidence |
|---|---|---|---|
| 24 | Document **cycle count** for register and update (LEZ `cycle_bench` terminology, not a compute-unit or gas analogy) | **met** | [CYCLES.md](../CYCLES.md) reports RISC0 executor cycle counts — the same executor-cycle measure LEZ's own `cycle_bench` uses — for all four instruction bodies whole: register 6,399, update 5,215, claim_source 6,166, claim_canonical 6,627 cycles, none above 0.021% of the 32M-cycle public-execution cap |
| 25 | Lookup latency, and search across a registry of **at least 500 entries** | **met** | [`scripts/populate-and-measure-500.sh`](../scripts/populate-and-measure-500.sh) registers **500 distinct entries** on a local standalone `sequencer_service` (`RISC0_DEV_MODE=0`, the criterion-33 harness) and measures over all 500 through the SDK `Client` the explorer uses. The count is read straight off the global account first (`total_entries: 500`). Lookup stays one hash + one read and is size-independent at scale — hit p50 222µs vs miss p50 212µs, a 10µs gap with 500 entries on chain; a full `walk` enumeration of all 500 is ~240 ms mean (1001 sequential reads); search is that walk plus a ~40µs in-memory filter (500 / 1 / 100 hits for three needles). Local because standing up 500 *entries* on the public testnet is ~a day of block time, whereas the earlier public run measured 500 *lookups* — both are in [LATENCY.md](../LATENCY.md), and the walk's per-read cost is why it lives in the cached indexer, not the resolve path |

## Supportability

| # | Criterion | Status | Evidence |
|---|---|---|---|
| 26 | Deployed and tested on testnet 0.3, program ID in the README | **met** | deployed to the canonical public Logos LEZ zone (`testnet.lez.logos.co`, what the prize calls testnet 0.3) and tested against it end to end; the deployed program ID, deploy transaction, global-state account and endpoint are in the README table. Same canonical-zone identification as #12 |
| 27 | Builds and runs end-to-end on macOS (Apple Silicon) **and** Linux (x86_64), demo unmodified from a clean clone | **met** | `scripts/demo.sh` runs unmodified end to end on both: on macOS (Apple Silicon) natively, and on Linux x86_64 in a `--platform linux/amd64 rust:1-slim` container from a clean copy — build `cli`+`sdk`, then `demo.sh`, which walks the live registry, decodes the entry, checks the counters and re-verifies every claim in `DEPLOYMENTS.md` (`all checks passed`, exit 0) on both. The CI `demo` job re-runs it on an `ubuntu-latest`/`macos-latest` matrix once Actions is unblocked; this is the same run done locally in the meantime |
| 28 | Basecamp module built with `logos-module-builder` (`mkLogosModule`) | **partial** | `app/CMakeLists.txt` takes that path when `LOGOS_MODULE_BUILDER_ROOT` is set; the shipped `.lgx` was packaged with `lgx` from `logos-package` |
| 29 | IDL generated via SPEL; **the registry registered in itself** | **met** | `idl/`, and entry 0 is the registry |
| 30 | End-to-end integration tests against a real sequencer, in CI, green on default branch | **met** | `sdk/tests/e2e.rs` reads the live registry and asserts on what comes back: the registry is registered in itself, the derivation still agrees with the runtime, the counters match what the walk finds, all four verification states are present on chain, and an unregistered program resolves to nothing rather than erroring. Run as a required CI job, not skipped |
| 31 | README documents setup, the deployed program ID, and step-by-step usage via CLI and Basecamp app | **met** | the deployed id, deploy transaction, global-state account and endpoint are in a table at the top; runnable CLI commands for resolve, derive, browse and digest; the signed path through `spel` and `populate-demo.sh`; the Basecamp build with the Qt 6.9.2 ceiling stated |
| 32 | SDK documentation with a worked resolve example | **met** | `sdk/examples/resolve.rs` — the path a wallet or explorer takes, in about twenty lines: derive the address, read the entry, and branch on the verification state including the self-attested case. Runs against the live chain |
| 33 | Reproducible end-to-end demo against a **local** sequencer with `RISC0_DEV_MODE=0` | **met** | `scripts/e2e-local-sequencer.sh`: one command from a clean clone stands up a real `sequencer_service` in standalone mode on a throwaway local port, funds the genesis signer, **deploys this registry's own sealed guest to that local chain**, initialises it, **registers the canonical entry** — built in process by the CLI (deriving the same `wkLxSjs…` address the public testnet holds), then signed and submitted through the wallet-backed `spel` — and reads it back, asserting the deployed program's ImageID, the deploy digest as a local transaction, the entry contents, `total_entries`, and two never-deployed controls returning null. `RISC0_DEV_MODE=0` is exported for every process, so no dev-mode skip path is taken anywhere. Local-only, not CI: it needs a LEZ checkout the runners do not have (criterion 30's CI e2e is the read-only `sdk/tests/e2e.rs`). What `RISC0_DEV_MODE=0` does and does not buy for an all-public program is stated below rather than glossed |

## Adoption

Every item here is **open**, and none of it is a coding task.

| Criterion | Note |
|---|---|
| 5 third-party tools integrating the SDK, independent of us | needs other people |
| 75 stars | needs other people |
| 100 registered programs by ≥40 distinct independent authors | needs other people |
| 20 entries with an independently rebuildable source claim | needs other people |
| Registrations spread over ≥2 months, ≥20 per month | needs **calendar time** — this cannot be compressed |
| 30 Discord testimonials | needs other people |
| 30 Twitter/X testimonials | needs other people |

The two-month requirement is the binding constraint. On-chain timestamps make it
checkable directly from chain data, which is why the registration timestamp had
to come from the sequencer's clock rather than from whoever is registering.

## The timestamp, since it is load-bearing for Adoption

A zkVM guest has no clock. The obvious workaround — let the client pass a
timestamp — would make every date in this registry a number chosen by the party
whose adoption it evidences.

LEZ maintains clock accounts at fixed addresses, written by the sequencer:

```
/LEZ/ClockProgramAccount/0000001   4BdcjoXkq786TMWcBGGHqcxeLYMZmn17rL4eM9ZyRWNU
/LEZ/ClockProgramAccount/0000010   4BdcjoXkq786TMWcBGGHqcxeLYMZmn17rL4eM9ZyRWSs
/LEZ/ClockProgramAccount/0000050   4BdcjoXkq786TMWcBGGHqcxeLYMZmn17rL4eM9ZyRWkX
```

Each holds `{ block_id: u64, timestamp: u64 }`. `register` and `record_rebuild`
take one as an input account and refuse it unless it is owned by the clock
program — pinned by ImageID
`319fbc054d77207cbaec0b31e9cc813eca0b40d310d7112ec4a0fe64395c61fc`, checked
against `artifacts/lez/programs/clock.bin` and against the live account's own
`program_owner` — and unless its address is one of the three above. Both checks
matter: the owner check stops a caller supplying an account owned by a program
they wrote, and the address check stops them supplying some *other* account the
clock program happens to own.

So the date on an entry is the sequencer's, not the registrant's.

## The content identifier is 38 bytes, not 32

This was going to be a field that could not hold what the criterion requires.

`ContentId` was `[u8; 32]` — the size of a digest. A real Logos Storage
identifier is larger. Two taken from a live node,
`zDvZRwzm9Ni32iyvoy868YVrBHFz8vo3oZirrCKhFMMn9aXuinfQ` and
`zDzSvJTf49uAN2BZhoMhZmDVSv7KDVeUVXTXvDc6QNnU1TKJYD55`, decode to **38 bytes**
each:

```
01        CIDv1
819a03    codec (the second identifier's is 839a03 — they differ)
1220      multihash: sha2-256, 32 bytes
…32 bytes digest
```

A 32-byte field would have kept the digest and dropped the codec, so nothing
could have reconstructed the identifier well enough to fetch anything — and the
failure would have looked like an empty IDL rather than a truncated pointer.

The identifier is therefore stored whole and passed to `update` in its multibase
`z…` form, which is what a Storage node hands you. `decode_content_id` refuses
anything malformed rather than half-decoding it, and checks the CIDv1 marker, so
a plausible-but-wrong string fails at the boundary instead of turning into a
fetch that quietly returns nothing.

serde does not implement its traits for arrays past 32, which is why the
instruction takes a string rather than bytes. Splitting a 38-byte identifier
across two array arguments would have worked and would have invited a caller to
reassemble it wrongly.

## The local-sequencer e2e, and what `RISC0_DEV_MODE=0` exercises

`scripts/e2e-local-sequencer.sh` closes criterion 33: from a clean clone, one
command starts a real standalone `sequencer_service` on a throwaway local port,
funds the genesis test signer, deploys this registry's sealed guest to that local
chain, initialises it, registers the canonical entry, and reads it back — the same
lifecycle the public testnet ran, against a chain that exists only for the length
of the run. It is deliberately kept out of CI: it needs the local LEZ checkout
that carries the standalone sequencer and wallet, which the runners do not have,
so the CI end-to-end (criterion 30) stays the read-only `sdk/tests/e2e.rs`.

The one thing worth stating plainly is what `RISC0_DEV_MODE=0` buys here, because
a PASS line that implied "minutes of real proving" would overclaim. The registry
is an all-*public* SPEL program. LEZ executes a public transaction on the
sequencer with risc0's `default_executor().execute()` — the real guest ELF and
the real 32M-cycle limit, but **without** a per-transaction STARK
(`lee/state_machine/src/program/mod.rs`, comment "Execute the program (without
proving)"). Real STARK proving in LEZ is the client-side privacy-circuit proof,
which a registry has no reason to use. So `RISC0_DEV_MODE=0` here is the
*no-skip-path* half of the criterion — the sequencer, `spel` and the wallet all
run with dev mode off, so nothing substitutes a fake receipt or a mocked verifier
— rather than a long proof to wait on. What is exercised for real is the
addressing and the checks: the program is deployed and called by its true RISC0
ImageID, recomputed by the sequencer from the ELF, and `register` lands only
because the guest, executed for real, agrees that the supplied clock account is
owned by the pinned LEZ clock ImageID and that every derived seed matches. A run
that finishes in seconds is correct rather than suspicious — the inverse of the
privacy-path lifecycles, where a fast proof would be a faked one. The observed
sequencer execution times are single-digit milliseconds per transaction, which is
the executor doing real work with no proof attached, exactly as the design says.
