# Cycle cost on LEZ v0.3

On LEZ v0.3 a public call runs the guest once to **plan** and once more to
**apply** each effect the plan emitted, and the transaction is charged the sum
of those RISC0 cycles as execution gas (`fee_actual_base`,
`lez/programs/fee/core/src/assess.rs`). The per-transaction ceiling is
`max_gas_exec` = **10,000,000** (`getFeeState` on a v0.3.0 sequencer).

The figures below are those sums, measured by running the real guest binary
through exactly that plan-then-apply sequence under the RISC0 3.0.5 executor
(the one the sequencer uses when it does not prove), with instructions and
shard selectors built by the SDK and shard contents carried from one
instruction to the next:

```bash
cd tools/cycles-v03 && cargo run --release -- <lez_registry.bin>
```

Guest: ImageID `0e3fe974e184e40062811f28c4d8c17df208bf80ee0ce7ee09148aa0f8dfa53d`
(the build the local end-to-end run deployed, `evidence/e2e-v03-local.txt`).

| instruction | plan | applies | total cycles (execution gas) | share of the 10M ceiling |
|---|---:|---|---:|---:|
| `initialize` | 10,164 | 6,897 | 17,061 | 0.17% |
| `register` (deployer, with source claim) | 63,014 | 8,833 + 7,078 + 7,811 + 18,667 | **105,403** | 1.05% |
| `update` (version, description, IDL) | 33,806 | 21,805 | **55,611** | 0.56% |
| `claim_source` | 21,802 | 22,939 | 44,741 | 0.45% |
| `record_rebuild` (match, offline: Verified) | 22,889 | 8,833 + 21,425 | 53,147 | 0.53% |
| `sync_header` (unchanged) | 20,469 | 8,833 + 20,703 | 50,005 | 0.50% |
| `freeze` | 17,760 | 20,836 | 38,596 | 0.39% |

The `register` applies are, in order: the read-only check of the program's
header in the loader shard (8,833), the global counter (7,078), the directory
slot (7,811), and the entry itself (18,667). The same 8,833-cycle header check
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
