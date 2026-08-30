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

use lez_registry_sdk::{
    base58, build_register, build_update, entry_address, Client, ProgramId, Publisher,
    RegisterRequest, Role, UpdateRequest,
};

fn hex32(s: &str) -> ProgramId {
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).expect("valid hex");
    }
    out
}

fn registry() -> ProgramId {
    hex32(&std::env::var("LEZ_REGISTRY").unwrap_or_else(|_| {
        "f59751e932e856f7565f34e79ae726791794dc68e55f2413bc0375008cc07bf8".into()
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
fn a_built_register_targets_the_address_the_chain_already_holds() {
    // The strongest check available without submitting: build the register the
    // registry's own entry would have come from, and assert its entry account is
    // the address the live chain actually has an entry at. If the builder's
    // account derivation drifted from the runtime's, this lands on an empty
    // address and the assertion below fails.
    let reg = registry();
    let c = client();
    let g = c.global().expect("answered").expect("global exists");

    let tx = build_register(
        &reg,
        &RegisterRequest {
            program: &reg,
            author: &[0u8; 32],
            publisher: Publisher::Canonical,
            name: "lez-program-registry",
            version: "0.7.0",
            description: "The registry itself, registered in itself.",
            clock: &[0u8; 32],
            seq_index: g.total_entries,
        },
    )
    .expect("valid metadata");

    assert_eq!(
        tx.entry_address(),
        entry_address(&reg, &reg),
        "the built entry account is the canonical entry address"
    );
    assert!(
        c.account_data(&tx.entry_address())
            .expect("answered")
            .is_some(),
        "nothing at {} — the builder's derivation has drifted from the runtime's",
        base58(&tx.entry_address())
    );
    // The declared account order the program destructures: entry, global,
    // directory, clock, author. Assert the two derivable ones agree with the
    // read path's own derivations, live.
    assert_eq!(tx.accounts[0].role, Role::Entry);
    assert_eq!(tx.accounts[1].role, Role::Global);
    assert_eq!(
        tx.accounts[1].id,
        lez_registry_sdk::global_address(&reg),
        "the global slot is the singleton the chain answers for"
    );
}

#[test]
#[ignore = "talks to a real sequencer"]
fn a_built_update_is_author_gated_against_the_live_entry() {
    // update is the recorded author's alone. The builder cannot enforce that
    // offline, but the read path can: build an update for the registry's own
    // entry signed by a stranger, and confirm the on-chain author disagrees —
    // which is exactly the check a caller runs before submitting a doomed tx.
    let reg = registry();
    let c = client();
    let live = c
        .entry(&reg)
        .expect("answered")
        .expect("the registry has its own entry");

    let stranger = [0x5Au8; 32];
    let tx = build_update(
        &reg,
        &UpdateRequest {
            program: &reg,
            author: &stranger,
            publisher: Publisher::Canonical,
            version: "0.7.1",
            description: "an update a stranger is not entitled to make",
            idl: "",
        },
    )
    .expect("valid metadata");

    assert_eq!(
        tx.entry_address(),
        entry_address(&reg, &reg),
        "update touches the same entry resolve returns"
    );
    assert_ne!(
        live.author, stranger,
        "the recorded author is not the stranger, so the program would reject this update"
    );
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
