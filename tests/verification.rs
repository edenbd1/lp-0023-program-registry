// SPDX-License-Identifier: MIT OR Apache-2.0
//! Every assertion here corresponds to a claim the registry makes about what
//! "verified" means. A claim without a test is a claim we should not print.

use lez_registry_core::*;

fn env(network_disabled: bool) -> BuildEnvironment {
    BuildEnvironment {
        image_digest: [7u8; 32],
        toolchain: "1.88.0".into(),
        network_disabled,
    }
}

fn claim_for(artifact: ProgramId, network_disabled: bool) -> SourceClaim {
    SourceClaim {
        repository: "https://github.com/example/prog".into(),
        commit: [1u8; 20],
        source_digest: [2u8; 32],
        build: env(network_disabled),
        artifact,
    }
}

#[test]
fn a_claim_is_not_a_verification() {
    // Typing a URL must not mint trust. Anyone can assert anything about a
    // program; the registry records the assertion and says it is only that.
    let v = Verification::claim(claim_for([9u8; 32], true)).unwrap();
    assert_eq!(v.state, VerificationState::Claimed);
    assert!(!v.is_verified());
}

#[test]
fn a_networked_rebuild_can_never_be_verified() {
    // This is the Solana hole: a build.rs that downloads the deployed binary
    // makes the rebuild match trivially while proving nothing about the source.
    // Offline is therefore not a nicety, it is the whole mechanism.
    let artifact = [3u8; 32];
    let mut v = Verification::claim(claim_for(artifact, false)).unwrap();
    let err = v.record_rebuild(artifact, artifact).unwrap_err();
    assert_eq!(err, RegistryError::NetworkedBuild);
    assert_eq!(v.state, VerificationState::Unreproducible);
    assert!(!v.is_verified());
}

#[test]
fn an_offline_rebuild_that_matches_is_verified() {
    let artifact = [3u8; 32];
    let mut v = Verification::claim(claim_for(artifact, true)).unwrap();
    v.record_rebuild(artifact, artifact).unwrap();
    assert_eq!(v.state, VerificationState::Verified);
    assert!(v.is_verified());
}

#[test]
fn a_rebuild_that_does_not_match_is_refused_rather_than_recorded() {
    let artifact = [3u8; 32];
    let mut v = Verification::claim(claim_for(artifact, true)).unwrap();
    assert_eq!(
        v.record_rebuild([4u8; 32], artifact).unwrap_err(),
        RegistryError::ArtifactMismatch
    );
    // And the state did not move: a failed rebuild leaves the claim a claim.
    assert_eq!(v.state, VerificationState::Claimed);
}

#[test]
fn a_claim_about_another_artifact_is_stale_not_failed() {
    // The distinction matters on screen. "Failed" reads as "someone tried to
    // cheat"; "stale" reads as "this was true of an older build", which is what
    // an upgraded program actually is.
    let deployed = [5u8; 32];
    let old = [4u8; 32];
    let mut v = Verification::claim(claim_for(old, true)).unwrap();
    v.record_rebuild(old, deployed).unwrap();
    assert_eq!(
        v.state,
        VerificationState::Stale {
            verified_artifact: old
        }
    );
    assert!(!v.is_verified(), "stale must not read as verified");
}

#[test]
fn an_upgrade_cannot_inherit_the_previous_verification() {
    // Verification is version-bound. Without this, an author could verify a
    // benign build, upgrade to anything, and keep the badge.
    let first = [1u8; 32];
    let mut v = Verification::claim(claim_for(first, true)).unwrap();
    v.record_rebuild(first, first).unwrap();
    assert!(v.is_verified());

    v.on_artifact_changed([2u8; 32]);
    assert_eq!(
        v.state,
        VerificationState::Stale {
            verified_artifact: first
        },
        "the new artifact must not be verified, and the old one must be named"
    );
    assert!(!v.is_verified());
}

#[test]
fn an_upgrade_drops_a_claim_that_was_about_the_old_artifact() {
    let first = [1u8; 32];
    let mut v = Verification::claim(claim_for(first, true)).unwrap();
    v.on_artifact_changed([2u8; 32]);
    assert_eq!(v.state, VerificationState::Unclaimed);
    assert!(v.claim.is_none(), "a claim about a dead artifact is noise");
}

#[test]
fn an_upgrade_keeps_a_claim_that_already_names_the_new_artifact() {
    // An author who claims the source of the build they are about to deploy
    // should not have that claim erased by the deployment.
    let next = [2u8; 32];
    let mut v = Verification::claim(claim_for(next, true)).unwrap();
    v.on_artifact_changed(next);
    assert_eq!(v.state, VerificationState::Claimed);
    assert!(v.claim.is_some());
}

#[test]
fn recording_a_rebuild_without_a_claim_is_refused() {
    let mut v = Verification::unclaimed();
    assert_eq!(
        v.record_rebuild([1u8; 32], [1u8; 32]).unwrap_err(),
        RegistryError::Empty
    );
}

#[test]
fn only_the_author_may_write_and_freezing_is_one_way() {
    let author = [1u8; 32];
    let stranger = [2u8; 32];
    let mut e = Entry::new(
        [9u8; 32],
        "counter".into(),
        "0.1.0".into(),
        author,
        Publisher::Canonical,
    )
    .unwrap();

    assert_eq!(
        e.require_writable_by(&stranger).unwrap_err(),
        RegistryError::NotAuthor
    );
    e.require_writable_by(&author).unwrap();

    e.freeze(&author).unwrap();
    assert!(e.frozen);
    // Frozen beats authorship: even the author is refused afterwards, and there
    // is no call anywhere that lowers the flag.
    assert_eq!(
        e.require_writable_by(&author).unwrap_err(),
        RegistryError::Immutable
    );
    assert_eq!(e.freeze(&author).unwrap_err(), RegistryError::Immutable);
}

#[test]
fn a_stranger_can_never_occupy_the_canonical_address() {
    // The canonical seed is a function of the program id alone, so there is
    // exactly one, and third-party entries are keyed by their publisher. A
    // stranger publishing about a program lands somewhere else by construction
    // rather than by a check that could be forgotten.
    let prog = [4u8; 32];
    let canonical = entry_seed(&prog);
    let a = third_party_entry_seed(&prog, &[1u8; 32]);
    let b = third_party_entry_seed(&prog, &[2u8; 32]);
    assert_ne!(canonical, a);
    assert_ne!(canonical, b);
    assert_ne!(a, b, "two third parties must not share an address");
}

#[test]
fn the_seeds_are_domain_separated() {
    // Without distinct prefixes, a global-state address and an entry address
    // are both "a hash of 32 bytes" and could in principle collide by
    // construction rather than by luck.
    let prog = [0u8; 32];
    assert_ne!(entry_seed(&prog), global_seed());
    assert_ne!(third_party_entry_seed(&prog, &[0u8; 32]), global_seed());
    // And the entry seed is a function of its input, not a constant.
    assert_ne!(entry_seed(&[0u8; 32]), entry_seed(&[1u8; 32]));
}

#[test]
fn the_program_id_is_lez_content_addressing_not_a_plain_hash() {
    // The registry's notion of "which bytecode" must be the chain's, or a
    // verified entry would be verified against a number the chain never uses.
    // LEZ hashes the little-endian length before the bytes, so two inputs that
    // differ only by framing get different ids.
    let a = program_id_of(b"\x00\x01");
    let b = program_id_of(b"\x00\x01\x00");
    assert_ne!(a, b);
    assert_ne!(a, [0u8; 32]);
    // Deterministic, which is the entire premise of a reproducible build.
    assert_eq!(a, program_id_of(b"\x00\x01"));
}

#[test]
fn entries_refuse_the_shapes_that_would_not_fit_their_account() {
    let author = [1u8; 32];
    assert_eq!(
        Entry::new(
            [1u8; 32],
            "".into(),
            "1".into(),
            author,
            Publisher::Canonical
        )
        .unwrap_err(),
        RegistryError::Empty
    );
    assert_eq!(
        Entry::new(
            [1u8; 32],
            "n".into(),
            "".into(),
            author,
            Publisher::Canonical
        )
        .unwrap_err(),
        RegistryError::Empty
    );
    let long = "x".repeat(MAX_NAME + 1);
    assert_eq!(
        Entry::new([1u8; 32], long, "1".into(), author, Publisher::Canonical).unwrap_err(),
        RegistryError::TooLong
    );

    let mut e = Entry::new(
        [1u8; 32],
        "n".into(),
        "1".into(),
        author,
        Publisher::Canonical,
    )
    .unwrap();
    e.tags = (0..MAX_TAGS + 1).map(alloc_tag).collect();
    assert_eq!(e.validate().unwrap_err(), RegistryError::TooLong);
    e.tags = vec!["".into()];
    assert_eq!(e.validate().unwrap_err(), RegistryError::TooLong);
}

fn alloc_tag(i: usize) -> String {
    format!("t{i}")
}

#[test]
fn an_empty_toolchain_is_refused_because_a_build_needs_one_to_be_reproducible() {
    let mut c = claim_for([1u8; 32], true);
    c.build.toolchain = String::new();
    assert_eq!(Verification::claim(c).unwrap_err(), RegistryError::Empty);
}

#[test]
fn a_claim_with_no_repository_is_refused() {
    let mut c = claim_for([1u8; 32], true);
    c.repository = String::new();
    assert_eq!(Verification::claim(c).unwrap_err(), RegistryError::Empty);
}
