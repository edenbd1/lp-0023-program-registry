// SPDX-License-Identifier: MIT OR Apache-2.0
//! Build `register` and `update` transactions in process.
//!
//! WHAT THIS MODULE PRODUCES, AND WHAT IT DELIBERATELY DOES NOT
//!
//! A LEZ public transaction is `Message { program_id, accounts, nonces,
//! instruction_data } + WitnessSet` — the accounts in the order the program
//! declares them, and `instruction_data` as risc0-serde `u32` words. This module
//! produces exactly those two program-defined halves: the ordered account plan
//! (with each account's address re-derived, not chosen) and the instruction
//! words. It does **not** fetch nonces, hold a key, or sign — signing is the
//! wallet's job and belongs where a key lives, not in a read-only SDK whose whole
//! argument is that nothing in its path can go down separately from the chain.
//!
//! So a caller hands the [`BuiltTransaction`]'s `account_ids()`, `signer_ids()`
//! and `instruction_words` to a signer, which is exactly the shape
//! `Message::new_preserialized(program_id, account_ids, nonces, instruction_data)`
//! takes. The bytes here are the bytes the deployed program decodes: the
//! encoding is pinned against risc0's own serializer in the tests, and the
//! account derivations are the same ones the read path already proves against
//! the live chain.
//!
//! WHY THE ENCODER IS REIMPLEMENTED RATHER THAN LINKED
//!
//! risc0's serializer would encode these words for us, but pulling the zkVM into
//! a resolve-and-read SDK to serialize four strings and three byte arrays is a
//! trade nobody reading this crate would take. The word format is small, fixed,
//! and pinned by [`tests`] against risc0's own published vectors, so it cannot
//! drift silently.

use crate::{directory_address, global_address, pda};
use lez_registry_core as core_;
pub use lez_registry_core::Publisher;
use lez_registry_core::{AccountId, ProgramId};

/// Instruction indices, which are the positions of the `#[instruction]`
/// functions in the program and therefore the variant indices risc0 writes as
/// the first word. Pinned against the generated IDL in the tests, so a reordered
/// program breaks a test rather than a live transaction.
pub const REGISTER_IX: u32 = 1;
pub const UPDATE_IX: u32 = 2;

/// Which declared account a slot is. The order of these in a built transaction
/// is the order the program destructures `pre_states`, so it is not cosmetic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The registry entry PDA.
    Entry,
    /// The singleton global-state PDA.
    Global,
    /// The sequence-directory slot PDA for this registration.
    Directory,
    /// A sequencer clock account, read for the registration timestamp.
    Clock,
    /// The signing author (or third-party publisher).
    Author,
}

impl Role {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Entry => "entry",
            Self::Global => "global",
            Self::Directory => "directory",
            Self::Clock => "clock",
            Self::Author => "author",
        }
    }
}

/// One account in a built transaction: which slot it is, the address the program
/// will insist on, and whether it must sign. `writable` mirrors the IDL and is
/// informational — the wire message carries only ids and signatures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountMeta {
    pub role: Role,
    pub id: AccountId,
    pub signer: bool,
    pub writable: bool,
}

/// A register or update transaction, resolved but unsigned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltTransaction {
    /// The registry program this transaction is addressed to.
    pub registry: ProgramId,
    /// Human name of the instruction, e.g. `"register"`.
    pub instruction: &'static str,
    /// The variant index risc0 wrote as the first instruction word.
    pub instruction_index: u32,
    /// Accounts in the order the program destructures them.
    pub accounts: Vec<AccountMeta>,
    /// risc0-serde words: `[variant_index, ..fields]`.
    pub instruction_words: Vec<u32>,
}

impl BuiltTransaction {
    /// Account ids in declared order — the `accounts` argument to a `Message`.
    #[must_use]
    pub fn account_ids(&self) -> Vec<AccountId> {
        self.accounts.iter().map(|a| a.id).collect()
    }

    /// The subset that must sign, in declared order — the accounts a `WitnessSet`
    /// covers and the ones whose nonces a signer fetches.
    #[must_use]
    pub fn signer_ids(&self) -> Vec<AccountId> {
        self.accounts
            .iter()
            .filter(|a| a.signer)
            .map(|a| a.id)
            .collect()
    }

    /// The entry PDA this transaction touches — always the first account, for
    /// both register and update.
    #[must_use]
    pub fn entry_address(&self) -> AccountId {
        self.accounts[0].id
    }

    /// The instruction words as little-endian bytes, the form an explorer or a
    /// JSON dry-run prints. `instruction_words` is the form a `Message` takes.
    #[must_use]
    pub fn instruction_data_bytes(&self) -> Vec<u8> {
        self.instruction_words
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect()
    }
}

/// Why a transaction could not be built. Field limits are the program's own, so
/// a value this rejects is a value the chain would reject — caught here without
/// a round trip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildError {
    /// A metadata field was empty or over the length the account layout reserves.
    Field(core_::RegistryError),
    /// The IDL pointer was neither empty nor a valid multibase content id.
    BadIdl,
}

impl core::fmt::Display for BuildError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Field(e) => write!(f, "field is empty or too long: {e:?}"),
            Self::BadIdl => write!(
                f,
                "idl is not a multibase base58btc content identifier (or empty to clear)"
            ),
        }
    }
}

impl std::error::Error for BuildError {}

/// A request to register a program.
///
/// `publisher` decides everything downstream: a [`Publisher::Canonical`] entry
/// lives at the address the program id alone derives, so there is one and a
/// stranger cannot occupy it; a [`Publisher::ThirdParty`] entry is keyed by
/// `author`, so any number coexist. `seq_index` is the registry's current
/// `total_entries` — read it from the chain, since the program re-derives the
/// directory slot from the counter and refuses a seed that is not the slot this
/// registration lands in.
#[derive(Debug, Clone)]
pub struct RegisterRequest<'a> {
    pub program: &'a ProgramId,
    pub author: &'a AccountId,
    pub publisher: Publisher,
    pub name: &'a str,
    pub version: &'a str,
    pub description: &'a str,
    pub clock: &'a AccountId,
    pub seq_index: u64,
}

/// A request to update an entry's metadata. `author` is the signer, and only
/// the entry's recorded author may update — a rule this builder cannot enforce
/// offline but a caller can check against the read path before submitting.
///
/// `idl` is the multibase `z…` content identifier a Logos Storage node hands
/// you, or `""` to clear the pointer. It is validated here the same way the
/// program validates it, so a malformed identifier fails before submission.
#[derive(Debug, Clone)]
pub struct UpdateRequest<'a> {
    pub program: &'a ProgramId,
    pub author: &'a AccountId,
    pub publisher: Publisher,
    pub version: &'a str,
    pub description: &'a str,
    pub idl: &'a str,
}

/// The seed for the entry a request targets: canonical from the program id
/// alone, third-party keyed additionally by the publisher.
#[must_use]
fn entry_seed_for(publisher: Publisher, program: &ProgramId, author: &AccountId) -> [u8; 32] {
    match publisher {
        Publisher::Canonical => core_::entry_seed(program),
        Publisher::ThirdParty => core_::third_party_entry_seed(program, author),
    }
}

/// Build a `register` transaction. The account addresses are re-derived, never
/// chosen, and the metadata is validated against the program's own field limits
/// so an over-long name fails here rather than after a round trip.
pub fn build_register(
    registry: &ProgramId,
    req: &RegisterRequest<'_>,
) -> Result<BuiltTransaction, BuildError> {
    // Validate with the program's own type: `Entry::new` checks name and
    // version, and a second `validate` after setting the description catches an
    // over-long one. Reusing the type is the point — the CLI cannot disagree
    // with the chain about what fits.
    let mut e = core_::Entry::new(
        *req.program,
        core_::Stamp {
            block: 0,
            millis: 0,
        },
        req.name.to_string(),
        req.version.to_string(),
        *req.author,
        req.publisher,
    )
    .map_err(BuildError::Field)?;
    e.description = req.description.to_string();
    e.validate().map_err(BuildError::Field)?;

    let entry_seed = entry_seed_for(req.publisher, req.program, req.author);
    let seq_seed = core_::seq_seed(req.seq_index);
    let canonical: u8 = matches!(req.publisher, Publisher::Canonical).into();

    let accounts = vec![
        AccountMeta {
            role: Role::Entry,
            id: pda(registry, &entry_seed),
            signer: false,
            writable: true,
        },
        AccountMeta {
            role: Role::Global,
            id: global_address(registry),
            signer: false,
            writable: true,
        },
        AccountMeta {
            role: Role::Directory,
            id: directory_address(registry, req.seq_index),
            signer: false,
            writable: true,
        },
        AccountMeta {
            role: Role::Clock,
            id: *req.clock,
            signer: false,
            writable: false,
        },
        AccountMeta {
            role: Role::Author,
            id: *req.author,
            signer: true,
            writable: false,
        },
    ];

    let mut w = Vec::new();
    w.push(REGISTER_IX);
    push_fixed_bytes(&mut w, &entry_seed);
    push_fixed_bytes(&mut w, &seq_seed);
    push_fixed_bytes(&mut w, req.program);
    w.push(u32::from(canonical));
    push_string(&mut w, req.name);
    push_string(&mut w, req.version);
    push_string(&mut w, req.description);

    Ok(BuiltTransaction {
        registry: *registry,
        instruction: "register",
        instruction_index: REGISTER_IX,
        accounts,
        instruction_words: w,
    })
}

/// Build an `update` transaction. The entry seed is re-derived from the program
/// id and publisher, and the IDL pointer is validated exactly as the program
/// validates it.
pub fn build_update(
    registry: &ProgramId,
    req: &UpdateRequest<'_>,
) -> Result<BuiltTransaction, BuildError> {
    // Match the program's field checks for the fields update actually sets. The
    // name is not touched by update, so it is not checked here.
    if req.version.is_empty() {
        return Err(BuildError::Field(core_::RegistryError::Empty));
    }
    if req.version.len() > core_::MAX_VERSION || req.description.len() > core_::MAX_DESCRIPTION {
        return Err(BuildError::Field(core_::RegistryError::TooLong));
    }
    // Empty clears the pointer; anything else must decode as a content id, the
    // same boundary the program enforces so a plausible-but-wrong string fails
    // before submission rather than turning into a fetch that returns nothing.
    if !req.idl.is_empty() && core_::decode_content_id(req.idl).is_none() {
        return Err(BuildError::BadIdl);
    }

    let entry_seed = entry_seed_for(req.publisher, req.program, req.author);

    let accounts = vec![
        AccountMeta {
            role: Role::Entry,
            id: pda(registry, &entry_seed),
            signer: false,
            // The IDL declares no `mut` on update's entry; the write rides on the
            // program's post-state claim, not on a wire flag. Mirrored here so the
            // plan a caller prints matches the IDL.
            writable: false,
        },
        AccountMeta {
            role: Role::Author,
            id: *req.author,
            signer: true,
            writable: false,
        },
    ];

    let mut w = Vec::new();
    w.push(UPDATE_IX);
    push_fixed_bytes(&mut w, &entry_seed);
    push_string(&mut w, req.version);
    push_string(&mut w, req.description);
    push_string(&mut w, req.idl);

    Ok(BuiltTransaction {
        registry: *registry,
        instruction: "update",
        instruction_index: UPDATE_IX,
        accounts,
        instruction_words: w,
    })
}

// ── risc0-serde word encoding ────────────────────────────────────────────────
//
// Reproduces `risc0_zkvm::serde::to_vec` for exactly the types register and
// update use. Pinned against risc0's published vectors in the tests.

/// A fixed-size `[u8; N]` serializes as a tuple: each byte its own zero-extended
/// `u32` word, no length prefix. (A `Vec<u8>` would prefix a length; a byte
/// array does not, which is why the seeds are arrays.)
fn push_fixed_bytes(out: &mut Vec<u32>, bytes: &[u8]) {
    out.extend(bytes.iter().map(|&b| u32::from(b)));
}

/// A `String` serializes as a `u32` byte-length, then the bytes packed four per
/// word little-endian, the final word zero-padded. An empty string is just the
/// length word `0`.
fn push_string(out: &mut Vec<u32>, s: &str) {
    let bytes = s.as_bytes();
    out.push(bytes.len() as u32);
    for chunk in bytes.chunks(4) {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        out.push(u32::from_le_bytes(word));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{base58, entry_address, third_party_entry_address};

    const REGISTRY_HEX: &str = "f59751e932e856f7565f34e79ae726791794dc68e55f2413bc0375008cc07bf8";

    fn registry() -> ProgramId {
        let mut out = [0u8; 32];
        for (i, b) in out.iter_mut().enumerate() {
            *b = u8::from_str_radix(&REGISTRY_HEX[i * 2..i * 2 + 2], 16).expect("valid hex");
        }
        out
    }

    // ── the word encoder against risc0's own published vectors ──────────────

    #[test]
    fn string_words_match_risc0_reference() {
        // From risc0_zkvm::serde::serializer test_str: "a" → [1, 0x61],
        // "abc" → [3, 0x00636261]. If these drift, the guest cannot decode a name.
        let mut a = Vec::new();
        push_string(&mut a, "a");
        assert_eq!(a, vec![1, 0x0000_0061]);

        let mut abc = Vec::new();
        push_string(&mut abc, "abc");
        assert_eq!(abc, vec![3, 0x0063_6261]);

        // Empty is a bare length word, which is how `update` clears an IDL.
        let mut empty = Vec::new();
        push_string(&mut empty, "");
        assert_eq!(empty, vec![0]);
    }

    #[test]
    fn fixed_bytes_are_one_word_per_byte_no_length() {
        let mut w = Vec::new();
        push_fixed_bytes(&mut w, &[0x01, 0x02, 0xff]);
        assert_eq!(w, vec![0x01, 0x02, 0xff]);
    }

    // ── register: structure, derivations and the full word layout ───────────

    #[test]
    fn register_canonical_accounts_and_derivations() {
        let reg = registry();
        let program = [0x11u8; 32];
        let author = [0x22u8; 32];
        let clock = [0x33u8; 32];
        let tx = build_register(
            &reg,
            &RegisterRequest {
                program: &program,
                author: &author,
                publisher: Publisher::Canonical,
                name: "n",
                version: "v",
                description: "",
                clock: &clock,
                seq_index: 7,
            },
        )
        .unwrap();

        let roles: Vec<_> = tx.accounts.iter().map(|a| a.role).collect();
        assert_eq!(
            roles,
            vec![
                Role::Entry,
                Role::Global,
                Role::Directory,
                Role::Clock,
                Role::Author
            ]
        );
        // Only the author signs.
        assert_eq!(tx.signer_ids(), vec![author]);
        // The entry is the canonical address the read path derives for this
        // program, so a built transaction and a resolve agree by construction.
        assert_eq!(tx.entry_address(), entry_address(&reg, &program));
        assert_eq!(tx.accounts[1].id, global_address(&reg));
        assert_eq!(tx.accounts[2].id, directory_address(&reg, 7));
        assert_eq!(tx.accounts[3].id, clock);
    }

    #[test]
    fn register_third_party_lands_at_the_publisher_keyed_address() {
        let reg = registry();
        let program = [0x11u8; 32];
        let author = [0x22u8; 32];
        let clock = [0x33u8; 32];
        let tx = build_register(
            &reg,
            &RegisterRequest {
                program: &program,
                author: &author,
                publisher: Publisher::ThirdParty,
                name: "n",
                version: "v",
                description: "",
                clock: &clock,
                seq_index: 0,
            },
        )
        .unwrap();
        assert_eq!(
            tx.entry_address(),
            third_party_entry_address(&reg, &program, &author),
            "a third-party entry must not land on the canonical address"
        );
        assert_ne!(tx.entry_address(), entry_address(&reg, &program));
        // canonical byte is 0 for a third-party entry: word after the three
        // 32-byte seeds/id (1 + 32 + 32 + 32 = index 97).
        assert_eq!(tx.instruction_words[97], 0);
    }

    #[test]
    fn register_word_layout_is_exactly_the_program_format() {
        let reg = registry();
        let program: [u8; 32] = core::array::from_fn(|i| i as u8);
        let author = [0xAAu8; 32];
        let clock = [0u8; 32];
        let tx = build_register(
            &reg,
            &RegisterRequest {
                program: &program,
                author: &author,
                publisher: Publisher::Canonical,
                name: "hi",
                version: "1",
                description: "abc",
                clock: &clock,
                seq_index: 3,
            },
        )
        .unwrap();

        let entry_seed = core_::entry_seed(&program);
        let seq_seed = core_::seq_seed(3);

        let mut expected = Vec::new();
        expected.push(REGISTER_IX);
        expected.extend(entry_seed.iter().map(|&b| u32::from(b)));
        expected.extend(seq_seed.iter().map(|&b| u32::from(b)));
        expected.extend(program.iter().map(|&b| u32::from(b)));
        expected.push(1); // canonical
        expected.extend([2, 0x0000_6968]); // "hi"
        expected.extend([1, 0x0000_0031]); // "1"
        expected.extend([3, 0x0063_6261]); // "abc"
        assert_eq!(tx.instruction_words, expected);
    }

    #[test]
    fn over_long_name_is_refused_before_submission() {
        let reg = registry();
        let program = [1u8; 32];
        let author = [2u8; 32];
        let clock = [3u8; 32];
        let long = "x".repeat(core_::MAX_NAME + 1);
        let err = build_register(
            &reg,
            &RegisterRequest {
                program: &program,
                author: &author,
                publisher: Publisher::Canonical,
                name: &long,
                version: "1",
                description: "",
                clock: &clock,
                seq_index: 0,
            },
        )
        .unwrap_err();
        assert_eq!(err, BuildError::Field(core_::RegistryError::TooLong));
    }

    // ── update: structure, word layout and IDL validation ───────────────────

    #[test]
    fn update_accounts_and_words() {
        let reg = registry();
        let program = [0x11u8; 32];
        let author = [0x22u8; 32];
        let tx = build_update(
            &reg,
            &UpdateRequest {
                program: &program,
                author: &author,
                publisher: Publisher::Canonical,
                version: "2",
                description: "d",
                idl: "",
            },
        )
        .unwrap();

        let roles: Vec<_> = tx.accounts.iter().map(|a| a.role).collect();
        assert_eq!(roles, vec![Role::Entry, Role::Author]);
        assert_eq!(tx.signer_ids(), vec![author]);
        assert_eq!(tx.entry_address(), entry_address(&reg, &program));

        let entry_seed = core_::entry_seed(&program);
        let mut expected = Vec::new();
        expected.push(UPDATE_IX);
        expected.extend(entry_seed.iter().map(|&b| u32::from(b)));
        expected.extend([1, 0x0000_0032]); // "2"
        expected.extend([1, 0x0000_0064]); // "d"
        expected.push(0); // empty idl clears the pointer
        assert_eq!(tx.instruction_words, expected);
    }

    #[test]
    fn update_rejects_a_malformed_idl_but_accepts_a_real_one() {
        let reg = registry();
        let program = [0x11u8; 32];
        let author = [0x22u8; 32];
        let base = UpdateRequest {
            program: &program,
            author: &author,
            publisher: Publisher::Canonical,
            version: "1",
            description: "",
            idl: "not-a-cid",
        };
        assert_eq!(build_update(&reg, &base).unwrap_err(), BuildError::BadIdl);

        // A real Logos Storage identifier from a live node decodes cleanly.
        let good = UpdateRequest {
            idl: "zDvZRwzmA4EqenwX34C9oqpDpp4jP5SM8k7y5kE7DccFe7xVDHNG",
            ..base
        };
        assert!(build_update(&reg, &good).is_ok());
    }

    #[test]
    fn empty_version_on_update_is_refused() {
        let reg = registry();
        let program = [0x11u8; 32];
        let author = [0x22u8; 32];
        let err = build_update(
            &reg,
            &UpdateRequest {
                program: &program,
                author: &author,
                publisher: Publisher::Canonical,
                version: "",
                description: "",
                idl: "",
            },
        )
        .unwrap_err();
        assert_eq!(err, BuildError::Field(core_::RegistryError::Empty));
    }

    // ── the instruction indices are the ones the generated IDL publishes ─────

    #[test]
    fn instruction_indices_match_the_generated_idl() {
        // The variant index is the first instruction word; if the program is
        // reordered, this catches it here rather than as a transaction that runs
        // the wrong instruction.
        let idl = include_str!("../../idl/lez_registry.idl.json");
        let idl: serde_json::Value = serde_json::from_str(idl).expect("IDL is valid JSON");
        let ixs = idl["instructions"].as_array().expect("instructions array");
        assert_eq!(ixs[REGISTER_IX as usize]["name"], "register");
        assert_eq!(ixs[UPDATE_IX as usize]["name"], "update");

        // And the arg order the encoder assumes is the arg order the IDL lists.
        let names = |i: usize| -> Vec<String> {
            ixs[i]["args"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| a["name"].as_str().unwrap().to_string())
                .collect()
        };
        assert_eq!(
            names(REGISTER_IX as usize),
            [
                "entry_seed",
                "seq_seed",
                "program_id",
                "canonical",
                "name",
                "version",
                "description"
            ]
        );
        assert_eq!(
            names(UPDATE_IX as usize),
            ["entry_seed", "version", "description", "idl"]
        );
    }

    #[test]
    fn base58_round_trips_the_entry_address() {
        // A sanity tie to the read path's own golden address: the registry's
        // canonical entry is a known live address, and a built register targets
        // exactly it.
        let reg = registry();
        let tx = build_register(
            &reg,
            &RegisterRequest {
                program: &reg,
                author: &[9u8; 32],
                publisher: Publisher::Canonical,
                name: "lez-program-registry",
                version: "0.7.0",
                description: "",
                clock: &[0u8; 32],
                seq_index: 0,
            },
        )
        .unwrap();
        assert_eq!(
            base58(&tx.entry_address()),
            "wkLxSjsYNRq24FKPzDSh1haT382zwMU5QFb2YuHRD4h"
        );
    }
}
