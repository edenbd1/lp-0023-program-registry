// Measures the zkVM cycle cost of every registry operation, inside the guest.
//
// Fixtures are compiled in rather than read from the host, so a run is
// reproducible from the ELF alone: the same guest always measures the same work.
// Each figure is a `cycle_count()` delta with the measurement's own overhead
// subtracted, and `black_box` stops the optimiser hoisting a pure function out
// of the region being timed.
//
// The number these are compared against is LEZ's 32,000,000-cycle cap on public
// execution. A registry whose operations were anywhere near it would be a
// registry nobody could afford to use.

use borsh::{BorshDeserialize, BorshSerialize};
use core::hint::black_box;
use risc0_zkvm::guest::env;

use lez_registry_core::{
    deployment_digest, entry_seed, global_seed, seq_seed, third_party_entry_seed,
    BuildEnvironment, Entry, Publisher, SourceClaim, Verification,
};

/// One measured region. Returns net cycles for `f`.
fn timed<T>(baseline: u64, mut f: impl FnMut() -> T) -> u64 {
    let a = env::cycle_count();
    black_box(f());
    let b = env::cycle_count();
    (b - a).saturating_sub(baseline)
}

fn stats(v: &[u64]) -> (u64, u64, u64) {
    let mut s = v.to_vec();
    s.sort_unstable();
    (s[s.len() / 2], s[0], s[s.len() - 1])
}

fn sample_claim() -> SourceClaim {
    SourceClaim {
        repository: "https://github.com/edenbd1/lez-program-registry".into(),
        commit: [0x8d; 20],
        source_digest: [0xae; 32],
        build: BuildEnvironment {
            image_digest: [0x3e; 32],
            toolchain: "1.88.0".into(),
            network_disabled: true,
        },
    }
}

fn sample_entry() -> Entry {
    let mut e = Entry::new(
        [0xe1; 32],
        "lez-program-registry".into(),
        "0.5.0".into(),
        [0xbf; 32],
        Publisher::Canonical,
    )
    .expect("fixture is valid");
    e.description = "Program registry with sealed-build verification.".into();
    e.tags = vec!["registry".into(), "verification".into()];
    e
}

fn main() {
    let baseline = {
        let a = env::cycle_count();
        black_box(0u128);
        let b = env::cycle_count();
        b - a
    };

    let mut out: Vec<(&str, u64, u64, u64, usize)> = Vec::new();

    // Address derivation — the whole of this registry's "index".
    let pids: [[u8; 32]; 8] = [
        [0x00; 32], [0x11; 32], [0x22; 32], [0x33; 32],
        [0xe1; 32], [0x49; 32], [0x51; 32], [0xff; 32],
    ];
    let v: Vec<u64> = pids
        .iter()
        .map(|p| timed(baseline, || entry_seed(black_box(p))))
        .collect();
    let (m, lo, hi) = stats(&v);
    out.push(("entry_seed", m, lo, hi, v.len()));

    let author = [0xbf; 32];
    let v: Vec<u64> = pids
        .iter()
        .map(|p| timed(baseline, || third_party_entry_seed(black_box(p), black_box(&author))))
        .collect();
    let (m, lo, hi) = stats(&v);
    out.push(("third_party_entry_seed", m, lo, hi, v.len()));

    let v: Vec<u64> = (0u64..8)
        .map(|n| timed(baseline, || seq_seed(black_box(n))))
        .collect();
    let (m, lo, hi) = stats(&v);
    out.push(("seq_seed", m, lo, hi, v.len()));

    let v = vec![timed(baseline, global_seed)];
    let (m, lo, hi) = stats(&v);
    out.push(("global_seed", m, lo, hi, v.len()));

    // The verification state machine.
    let v: Vec<u64> = (0..8)
        .map(|_| timed(baseline, || Verification::claim(black_box(sample_claim()))))
        .collect();
    let (m, lo, hi) = stats(&v);
    out.push(("Verification::claim", m, lo, hi, v.len()));

    let subject = [0xe1; 32];
    let attester = [0xaa; 32];
    let v: Vec<u64> = (0..8)
        .map(|_| {
            let mut ver = Verification::claim(sample_claim()).expect("valid");
            timed(baseline, || {
                ver.record_rebuild(black_box(subject), black_box(subject), black_box(attester))
            })
        })
        .collect();
    let (m, lo, hi) = stats(&v);
    out.push(("record_rebuild (accepts)", m, lo, hi, v.len()));

    let v: Vec<u64> = (0..8)
        .map(|_| {
            let mut c = sample_claim();
            c.build.network_disabled = false;
            let mut ver = Verification::claim(c).expect("valid");
            timed(baseline, || {
                ver.record_rebuild(black_box(subject), black_box(subject), black_box(attester))
            })
        })
        .collect();
    let (m, lo, hi) = stats(&v);
    out.push(("record_rebuild (refuses)", m, lo, hi, v.len()));

    // Entry handling, which every instruction pays.
    let e = sample_entry();
    let v: Vec<u64> = (0..8)
        .map(|_| timed(baseline, || e.validate()))
        .collect();
    let (m, lo, hi) = stats(&v);
    out.push(("Entry::validate", m, lo, hi, v.len()));

    let v: Vec<u64> = (0..8)
        .map(|_| timed(baseline, || borsh::to_vec(black_box(&e))))
        .collect();
    let (m, lo, hi) = stats(&v);
    out.push(("Entry serialise", m, lo, hi, v.len()));

    let bytes = borsh::to_vec(&e).expect("fixture serialises");
    let v: Vec<u64> = (0..8)
        .map(|_| timed(baseline, || Entry::try_from_slice(black_box(&bytes))))
        .collect();
    let (m, lo, hi) = stats(&v);
    out.push(("Entry deserialise", m, lo, hi, v.len()));

    // The digest that checks a local file against a deployment. Sized at 4 KiB
    // and 64 KiB because it scales with the artifact, unlike everything above.
    let small = vec![0xABu8; 4096];
    let v: Vec<u64> = (0..4)
        .map(|_| timed(baseline, || deployment_digest(black_box(&small))))
        .collect();
    let (m, lo, hi) = stats(&v);
    out.push(("deployment_digest 4KiB", m, lo, hi, v.len()));

    let big = vec![0xABu8; 65536];
    let v: Vec<u64> = (0..4)
        .map(|_| timed(baseline, || deployment_digest(black_box(&big))))
        .collect();
    let (m, lo, hi) = stats(&v);
    out.push(("deployment_digest 64KiB", m, lo, hi, v.len()));

    // What one `register` actually costs: derive, build, validate, store.
    let whole_register = timed(baseline, || {
        let seed = entry_seed(black_box(&subject));
        let mut e = Entry::new(
            black_box(subject),
            "lez-program-registry".into(),
            "0.5.0".into(),
            author,
            Publisher::Canonical,
        )
        .expect("valid");
        e.description = "Program registry with sealed-build verification.".into();
        let _ = e.validate();
        let bytes = borsh::to_vec(&e).expect("serialises");
        (seed, bytes.len())
    });

    env::commit(&(out, baseline, whole_register));
}
