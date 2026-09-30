// SPDX-License-Identifier: MIT OR Apache-2.0
//! Read a live v0.3 registry and check what it holds agrees with itself.
//!
//! Ignored by default: they need a sequencer. Point them at one with
//! `LEZ_RPC` and `PROVENANCE_REGISTRY` (both set by `scripts/e2e-v03-local.sh`
//! for its local chain), and run `cargo test -- --ignored`. Without both
//! variables they say so and pass, so the default e2e job reads as "not
//! configured" rather than as a green run against nothing.

use lez_registry_sdk::{parse_account, Client, Publisher};

fn client() -> Option<Client> {
    let rpc = std::env::var("LEZ_RPC").ok()?;
    let reg = parse_account(&std::env::var("PROVENANCE_REGISTRY").ok()?)?;
    Some(Client::new(rpc, reg))
}

#[test]
#[ignore = "talks to a real sequencer"]
fn the_registry_is_registered_in_itself_by_its_deployer() {
    let Some(c) = client() else {
        eprintln!("skipped: set LEZ_RPC and PROVENANCE_REGISTRY");
        return;
    };
    let reg = *c.registry();
    let e = c
        .entry(&reg)
        .expect("the sequencer answered")
        .expect("the registry has a deployer entry for its own program account");
    assert_eq!(e.program_id, reg);
    assert_eq!(e.publisher, Publisher::Canonical);
    assert_eq!(e.author, reg, "a deployer entry is signed by the header account");
    let h = c.program_header(&reg).unwrap().expect("the registry is deployed");
    assert_eq!(e.image_id, h.image_id, "the entry records the header's ImageID");
}

#[test]
#[ignore = "talks to a real sequencer"]
fn the_walk_visits_exactly_the_counter() {
    let Some(c) = client() else {
        eprintln!("skipped: set LEZ_RPC and PROVENANCE_REGISTRY");
        return;
    };
    let total = c.global().unwrap().expect("initialized").total_entries;
    let entries = c.walk().unwrap();
    assert_eq!(entries.len() as u64, total, "every slot resolves to an entry");
    for e in &entries {
        // Every entry points at a real program.
        assert!(
            c.program_header(&e.program_id).unwrap().is_some(),
            "{} names an account that holds no program",
            e.name
        );
    }
}
