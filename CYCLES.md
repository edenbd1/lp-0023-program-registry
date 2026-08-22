# zkVM cycle cost

Measured with the RISC0 3.0.5 executor (no proving), guest built in release with
`overflow-checks = true` — the same semantics an on-chain program ships with.
Reproduce with `cd zkvm && cargo run --release`. **This file is regenerated from
that command's output**, so a figure here and a figure on your screen cannot
drift apart.

Counts are `env::cycle_count()` deltas taken inside the guest, with the
90-cycle cost of the measurement itself subtracted from every row.
`core::hint::black_box` stops the optimiser hoisting a pure function out of the
timed region.

The number that matters is LEZ's **32,000,000-cycle cap on public execution**.

## Per operation

| op | cases | median | min | max |
|---|---:|---:|---:|---:|
| `entry_seed` | 8 | 4718 | 4718 | 4718 |
| `third_party_entry_seed` | 8 | 9215 | 9215 | 9215 |
| `seq_seed` | 8 | 4720 | 4720 | 4720 |
| `global_seed` | 1 | 110 | 110 | 110 |
| `Verification::claim` | 8 | 451 | 447 | 451 |
| `record_rebuild (accepts)` | 8 | 625 | 625 | 625 |
| `record_rebuild (refuses)` | 8 | 625 | 625 | 625 |
| `Entry::validate` | 8 | 35 | 35 | 35 |
| `Entry serialise` | 8 | 861 | 861 | 861 |
| `Entry deserialise` | 8 | 3873 | 3873 | 3873 |
| `deployment_digest 4KiB` | 4 | 261637 | 261636 | 261637 |
| `deployment_digest 64KiB` | 4 | 4105477 | 4105476 | 4105477 |

## Per instruction body

Each measured whole rather than summed from the rows above, because a sum omits
the decode of what is already stored — and for `update` that decode is most of
the cost.

| instruction body | cycles | share of the cap |
|---|---:|---:|
| `register` | 6399 | 0.0200% |
| `update` | 5215 | 0.0163% |
| `claim_source` | 6166 | 0.0193% |
| `claim_canonical` | 6627 | 0.0207% |

`update` is the cheapest of the four despite doing a decode, an edit, a
validation and a re-encode: it derives no addresses. `register` and
`claim_canonical` each pay for a seed derivation, which at ~4,700 cycles is the
single most expensive thing any of them does.

Nothing here is within three orders of magnitude of the cap.

## The one row that decides a design question

`deployment_digest` is the only operation whose cost scales with its input, at
**62.6 cycles per byte**. Projected onto this registry's own artifact
(486,112 bytes) that is **≈30,452,295 cycles, 95% of the entire 32M
budget** — for the hash alone, before one line of program logic runs.

So the chain cannot check an artifact against its digest on chain. Not "should
not": cannot afford to. That is a second and independent reason for keeping
verification off chain, the first being that a chain has no compiler.

Off chain the same digest costs microseconds and the answer is identical, which
is why it lives in the CLI and the SDK.

## What this rules out

Anything that would make a registry operation scale with the number of entries.
`entry_seed` is a single SHA-256 over 53 bytes and costs the same whether the
registry holds four programs or four million.
