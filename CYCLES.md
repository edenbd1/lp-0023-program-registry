# zkVM cycle cost

Measured with the RISC0 3.0.5 executor (no proving), guest built in release with
`overflow-checks = true` — the same semantics an on-chain program ships with.
Reproduce with `cd zkvm && cargo run --release`. **This table is regenerated from
that command's output**, so a figure here and a figure on your screen cannot
drift apart; if they do, the harness changed and this document is wrong.

Counts are `env::cycle_count()` deltas taken inside the guest, with the
90-cycle cost of the measurement itself subtracted from every row.
`core::hint::black_box` wraps each call so the optimiser cannot hoist a pure
function out of the timed region. Repeated runs are byte-identical — every `min`
below equals its `max` except where the input genuinely varies.

The number that matters is LEZ's **32,000,000-cycle cap on public execution**.

| op | cases | median | min | max |
|---|---:|---:|---:|---:|
| `entry_seed` | 8 | 4718 | 4718 | 4718 |
| `third_party_entry_seed` | 8 | 9215 | 9215 | 9215 |
| `seq_seed` | 8 | 4720 | 4720 | 4720 |
| `global_seed` | 1 | 110 | 110 | 110 |
| `Verification::claim` | 8 | 442 | 438 | 442 |
| `record_rebuild (accepts)` | 8 | 612 | 612 | 612 |
| `record_rebuild (refuses)` | 8 | 612 | 612 | 612 |
| `Entry::validate` | 8 | 35 | 35 | 35 |
| `Entry serialise` | 8 | 830 | 830 | 830 |
| `Entry deserialise` | 8 | 3786 | 3786 | 3786 |
| `deployment_digest 4KiB` | 4 | 261637 | 261636 | 261637 |
| `deployment_digest 64KiB` | 4 | 4105477 | 4105476 | 4105477 |

Whole `register` body — derive the seed, build the entry, validate it, serialise
it: **6,294 cycles**, or **0.0197%** of the cap. Registering a program is not where
a registry runs out of budget.

## The one row that is expensive, and what it decides

`deployment_digest` is the only operation whose cost scales with its input, at
**62.6 cycles per byte**. Projected onto this registry's own artifact
(462,564 bytes) that is **≈28,977,140 cycles, 91% of the entire 32M
budget** — for the hash alone, before one line of program logic runs.

So the chain cannot check an artifact against its digest on chain. Not "should
not": cannot afford to. That is a second and independent reason for the design
decision in the README. The first is that a chain has no compiler; this one is
that even the cheap half of verification does not fit in the budget.

It is also why `deployment_digest` lives in the CLI and the SDK rather than in an
instruction. Off chain it costs microseconds and needs no budget at all, and the
answer is identical.

## What this rules out

Anything that would make a registry operation scale with the number of entries.
`entry_seed` is a single SHA-256 over 53 bytes and costs the same whether the
registry holds four programs or four million — the on-chain half of the property
the lookup benchmark measures off chain.
