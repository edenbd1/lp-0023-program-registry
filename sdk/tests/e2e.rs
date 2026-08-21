// SPDX-License-Identifier: MIT OR Apache-2.0
//! End-to-end against a real sequencer.
//!
//! These read the live registry over JSON-RPC and assert on what comes back, so
//! they fail when the chain, the derivation or the encoding stops agreeing —
//! which is the whole class of bug the unit tests structurally cannot catch. A
//! wrong PDA derivation, for instance, returns a perfectly well-formed address
//! that simply has nothing at it, and only a real read notices.
//!
//! They are ignored by default and run with `--ignored`, because a network test
//! in the default set makes `cargo test` fail on a train. CI runs them
//! explicitly, so "skipped" is not available as an outcome there.
//!
//! Point them elsewhere with `LEZ_ENDPOINT` and `LEZ_REGISTRY`.

use lez_registry_sdk::{base58, entry_address, Client, ProgramId};

fn hex32(s: &str) -> ProgramId {
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).expect("valid hex");
    }
    out
}

fn registry() -> ProgramId {
    hex32(&std::env::var("LEZ_REGISTRY").unwrap_or_else(|_| {
        "17c7ec55940d057d2ad1a95b0ede97d29ede1d310dcd35de9e09df2e214c0c30".into()
    }))
}

fn client() -> Client {
    Client::new(
        std::env::var("LEZ_ENDPOINT").unwrap_or_else(|_| "https://testnet.lez.logos.co".into()),
        registry(),
    )
}

#[test]
#[ignore = "talks to a real sequencer"]
fn the_registry_is_registered_in_itself() {
    let reg = registry();
    let e = client()
        .entry(&reg)
        .expect("the sequencer answered")
        .expect("the registry has an entry for its own program id");

    assert_eq!(e.program_id, reg, "the entry is about this program");
    assert_eq!(e.name, "lez-program-registry");
    assert!(
        matches!(e.publisher, lez_registry_sdk::Publisher::Canonical),
        "its own entry is the canonical one"
    );
}

#[test]
#[ignore = "talks to a real sequencer"]
fn the_derivation_still_agrees_with_the_runtime() {
    // The failure this catches: a derivation that drifts returns a well-formed
    // address holding nothing, which reads as "not registered" rather than as an
    // error. Only comparing against a chain that has something there notices.
    let reg = registry();
    let addr = entry_address(&reg, &reg);
    let c = client();
    assert!(
        c.entry(&reg).expect("answered").is_some(),
        "nothing at {} — the derivation has drifted from the runtime's",
        base58(&addr)
    );
}

#[test]
#[ignore = "talks to a real sequencer"]
fn the_counters_agree_with_what_the_walk_finds() {
    let c = client();
    let g = c.global().expect("answered").expect("global exists");
    let entries = c.walk().expect("the walk completed");

    assert_eq!(
        entries.len() as u64,
        g.total_entries,
        "the directory and the counter disagree, so one of them is lying"
    );
    let verified = entries
        .iter()
        .filter(|e| e.verification.is_verified())
        .count() as u64;
    assert_eq!(
        verified, g.total_verified,
        "total_verified does not match the entries that are actually verified"
    );
}

#[test]
#[ignore = "talks to a real sequencer"]
fn every_verification_state_is_reachable_and_distinct() {
    // The prize requires four distinct states and calls collapsing any of them a
    // failure. Asserting they are all present on chain is stronger than
    // asserting the enum has four variants.
    use lez_registry_sdk::VerificationState::*;
    let entries = client().walk().expect("the walk completed");
    for wanted in [Unclaimed, Claimed, Verified, Mismatched] {
        assert!(
            entries.iter().any(|e| e.verification.state == wanted),
            "no entry is in state {wanted:?} — the state is unreachable in practice"
        );
    }
}

#[test]
#[ignore = "talks to a real sequencer"]
fn an_unregistered_program_resolves_to_nothing_rather_than_erroring() {
    // A derived address always exists as an address. What separates registered
    // from unregistered is whether anything was written there, and a client that
    // treated the second as an error would be unusable.
    let nowhere = [0x5Au8; 32];
    assert!(client().entry(&nowhere).expect("answered").is_none());
}
