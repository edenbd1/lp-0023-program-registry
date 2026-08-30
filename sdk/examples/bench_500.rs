// SPDX-License-Identifier: MIT OR Apache-2.0
//! Measure lookup and search latency against a registry that actually holds
//! **at least 500 entries**, on a local standalone LEZ sequencer.
//!
//! `bench_lookup` makes the structural argument — resolution is one SHA-256 and
//! one account read, so it cannot depend on registry size — and measures it with
//! 500 *lookups* against the public testnet, where standing up 500 *entries*
//! would be hours of block time. This example closes that gap the honest way: run
//! it against a chain the populate script has filled with ≥500 real entries, and
//! it measures the three things that matter at that scale, all through the same
//! SDK `Client` the explorer and the indexer use:
//!
//!   1. resolution of a registered program (a hit) vs. an unregistered one (a
//!      miss). If lookup cost grew with the number of entries, the hit — which
//!      would have to *find* something — would separate from the miss. It does
//!      not; both are one hash and one read.
//!   2. full-registry enumeration via `Client::walk`, the `getProgramAccounts`-
//!      free directory walk the explorer's `/search` and the indexer both run.
//!      This one *is* linear in the entry count, openly, and this is where the
//!      500-entry number bites.
//!   3. search proper: `walk` followed by the same in-memory predicate the CLI's
//!      `search`, the explorer and the indexer apply. Its cost is the walk plus a
//!      filter over already-fetched entries, so the walk is the whole story.
//!
//! Configuration is by environment so the same binary runs against any chain:
//!   BENCH_ENDPOINT   RPC URL of the sequencer            (required)
//!   BENCH_REGISTRY   registry program id, 64 hex chars   (required)
//!   BENCH_N          how many entries were registered    (default 500)
//!   BENCH_LOOKUPS    single-entry lookups to time        (default 500)
//!   BENCH_WALKS      full-registry walks to time         (default 5)
//!
//! The registered program ids are derived by the same rule the populate script
//! uses — `sha256("lp0023-500-bench:" ‖ u64_le(i))` — so a "hit" here reads an
//! address the script really wrote to, and a "miss" reads one nothing was.

use lez_registry_sdk::{base58, entry_address, Client, ProgramId};
use sha2::{Digest, Sha256};
use std::time::Instant;

/// The i-th synthetic program id. Must match the populate script byte for byte.
fn pid(i: u64) -> ProgramId {
    let mut h = Sha256::new();
    h.update(b"lp0023-500-bench:");
    h.update(i.to_le_bytes());
    h.finalize().into()
}

/// A program id nothing was ever registered under: a distinct domain, so it can
/// never collide with a real entry's `pid(i)`.
fn miss_pid(i: u64) -> ProgramId {
    let mut h = Sha256::new();
    h.update(b"lp0023-500-bench:MISS:");
    h.update(i.to_le_bytes());
    h.finalize().into()
}

fn env_or<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn hex32(s: &str) -> ProgramId {
    let s = s.strip_prefix("0x").unwrap_or(s);
    assert_eq!(s.len(), 64, "BENCH_REGISTRY must be 64 hex characters");
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).expect("valid hex");
    }
    out
}

fn pct(sorted: &[u128], p: usize) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    sorted[(sorted.len() - 1) * p / 100]
}

fn report(label: &str, mut v: Vec<u128>) {
    if v.is_empty() {
        println!("  {label:<22} (none)");
        return;
    }
    v.sort_unstable();
    let mean = v.iter().sum::<u128>() / v.len() as u128;
    println!(
        "  {label:<22} n={:<4} p50 {:>5}µs  p95 {:>6}µs  max {:>6}µs  mean {:>5}µs",
        v.len(),
        pct(&v, 50),
        pct(&v, 95),
        v[v.len() - 1],
        mean
    );
}

fn main() {
    let endpoint =
        std::env::var("BENCH_ENDPOINT").expect("set BENCH_ENDPOINT to the local sequencer RPC URL");
    let registry = hex32(&std::env::var("BENCH_REGISTRY").expect("set BENCH_REGISTRY (64 hex)"));
    let n: u64 = env_or("BENCH_N", 500);
    let lookups: usize = env_or("BENCH_LOOKUPS", 500);
    let walks: usize = env_or("BENCH_WALKS", 5);

    let client = Client::new(endpoint.clone(), registry);

    println!("registry {}", base58(&registry));
    println!("endpoint {endpoint}");

    // 0. What the chain thinks it holds. Everything below is only meaningful if
    //    this is genuinely ≥ 500.
    let total = match client.global() {
        Ok(Some(g)) => g.total_entries,
        Ok(None) => {
            eprintln!("global state not found — is the registry initialised on this endpoint?");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("could not read global state: {e}");
            std::process::exit(1);
        }
    };
    println!("total_entries on chain: {total}");
    if total < 500 {
        eprintln!("WARNING: fewer than 500 entries on chain — the scale claim is not met");
    }

    // 1. Derivation alone: the entire "index" this registry has for resolution.
    let d = 500_000u32;
    let start = Instant::now();
    let mut sink = 0u8;
    for i in 0..d {
        sink ^= entry_address(&registry, &pid(u64::from(i)))[0];
    }
    let elapsed = start.elapsed();
    println!("\nderivation");
    println!(
        "  {d} addresses in {:?} — {:.3} µs each (checksum {sink})",
        elapsed,
        elapsed.as_secs_f64() * 1e6 / f64::from(d)
    );

    // 2. Single-entry lookups, hits and misses side by side. A hit reads an
    //    address the populate script wrote to; a miss reads one nothing did. If
    //    resolution cost depended on how full the registry is, these would part.
    let hit_pop = n.max(1);
    let mut hits = Vec::new();
    let mut misses = Vec::new();
    let mut resolved = 0usize;
    println!("\n{lookups} single-entry lookups against {total} on-chain entries");
    for i in 0..lookups {
        let hit = i % 2 == 0;
        let program = if hit {
            pid((i as u64 / 2) % hit_pop)
        } else {
            miss_pid(i as u64)
        };
        let t = Instant::now();
        let got = client.entry(&program);
        let us = t.elapsed().as_micros();
        match got {
            Ok(Some(_)) => {
                resolved += 1;
                hits.push(us);
            }
            Ok(None) => {
                if hit {
                    eprintln!("  warning: a registered program did not resolve (i={i})");
                }
                misses.push(us);
            }
            Err(e) => eprintln!("  lookup {i} failed: {e}"),
        }
    }
    report("registered (hit)", hits);
    report("unregistered (miss)", misses);
    println!("  resolved {resolved} of {lookups}");

    // 3. Full-registry enumeration — the walk the explorer and indexer run. This
    //    is what "search across ≥500 entries" costs: two reads per entry, linear
    //    and openly so. Repeated so the number is a distribution, not one sample.
    println!("\nfull-registry walk ({walks}×), the enumeration search runs on top of");
    let mut walk_ms = Vec::new();
    let mut walked = 0usize;
    for _ in 0..walks {
        let t = Instant::now();
        match client.walk() {
            Ok(entries) => {
                walked = entries.len();
                walk_ms.push(t.elapsed().as_millis());
            }
            Err(e) => eprintln!("  walk failed: {e}"),
        }
    }
    if !walk_ms.is_empty() {
        walk_ms.sort_unstable();
        let mean = walk_ms.iter().sum::<u128>() / walk_ms.len() as u128;
        println!(
            "  walked {walked} entries — p50 {}ms  max {}ms  mean {}ms  ({} reads per walk)",
            pct(&walk_ms, 50),
            walk_ms[walk_ms.len() - 1],
            mean,
            1 + 2 * walked, // one global read + two per entry
        );
    }

    // 4. Search proper: one walk, then the same predicate the CLI/explorer apply.
    //    The filter is in-memory over entries already fetched, so its cost is
    //    noise beside the walk — which is the point of reporting the walk.
    let t = Instant::now();
    let entries = client.walk().unwrap_or_default();
    let walk_for_search = t.elapsed();
    for (needle, why) in [
        ("bench-prog", "every synthetic entry"),
        ("bench-prog-0042", "one exact entry"),
        ("bench-prog-01", "a ten-entry prefix"),
    ] {
        let t = Instant::now();
        let hits = entries
            .iter()
            .filter(|e| {
                e.name.to_lowercase().contains(needle)
                    || e.description.to_lowercase().contains(needle)
            })
            .count();
        println!(
            "\nsearch {needle:<18} {hits:>3} hits — filter {:?}, walk {:?} ({why})",
            t.elapsed(),
            walk_for_search
        );
    }
}
