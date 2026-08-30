# Lookup latency, and search across ≥500 entries

Two commands reproduce everything here:

- `cargo run --release --manifest-path sdk/Cargo.toml --example bench_lookup`
  times 500 **lookups** against the public testnet — the size-independence
  argument, measured on the live endpoint.
- `./scripts/populate-and-measure-500.sh` stands up a **local** standalone
  sequencer, registers **500 distinct entries** on it, and measures lookup and
  search across all 500 — the scale the criterion names, which the public
  testnet cannot supply cheaply (see "Why the scale run is local", below).

## The claim

Resolving a program in this registry is **one SHA-256 and one account read**.
There is no scan, no index lookup, and no list to walk, so the cost cannot depend
on how many programs the registry holds. That is structural — the code path does
not contain the registry's size — but a structural argument is easy to make and
easy to be wrong about, so it is measured two ways: on the public testnet against
a handful of entries, and on a local chain against 500.

## Derivation

```
500000 addresses in 225.15ms — 0.45 µs each
```

That is the whole of this registry's "index": under half a microsecond of hashing
per program resolved, and it is the only registry-side work a lookup does.

## 500 live lookups (public testnet)

Each lookup is a full round trip to `https://testnet.lez.logos.co`. They are
split into addresses that hold a registered entry and addresses nothing was ever
written to — because if resolution depended on what the registry stores, those
two populations would separate. (Run 2026-08-21.)

```
  registered             n=125  p50  160ms  p95  235ms  max  527ms  mean  189ms
  never registered       n=375  p50  159ms  p95  247ms  max 1237ms  mean  199ms
  resolved 125 of 500
```

They do not separate. p50 differs by **one millisecond**, which is noise on a
public endpoint; the tail is wider for the misses purely because there are three
times as many of them. What is being measured is the network, not the registry.

## Search across a registry of at least 500 entries (local standalone sequencer)

`./scripts/populate-and-measure-500.sh` registers **500 distinct programs** on a
real `sequencer_service` running in standalone mode on a throwaway local port,
with `RISC0_DEV_MODE=0` exported for every process — the same harness the
criterion-33 e2e uses. Each program id is `sha256("lp0023-500-bench:" ‖
u64_le(i))`, each entry a canonical registration authored by the genesis signer;
registration is serial because each one must land in the directory slot the
previous one incremented the counter into. The 500 landed in **314 s** (~0.63 s
each). The count is then read straight off the global account before anything is
measured:

```
total_entries on chain: 500
```

All three measurements below use the same SDK `Client` the explorer and the
indexer use — `entry` to resolve, `walk` to enumerate — so these are the real
code paths, only the chain is local. (Run 2026-08-31, on a laptop.)

**Single-entry lookup, at 500 entries.** A hit reads an address really written to;
a miss reads one nothing was. If resolution cost grew with how full the registry
is, the hit — which would have to *find* something — would separate from the miss.

```
  registered (hit)     n=250  p50 222µs  p95 328µs  max 806µs  mean 243µs
  unregistered (miss)  n=250  p50 212µs  p95 316µs  max 430µs  mean 233µs
  resolved 250 of 500
```

They do not separate: p50 differs by **10 microseconds** across the two
populations, with 500 entries on chain. Reading an entry costs the same as
reading an empty address, which is the structural claim, now measured at scale.
The whole lookup is sub-millisecond because the round trip is local; on a public
endpoint it is the ~160 ms network round trip above, and still independent of
size.

**Full-registry enumeration.** `walk` is what "search across ≥500 entries" runs
on top of: it reads the counter, then two accounts per entry (the directory slot
and the entry it names), because LEZ has no `getProgramAccounts`. Linear, and
openly so — five walks of the full 500:

```
  walked 500 entries — p50 215ms  max 282ms  mean 239ms  (1001 reads per walk)
```

1001 sequential reads (one global + two per entry) in ~240 ms over the local
endpoint. This is the number that scales with the registry: it is dominated by
the per-read round trip, so on the public testnet the same 500-entry walk would
be ~1001 × ~160 ms uncached — which is exactly why the walk lives in the optional
indexer and the explorer serves it from a cache, never in the resolve path.

**Search proper.** Search is one `walk` followed by the same in-memory predicate
the CLI's `search`, the explorer and the indexer apply. The filter runs over
entries already in hand, so its cost is noise beside the walk:

```
  bench-prog         500 hits — filter 46µs   (every synthetic entry)
  bench-prog-0042      1 hit  — filter 32µs   (one exact entry)
  bench-prog-01      100 hits — filter 28µs   (a ten-entry prefix)
```

So a full search across 500 entries is the ~240 ms enumeration plus ~40 µs of
filtering: the walk is the whole story, and it is sub-second locally for the full
registry.

### Why the scale run is local

Standing up 500 *entries* on the public testnet is not the same order of cost as
500 *lookups*. A registration is a signed transaction; on the public endpoint it
confirms in about a minute, so 500 serial registrations is the better part of a
working day of block time and a lot of public traffic — for a number a local
chain answers exactly. The local standalone sequencer is a real
`sequencer_service`, not a mock: it executes each registration for real (the
guest ELF, the 32M-cycle limit, the pinned clock account the program refuses to
proceed without), with dev mode off. What differs from the public testnet is only
the block cadence and the network round trip — the first is why populating 500 is
minutes not hours, the second is why the reads are microseconds not milliseconds.
Neither changes the shape of what is measured: lookup is size-independent, and
search is a linear walk whose cost is per-read latency times entry count.

## What this leaves to the network

On the public testnet the registry held **4 entries** when the lookup run above
was taken, and that is still true — the 500-entry claim is answered on the local
chain, deliberately, for the reason just given. The one thing a local chain
cannot reproduce is public-endpoint round-trip latency, and that is precisely the
term that makes an uncached 500-entry *walk* slow in production. The design's
answer to that is not to make the walk faster but to keep it out of the path that
has to be fast: resolution never walks, and browsing is cached. Both halves are
measured above.
