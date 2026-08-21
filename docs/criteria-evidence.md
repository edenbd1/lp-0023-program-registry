# LP-0023 criteria, and where this submission actually stands

The prize text is [PR #127](https://github.com/logos-co/lambda-prize/pull/127) on
`logos-co/lambda-prize` (`prizes/LP-0023.md`, status **Draft**). It is not yet
merged, so the criteria below are quoted from that PR and may still change.

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
| 12 | Operates end-to-end against Logos testnet 0.3 | **partial** | live on the public LEZ testnet; the "0.3" label has not been confirmed against this endpoint |

## Usability

| # | Criterion | Status | Evidence |
|---|---|---|---|
| 13 | SDK for third parties: register, resolve, search | **partial** | `sdk/` resolves and enumerates; it does not yet build register/update transactions |
| 14 | CLI covering register, update, lookup, search, verification | **partial** | `lookup`, `search`, `entry-address`, `deployment-digest`, `verify`, `explain-entry` and `idl` all run against the live chain; **register and update still go through `spel`** and building those transactions in-process is not done |
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
| 24 | Document compute units for register and update | **partial** | [CYCLES.md](../CYCLES.md) measures the register body at 6,294 cycles; `update` is not separately measured |
| 25 | Lookup latency, and search across a registry of **at least 500 entries** | **partial** | [LATENCY.md](../LATENCY.md) measures 500 live lookups; the registry holds 4 entries, and the gap is stated there rather than glossed |

## Supportability

| # | Criterion | Status | Evidence |
|---|---|---|---|
| 26 | Deployed and tested on testnet 0.3, program ID in the README | **partial** | program ID published; testnet label unconfirmed |
| 27 | Builds and runs end-to-end on macOS (Apple Silicon) **and** Linux (x86_64), demo unmodified from a clean clone | **partial** | `scripts/demo.sh` is written portably and has been run on macOS only |
| 28 | Basecamp module built with `logos-module-builder` (`mkLogosModule`) | **partial** | `app/CMakeLists.txt` takes that path when `LOGOS_MODULE_BUILDER_ROOT` is set; the shipped `.lgx` was packaged with `lgx` from `logos-package` |
| 29 | IDL generated via SPEL; **the registry registered in itself** | **met** | `idl/`, and entry 0 is the registry |
| 30 | End-to-end integration tests against a real sequencer, in CI, green on default branch | **met** | `sdk/tests/e2e.rs` reads the live registry and asserts on what comes back: the registry is registered in itself, the derivation still agrees with the runtime, the counters match what the walk finds, all four verification states are present on chain, and an unregistered program resolves to nothing rather than erroring. Run as a required CI job, not skipped |
| 31 | README documents setup, program ID, CLI and Basecamp usage step by step | **partial** | README covers design and layout; step-by-step usage is thin |
| 32 | SDK docs with a worked resolve example | **open** | crate docs exist; no worked example |
| 33 | Reproducible end-to-end demo against a **local** sequencer with `RISC0_DEV_MODE=0` | **partial** | `demo.sh` runs read-only against the public testnet |

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
