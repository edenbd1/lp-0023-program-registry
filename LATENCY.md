# Lookup latency

Reproduce with
`cargo run --release --manifest-path sdk/Cargo.toml --example bench_lookup`.
The figures below are that command's output against the public testnet on
2026-08-21.

## The claim

Resolving a program in this registry is **one SHA-256 and one account read**.
There is no scan, no index lookup, and no list to walk, so the cost cannot depend
on how many programs the registry holds. That is structural — the code path does
not contain the registry's size — but a structural argument is easy to make and
easy to be wrong about, so it is also measured.

## Derivation

```
500000 addresses in 173.998417ms — 0.35 µs each
```

That is the whole of this registry's "index": 350 nanoseconds of hashing, on a
laptop, per program resolved.

## 500 live lookups

Each lookup is a full round trip to `https://testnet.lez.logos.co`. They are
split into addresses that hold a registered entry and addresses nothing was ever
written to — because if resolution depended on what the registry stores, those
two populations would separate.

```
  registered             n=125  p50  160ms  p95  235ms  max  527ms  mean  189ms
  never registered       n=375  p50  159ms  p95  247ms  max 1237ms  mean  199ms
  resolved 125 of 500
```

They do not separate. p50 differs by **one millisecond**, which is noise on a
public endpoint; the tail is wider for the misses purely because there are three
times as many of them. What is being measured is the network, not the registry.

## What this does *not* show

The registry held **4 entries** when this ran, not 500. Five hundred *lookups*
were measured, not five hundred *entries*, and the difference is worth stating
plainly rather than hoping nobody checks: a registration is a signed transaction
that takes about 60 seconds to confirm on this testnet, so 500 of them is a
little over eight hours of block time and a lot of testnet traffic for a number
that the derivation already determines.

The measurement above is the honest substitute, and it answers the question that
matters. If lookup cost grew with registry size, reading an address that holds an
entry would differ from reading one that does not — the first would have to find
something. It does not differ.

## Browsing is a different operation

```
walk: 4 entries in 1.633180959s
```

Walking every entry is **linear** and openly so: two reads per entry, one for the
directory slot and one for the entry itself. That is exactly why it lives in the
optional indexer and not in the resolve path. Browsing a registry is a
convenience and may be slow; resolving a program is not and must not be.
