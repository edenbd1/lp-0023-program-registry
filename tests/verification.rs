// SPDX-License-Identifier: MIT OR Apache-2.0
//! Every assertion here corresponds to a claim the registry makes about what
//! "verified" means. A claim without a test is a claim we should not print.

use lez_registry_core::*;

/// A third party doing the rebuild. Distinct from AUTHOR so that any test which
/// accidentally conflates the two fails rather than passes quietly.
const ATTESTER: AccountId = [0xAA; 32];
const AUTHOR: AccountId = [0xBB; 32];

fn env(network_disabled: bool) -> BuildEnvironment {
    BuildEnvironment {
        image_digest: [7u8; 32],
        toolchain: "1.88.0".into(),
        network_disabled,
    }
}

fn claim_for(network_disabled: bool) -> SourceClaim {
    SourceClaim {
        repository: "https://github.com/example/prog".into(),
        commit: [1u8; 20],
        source_digest: [2u8; 32],
        build: env(network_disabled),
    }
}

#[test]
fn a_claim_is_not_a_verification() {
    // Typing a URL must not mint trust. Anyone can assert anything about a
    // program; the registry records the assertion and says it is only that.
    let v = Verification::claim(claim_for(true)).unwrap();
    assert_eq!(v.state, VerificationState::Claimed);
    assert!(!v.is_verified());
}

#[test]
fn a_networked_rebuild_can_never_be_verified() {
    // This is the Solana hole: a build.rs that downloads the deployed binary
    // makes the rebuild match trivially while proving nothing about the source.
    // Offline is therefore not a nicety, it is the whole mechanism.
    let artifact = [3u8; 32];
    let mut v = Verification::claim(claim_for(false)).unwrap();
    let err = v.record_rebuild(artifact, artifact, ATTESTER).unwrap_err();
    assert_eq!(err, RegistryError::NetworkedBuild);
    assert_eq!(v.state, VerificationState::Unreproducible);
    assert!(!v.is_verified());
}

#[test]
fn an_offline_rebuild_that_matches_is_verified() {
    let artifact = [3u8; 32];
    let mut v = Verification::claim(claim_for(true)).unwrap();
    v.record_rebuild(artifact, artifact, ATTESTER).unwrap();
    assert_eq!(v.state, VerificationState::Verified);
    assert!(v.is_verified());
}

#[test]
fn a_rebuild_that_does_not_match_is_refused_rather_than_recorded() {
    let artifact = [3u8; 32];
    let mut v = Verification::claim(claim_for(true)).unwrap();
    assert_eq!(
        v.record_rebuild([4u8; 32], artifact, ATTESTER).unwrap_err(),
        RegistryError::ArtifactMismatch
    );
    // And the state did not move: a failed rebuild leaves the claim a claim.
    assert_eq!(v.state, VerificationState::Claimed);
}

#[test]
fn a_verification_cannot_go_stale_because_the_subject_is_the_key() {
    // The state a registry for an account-addressed chain needs most is the one
    // this type cannot express. There, code at an address can be replaced under
    // an honestly earned verification, so "verified" has to decay. Here the
    // entry is keyed by the ImageID, so the only way to have different bytecode
    // is to be a different entry, and a verification stays true.
    let subject = [1u8; 32];
    let mut v = Verification::claim(claim_for(true)).unwrap();
    v.record_rebuild(subject, subject, ATTESTER).unwrap();
    assert!(v.is_verified());

    // There is no call that takes this entry to any other artifact. The nearest
    // thing to an upgrade is a rebuild claiming different bytecode, and that is
    // a mismatch -- refused, and it does not disturb what was already proven.
    assert_eq!(
        v.record_rebuild([2u8; 32], subject, ATTESTER).unwrap_err(),
        RegistryError::ArtifactMismatch
    );
    assert!(
        v.is_verified(),
        "a refused rebuild must not revoke a verification that was earned"
    );
}

#[test]
fn an_upgrade_is_a_different_entry_and_inherits_nothing() {
    // Version binding is real, it just lives in the addressing rather than in a
    // state transition: two builds are two keys, hence two entries, and the
    // second starts where every entry starts.
    let v1 = [1u8; 32];
    let v2 = [2u8; 32];
    assert_ne!(
        entry_seed(&v1),
        entry_seed(&v2),
        "two artifacts must not share an entry address"
    );

    let mut first = Verification::claim(claim_for(true)).unwrap();
    first.record_rebuild(v1, v1, ATTESTER).unwrap();
    assert!(first.is_verified());

    let second = Verification::unclaimed();
    assert_eq!(second.state, VerificationState::Unclaimed);
    assert!(
        !second.is_verified(),
        "a new build must not inherit the badge of the one it replaces"
    );
}

#[test]
fn recording_a_rebuild_without_a_claim_is_refused() {
    let mut v = Verification::unclaimed();
    assert_eq!(
        v.record_rebuild([1u8; 32], [1u8; 32], ATTESTER)
            .unwrap_err(),
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
fn the_deployment_digest_is_lez_framing_not_a_plain_hash() {
    // Found by running the CLI against a real deployment rather than by reading
    // the code: this function's output on lez_registry.bin is the *deploy
    // transaction hash*, while the program is called by its RISC0 ImageID,
    // which is a different number derived from the ELF. Both matter; keying a
    // registry on the wrong one would resolve nothing.
    //
    // LEZ frames the little-endian length before the bytes, so two inputs that
    // differ only by framing get different digests.
    let a = deployment_digest(b"\x00\x01");
    let b = deployment_digest(b"\x00\x01\x00");
    assert_ne!(a, b);
    assert_ne!(a, [0u8; 32]);
    // Deterministic, which is the entire premise of a reproducible build.
    assert_eq!(a, deployment_digest(b"\x00\x01"));
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
    let mut c = claim_for(true);
    c.build.toolchain = String::new();
    assert_eq!(Verification::claim(c).unwrap_err(), RegistryError::Empty);
}

#[test]
fn a_claim_with_no_repository_is_refused() {
    let mut c = claim_for(true);
    c.repository = String::new();
    assert_eq!(Verification::claim(c).unwrap_err(), RegistryError::Empty);
}

#[test]
fn a_verification_names_who_attested_to_it() {
    // The chain cannot compile anything, so the registry cannot verify a build.
    // Recording who says they rebuilt it is the difference between a fact the
    // registry can support and a claim it cannot.
    let subject = [1u8; 32];
    let mut v = Verification::claim(claim_for(true)).unwrap();
    assert_eq!(v.attester, None, "an unrebuilt claim vouches for nothing");

    v.record_rebuild(subject, subject, ATTESTER).unwrap();
    assert!(v.is_verified());
    assert_eq!(v.attester, Some(ATTESTER));
}

#[test]
fn a_self_attested_build_is_verified_but_flagged_as_such() {
    // An author rebuilding their own release reproducibly is useful and is not
    // an independent check. Both are true, so both are representable.
    let subject = [1u8; 32];

    let mut mine = Verification::claim(claim_for(true)).unwrap();
    mine.record_rebuild(subject, subject, AUTHOR).unwrap();
    assert!(mine.is_verified());
    assert!(
        mine.is_self_attested(&AUTHOR),
        "the only voucher is the shipper, and a UI must be able to say so"
    );

    let mut theirs = Verification::claim(claim_for(true)).unwrap();
    theirs.record_rebuild(subject, subject, ATTESTER).unwrap();
    assert!(theirs.is_verified());
    assert!(
        !theirs.is_self_attested(&AUTHOR),
        "an independent rebuild must not be labelled self-attested"
    );
}

#[test]
fn a_networked_rebuild_still_records_who_ran_it() {
    // Unreproducible is an outcome someone produced, not weather. Without the
    // attester it is impossible to tell a stalled claim from one that was tried
    // and failed, or to tell who tried.
    let subject = [1u8; 32];
    let mut v = Verification::claim(claim_for(false)).unwrap();
    let err = v.record_rebuild(subject, subject, ATTESTER).unwrap_err();
    assert_eq!(err, RegistryError::NetworkedBuild);
    assert_eq!(v.state, VerificationState::Unreproducible);
    assert_eq!(v.attester, Some(ATTESTER));
    assert!(!v.is_verified());
}
