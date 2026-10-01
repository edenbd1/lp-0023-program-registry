# Cycle cost on LEZ v0.3

On LEZ v0.3 a public call runs the guest once to **plan** and once more to
**apply** each effect the plan emitted, and the transaction is charged the sum
of those RISC0 cycles as execution gas (`fee_actual_base`,
`lez/programs/fee/core/src/assess.rs`). The per-transaction ceiling is
`max_gas_exec` = **10,000,000** (`getFeeState` on a v0.3.0 sequencer).

The figures below are those sums, measured the way LEZ v0.3.0's own
[`tools/cycle_bench`](https://github.com/logos-blockchain/logos-execution-zone/tree/v0.3.0/tools/cycle_bench)
measures the built-in programs (tag `v0.3.0`, commit `db66590ab`): guest inputs
written by `lee::program::Program::write_plan_inputs` and `write_apply_inputs`
(the writers the sequencer uses), `default_executor().execute` under RISC0 3.0.5
with no proving, one row per plan call and per apply call, user cycles from the
executor, wall time as best and mean of five runs after a discarded warmup.
`cycle_bench`'s fixtures are the built-in programs only, so the registry's
cases live in `tools/cycles-v03`: instructions and shard selectors built by the
SDK, shard contents carried from one instruction to the next.

```bash
cd tools/cycles-v03 && cargo run --release -- ../../artifacts/programs/lez_registry.bin
```

Guest: the committed sealed build `artifacts/programs/lez_registry.bin`, ImageID
`dd12f4a8e3c7218d85cdf6c0625014b352e65be3ff447b911766476671cd6266`
([deploy-v03.md](docs/deploy-v03.md)). Full output, with per-call segments and
executor times: [`evidence/cycles-v03.txt`](evidence/cycles-v03.txt).

| instruction | plan | applies | total cycles (execution gas) | share of the 10M ceiling |
|---|---:|---|---:|---:|
| `initialize` | 10,374 | 7,068 | 17,442 | 0.17% |
| `register` (deployer, with source claim) | 63,401 | 8,973 + 7,220 + 7,983 + 18,720 | **106,297** | 1.06% |
| `update` (version, description, IDL) | 34,442 | 21,884 | **56,326** | 0.56% |
| `claim_source` | 21,982 | 23,006 | 44,988 | 0.45% |
| `record_rebuild` (match, offline: Verified) | 23,106 | 8,973 + 21,453 | 53,532 | 0.54% |
| `sync_header` (unchanged) | 20,691 | 8,973 + 20,772 | 50,436 | 0.50% |
| `freeze` | 17,979 | 20,901 | 38,880 | 0.39% |

Every call is one segment, and each takes about 30 ms in the executor on an
Apple M-series laptop, most of it the fixed per-call cost of loading the ELF
that `cycle_bench`'s calibration reports as its intercept.

The `register` applies are, in order: the read-only check of the program's
header in the loader shard (8,973), the global counter (7,220), the directory
slot (7,983), and the entry itself (18,720). The same 8,973-cycle header check
is what `record_rebuild` and `sync_header` pay to compare against the chain.

The entry this sequence ends with is 500 bytes in the registry's shard.

## What the numbers say

- Every instruction is about 1% of the ceiling or less, so the CLI's default gas
  limit of 1,000,000 leaves roughly a tenfold margin and keeps the fee reserve
  small. `PROVENANCE_GAS_LIMIT` overrides it.
- A refused call (a guest panic, such as an update by someone who is not the
  author) is charged its **whole gas limit**: the executor reports no cycle
  count for a panicked session. The local run shows it: a refused update with the
  default 1,000,000 limit cost 8,004,200 at a base fee of 8, against 448,464 for the
  accepted one. Keeping the limit near the
  real cost is what keeps a refusal cheap.
- `plan` dominates `register` because it builds and validates the whole entry
  (and derives three PDAs) before any shard is touched; the applies only check
  and write.
- Nothing scales with the number of entries: resolving, registering and
  updating cost the same whether the registry holds four programs or four
  million.
- Hashing an artifact on chain is still out of the question. Verifying a
  rebuild needs no hash at all on v0.3: the loader already recorded the ImageID
  in the header, and the registry compares 32 bytes against it.
