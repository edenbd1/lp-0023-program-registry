// SPDX-License-Identifier: MIT OR Apache-2.0
//! Measure what it costs to resolve a program in this registry.
//!
//! The question behind "lookup latency at scale" is whether resolution degrades
//! as the registry fills up. Here it cannot, and the reason is structural rather
//! than measured: resolving is one SHA-256 and one account read. There is no
//! scan, no index lookup, no list to walk. The code path does not contain the
//! registry's size.
//!
//! What is measured is therefore the honest thing: 500 real lookups against the
//! public testnet, split into addresses that hold an entry and addresses that do
//! not. If resolution depended on what the registry holds, those two populations
//! would separate. Reported side by side so you can see whether they do.
//!
//! Browsing is a different operation with a different cost, and this reports that
//! too: `walk()` is linear in the number of entries by definition, which is
//! exactly why it belongs in an optional indexer and not in the resolve path.

use lez_registry_sdk::{entry_address, Client, ProgramId};
use std::time::Instant;

const ENDPOINT: &str = "https://testnet.lez.logos.co";
const REGISTRY: ProgramId = [
    0xe1, 0x48, 0x4f, 0x14, 0x47, 0xed, 0xd7, 0x66, 0x9d, 0x14, 0x43, 0x15, 0x48, 0x41, 0x16, 0x0f,
    0x4d, 0xdf, 0xe4, 0xc5, 0xd9, 0x3d, 0x41, 0x05, 0x27, 0xe5, 0xcf, 0x99, 0xab, 0x9a, 0xac, 0xba,
];
const LOOKUPS: usize = 500;

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
        "  {label:<22} n={:<4} p50 {:>4}ms  p95 {:>4}ms  max {:>4}ms  mean {:>4}ms",
        v.len(),
        pct(&v, 50),
        pct(&v, 95),
        v[v.len() - 1],
        mean
    );
}

fn main() {
    let client = Client::new(ENDPOINT, REGISTRY);

    // 1. Derivation alone. This is the entire "index" this registry has.
    let n = 500_000;
    let start = Instant::now();
    let mut sink = 0u8;
    for i in 0..n {
        let mut pid = [0u8; 32];
        pid[..8].copy_from_slice(&(i as u64).to_le_bytes());
        sink ^= entry_address(&REGISTRY, &pid)[0];
    }
    let elapsed = start.elapsed();
    println!("derivation");
    println!(
        "  {n} addresses in {:?} — {:.2} µs each (checksum {sink})",
        elapsed,
        elapsed.as_secs_f64() * 1e6 / f64::from(n)
    );

    // 2. Real lookups. The registered ones are read repeatedly; the rest are
    //    addresses nothing was ever written to. Both are full round trips.
    let registered: Vec<ProgramId> = vec![
        REGISTRY,
        hex32("49db0fc91883668a5ccb85242aa1cdf923a557a7327f7c0613ad526fa56fc510"),
        hex32("51f28557602ae6daff97e51127ba6dbc5ddfe6048ea7cd89ca5e170ab6c7a82d"),
        hex32("4c6e62a585934f87ef919c63a252fb33faef14d9ed1af2bd17da62deaf93ea7f"),
    ];

    let mut hits = Vec::new();
    let mut misses = Vec::new();
    let mut found = 0usize;
    println!("\n{LOOKUPS} live lookups against {ENDPOINT}");
    for i in 0..LOOKUPS {
        let (pid, is_registered) = if i % 4 == 0 {
            (registered[(i / 4) % registered.len()], true)
        } else {
            let mut p = [0u8; 32];
            p[..8].copy_from_slice(&(i as u64).to_le_bytes());
            p[8] = 0xAB;
            (p, false)
        };
        let t = Instant::now();
        let got = client.entry(&pid);
        let ms = t.elapsed().as_millis();
        match got {
            Ok(Some(_)) => {
                found += 1;
                hits.push(ms);
            }
            Ok(None) => {
                if is_registered {
                    eprintln!("  warning: a registered program did not resolve");
                }
                misses.push(ms);
            }
            Err(e) => eprintln!("  lookup {i} failed: {e}"),
        }
    }
    report("registered", hits);
    report("never registered", misses);
    println!("  resolved {found} of {LOOKUPS}");

    // 3. Browsing, for contrast. Linear, and openly so.
    let t = Instant::now();
    match client.walk() {
        Ok(entries) => println!(
            "\nwalk: {} entries in {:?} — linear, which is why it lives in the indexer",
            entries.len(),
            t.elapsed()
        ),
        Err(e) => eprintln!("\nwalk failed: {e}"),
    }
}

fn hex32(s: &str) -> ProgramId {
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).expect("valid hex");
    }
    out
}
