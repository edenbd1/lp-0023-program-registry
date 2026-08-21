// SPDX-License-Identifier: MIT OR Apache-2.0
//! Every assertion here corresponds to a claim the registry makes about what
//! "verified" means. A claim without a test is a claim we should not print.

use lez_registry_core::*;

/// A point in chain time. Real ones come from the sequencer's clock account;
/// tests supply their own because the state machine only ever stores it.
const T0: Stamp = Stamp {
    block: 100,
    millis: 1_787_000_000_000,
};
const T1: Stamp = Stamp {
    block: 200,
    millis: 1_787_000_600_000,
};

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
    let err = v
        .record_rebuild(artifact, artifact, ATTESTER, T0)
        .unwrap_err();
    assert_eq!(err, RegistryError::NetworkedBuild);
    assert_eq!(v.state, VerificationState::Unreproducible);
    assert!(!v.is_verified());
}

#[test]
fn an_offline_rebuild_that_matches_is_verified() {
    let artifact = [3u8; 32];
    let mut v = Verification::claim(claim_for(true)).unwrap();
    v.record_rebuild(artifact, artifact, ATTESTER, T0).unwrap();
    assert_eq!(v.state, VerificationState::Verified);
    assert!(v.is_verified());
}

#[test]
fn a_rebuild_that_does_not_match_is_recorded_as_mismatched_not_left_as_claimed() {
    // Mismatched is its own state on purpose. "Nobody has said where the source
    // is" and "somebody said, and it does not reproduce" are opposite signals,
    // and a registry that showed the second as the first would be hiding the
    // stronger of the two.
    let artifact = [3u8; 32];
    let mut v = Verification::claim(claim_for(true)).unwrap();
    assert_eq!(
        v.record_rebuild([4u8; 32], artifact, ATTESTER, T0)
            .unwrap_err(),
        RegistryError::ArtifactMismatch
    );
    assert_eq!(v.state, VerificationState::Mismatched);
    assert_eq!(
        v.attester,
        Some(ATTESTER),
        "who found it must be on the record"
    );
    assert_eq!(v.last_checked, Some(T0), "and when");
    assert!(!v.is_verified());
}

#[test]
fn a_mismatch_assertion_cannot_erase_a_demonstrated_verification() {
    // `rebuilt` is whatever the caller says; the chain cannot check it. If a
    // mismatch could overwrite Verified, one transaction and 32 wrong bytes
    // would un-verify anything.
    let subject = [1u8; 32];
    let mut v = Verification::claim(claim_for(true)).unwrap();
    v.record_rebuild(subject, subject, AUTHOR, T0).unwrap();
    assert!(v.is_verified());

    assert_eq!(
        v.record_rebuild([9u8; 32], subject, ATTESTER, T1)
            .unwrap_err(),
        RegistryError::AlreadyVerified
    );
    assert!(
        v.is_verified(),
        "a passer-by must not undo a demonstrated fact"
    );
    assert_eq!(
        v.attester,
        Some(AUTHOR),
        "and must not appear to have attested"
    );
    assert_eq!(v.last_checked, Some(T0), "nor move the clock on it");
}

#[test]
fn a_verdict_is_always_dated_so_staleness_is_the_readers_to_judge() {
    let subject = [1u8; 32];
    let mut v = Verification::claim(claim_for(true)).unwrap();
    assert_eq!(v.last_checked, None, "an unchecked claim has no date");
    v.record_rebuild(subject, subject, ATTESTER, T1).unwrap();
    assert_eq!(v.last_checked, Some(T1));
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
    v.record_rebuild(subject, subject, ATTESTER, T0).unwrap();
    assert!(v.is_verified());

    // There is no call that takes this entry to any other artifact. The nearest
    // thing to an upgrade is a rebuild claiming different bytecode, and once a
    // verification exists that is refused outright -- see
    // `a_mismatch_assertion_cannot_erase_a_demonstrated_verification`.
    assert_eq!(
        v.record_rebuild([2u8; 32], subject, ATTESTER, T1)
            .unwrap_err(),
        RegistryError::AlreadyVerified
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
    first.record_rebuild(v1, v1, ATTESTER, T0).unwrap();
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
        v.record_rebuild([1u8; 32], [1u8; 32], ATTESTER, T0)
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
        T0,
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
            T0,
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
            T0,
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
        Entry::new(
            [1u8; 32],
            T0,
            long,
            "1".into(),
            author,
            Publisher::Canonical
        )
        .unwrap_err(),
        RegistryError::TooLong
    );

    let mut e = Entry::new(
        [1u8; 32],
        T0,
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

    v.record_rebuild(subject, subject, ATTESTER, T0).unwrap();
    assert!(v.is_verified());
    assert_eq!(v.attester, Some(ATTESTER));
}

#[test]
fn a_self_attested_build_is_verified_but_flagged_as_such() {
    // An author rebuilding their own release reproducibly is useful and is not
    // an independent check. Both are true, so both are representable.
    let subject = [1u8; 32];

    let mut mine = Verification::claim(claim_for(true)).unwrap();
    mine.record_rebuild(subject, subject, AUTHOR, T0).unwrap();
    assert!(mine.is_verified());
    assert!(
        mine.is_self_attested(&AUTHOR),
        "the only voucher is the shipper, and a UI must be able to say so"
    );

    let mut theirs = Verification::claim(claim_for(true)).unwrap();
    theirs
        .record_rebuild(subject, subject, ATTESTER, T0)
        .unwrap();
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
    let err = v
        .record_rebuild(subject, subject, ATTESTER, T0)
        .unwrap_err();
    assert_eq!(err, RegistryError::NetworkedBuild);
    assert_eq!(v.state, VerificationState::Unreproducible);
    assert_eq!(v.attester, Some(ATTESTER));
    assert!(!v.is_verified());
}

#[test]
fn the_sequence_directory_makes_the_registry_walkable_without_enumeration() {
    // The chain has no getProgramAccounts and no accounts-by-owner — probed
    // against the live sequencer, every such method answers "Method not found".
    // Resolution never needed one, because an entry's address is a function of
    // the program id. Discovery did: without this directory an indexer cannot
    // learn which program ids were ever registered.
    let a = seq_seed(0);
    let b = seq_seed(1);
    assert_ne!(a, b, "consecutive registrations must not share a slot");

    // Walking is derivation, not a query: slot n is computable before the
    // registry is asked anything, so an indexer's only chain access is a read
    // per slot.
    let walked: Vec<_> = (0..5).map(seq_seed).collect();
    let again: Vec<_> = (0..5).map(seq_seed).collect();
    assert_eq!(walked, again, "the walk must be reproducible");
    assert_eq!(
        walked
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        5,
        "five slots, five distinct addresses"
    );
}

#[test]
fn a_directory_slot_carries_the_seed_so_third_party_entries_are_reachable() {
    let slot = DirectorySlot {
        program_id: [3u8; 32],
        entry_seed: entry_seed(&[3u8; 32]),
    };
    let bytes = borsh::to_vec(&slot).unwrap();
    assert_eq!(borsh::from_slice::<DirectorySlot>(&bytes).unwrap(), slot);

    // A slot holding only the program id would send every walker to the
    // canonical entry, so a third-party entry would vanish from the listing --
    // exactly the entries a registry must not be able to hide.
    let third = DirectorySlot {
        program_id: [3u8; 32],
        entry_seed: third_party_entry_seed(&[3u8; 32], &[9u8; 32]),
    };
    assert_ne!(
        slot.entry_seed, third.entry_seed,
        "two entries about one program must lead to two different accounts"
    );
}

#[test]
fn a_real_logos_storage_identifier_round_trips() {
    // Two identifiers from a live Logos Storage node. They decode to 38 bytes,
    // not 32 -- CIDv1 marker, varint codec, then a multihash that carries its
    // own code and length before the digest. A 32-byte field would have kept
    // the digest and dropped the codec, leaving nothing able to fetch anything.
    for cid in [
        "zDvZRwzm9Ni32iyvoy868YVrBHFz8vo3oZirrCKhFMMn9aXuinfQ",
        "zDzSvJTf49uAN2BZhoMhZmDVSv7KDVeUVXTXvDc6QNnU1TKJYD55",
    ] {
        let got = decode_content_id(cid).expect("decodes");
        assert_eq!(got.len(), CONTENT_ID_LEN);
        assert_eq!(got[0], 0x01, "CIDv1 marker");
    }

    // The two differ in their codec bytes, which is exactly the part a 32-byte
    // field would have discarded.
    let a = decode_content_id("zDvZRwzm9Ni32iyvoy868YVrBHFz8vo3oZirrCKhFMMn9aXuinfQ").unwrap();
    let b = decode_content_id("zDzSvJTf49uAN2BZhoMhZmDVSv7KDVeUVXTXvDc6QNnU1TKJYD55").unwrap();
    assert_ne!(&a[..4], &b[..4], "different codecs");
}

#[test]
fn a_malformed_identifier_is_refused_rather_than_half_decoded() {
    // A half-decoded identifier fetches the wrong object or nothing, and both
    // are worse than an error.
    assert!(decode_content_id("").is_none(), "empty");
    assert!(
        decode_content_id("QmDvZRwzm9Ni32iyvoy868YVrBHFz8vo3oZirrCKhFMMn9a").is_none(),
        "not multibase base58btc"
    );
    assert!(
        decode_content_id("zDvZRwzm9Ni32i").is_none(),
        "too short to be a content identifier"
    );
    assert!(
        decode_content_id("zDvZRwzm9Ni32iyvoy868YVrBHFz8vo3oZirrCKhFMMn9aXuinfQQQQQ").is_none(),
        "too long"
    );
    assert!(
        decode_content_id("zDvZRwzm9Ni32iyvoy868YVrBHFz8vo3oZirrCKhFMMn9aXuinf0").is_none(),
        "0 is not in the base58 alphabet"
    );
}

/// Squatting is possible and worthless: registering first gets you an entry, and
/// only a rebuild that reproduces the deployed bytecode makes it stick.
mod canonical_takeover {
    use super::*;

    const SQUATTER: AccountId = [0x11; 32];
    const REAL: AccountId = [0x22; 32];
    const SUBJECT: ProgramId = [0x33; 32];

    fn squatted() -> Entry {
        let mut e = Entry::new(
            SUBJECT,
            T0,
            "popular-program".into(),
            "1.0.0".into(),
            SQUATTER,
            Publisher::Canonical,
        )
        .unwrap();
        e.verification = Verification::claim(claim_for(true)).unwrap();
        e
    }

    #[test]
    fn whoever_can_rebuild_it_takes_the_entry() {
        let mut e = squatted();
        assert_eq!(e.author, SQUATTER);

        e.claim_canonical(REAL, SUBJECT, T1).unwrap();
        assert_eq!(
            e.author, REAL,
            "the entry moves to whoever proved the build"
        );
        assert!(e.verification.is_verified());
        assert_eq!(e.verification.attester, Some(REAL));
        assert_eq!(e.verification.last_checked, Some(T1));
    }

    #[test]
    fn a_challenger_who_cannot_reproduce_the_bytecode_takes_nothing() {
        // This is the demo the criterion asks for: an unauthorised attempt at a
        // canonical entry, rejected.
        let mut e = squatted();
        assert_eq!(
            e.claim_canonical(REAL, [0x99; 32], T1).unwrap_err(),
            RegistryError::ArtifactMismatch
        );
        assert_eq!(
            e.author, SQUATTER,
            "the squatter keeps an entry nobody can back"
        );
        assert!(!e.verification.is_verified());
    }

    #[test]
    fn a_networked_rebuild_cannot_buy_the_entry() {
        // Otherwise the takeover path becomes the attack: a build.rs that
        // downloads the deployed binary reproduces it perfectly.
        let mut e = squatted();
        e.verification = Verification::claim(claim_for(false)).unwrap();
        assert_eq!(
            e.claim_canonical(REAL, SUBJECT, T1).unwrap_err(),
            RegistryError::NetworkedBuild
        );
        assert_eq!(e.author, SQUATTER);
    }

    #[test]
    fn a_verified_entry_cannot_be_taken_from_its_author() {
        // Once someone has demonstrated the build, the author is settled. A
        // takeover that worked on a verified entry would be the attack rather
        // than the defence against it.
        let mut e = squatted();
        e.claim_canonical(REAL, SUBJECT, T1).unwrap();

        let third: AccountId = [0x44; 32];
        assert_eq!(
            e.claim_canonical(third, SUBJECT, T1).unwrap_err(),
            RegistryError::AlreadyVerified
        );
        assert_eq!(e.author, REAL);
    }

    #[test]
    fn a_third_party_entry_is_not_contestable() {
        // It is keyed by its publisher, so it is theirs by construction and
        // there is nothing to contest.
        let mut e = Entry::new(
            SUBJECT,
            T0,
            "someone's opinion".into(),
            "1.0.0".into(),
            SQUATTER,
            Publisher::ThirdParty,
        )
        .unwrap();
        e.verification = Verification::claim(claim_for(true)).unwrap();
        assert_eq!(
            e.claim_canonical(REAL, SUBJECT, T1).unwrap_err(),
            RegistryError::NotAuthor
        );
    }

    #[test]
    fn a_frozen_entry_is_not_contestable_either() {
        let mut e = squatted();
        e.freeze(&SQUATTER).unwrap();
        assert_eq!(
            e.claim_canonical(REAL, SUBJECT, T1).unwrap_err(),
            RegistryError::Immutable
        );
    }
}
