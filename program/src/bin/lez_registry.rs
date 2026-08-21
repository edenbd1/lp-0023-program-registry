// The LEZ program registry, on chain.
//
// WHAT THIS PROGRAM IS FOR
//
// A LEZ program is identified by an opaque 32-byte id. Nothing in it says what
// the program does, who wrote it, what its interface is, or whether the source
// you are reading is the source that is running. This program is where those
// answers live, so that a wallet showing a transaction, an explorer decoding
// one, or a developer choosing a dependency can all ask the same place.
//
// WHY THE RULES ARE NOT WRITTEN HERE
//
// Every check below delegates to `lez-registry-core`, the same crate the CLI
// and the SDK link. Address derivation and the meaning of "verified" are
// written once. If they were written twice they would agree until the day they
// did not, and that day would be the day a user trusted a wrong answer.
//
// WHAT IS DELIBERATELY ABSENT
//
// No curation, no ranking, no trust score. The registry records what authors
// assert plus what is objectively checkable, and refuses to decide which
// programs are good — that is a judgement, and a judgement stored on chain by
// this program would be mistaken for a fact.

// SPEL instructions take accounts and arguments as one flat parameter list, so
// `too_many_arguments` fires on every non-trivial instruction; splitting them
// into structs to satisfy the lint would fight the macro rather than help a
// reader. `redundant_clone` fires inside the macro's own expansion, where there
// is nothing here to change.
#![allow(
    clippy::too_many_arguments,
    clippy::redundant_clone,
    clippy::cloned_ref_to_slice_refs
)]
#![no_main]

use spel_framework::prelude::*;

risc0_zkvm::guest::entry!(main);

const E_BAD_ENTRY: u32 = 9001;
const E_NOT_ANCHORED: u32 = 9002;
const E_NOT_AUTHOR: u32 = 9003;
const E_IMMUTABLE: u32 = 9004;
const E_SEED_MISMATCH: u32 = 9005;
const E_BAD_CLAIM: u32 = 9006;
const E_NOT_VERIFIED: u32 = 9007;
const E_ALREADY_REGISTERED: u32 = 9008;
const E_BAD_CLOCK: u32 = 9009;

/// The LEZ clock program, pinned by ImageID.
///
/// A zkVM guest has no clock, and the obvious workaround — let the client pass a
/// timestamp — makes every date in this registry a number the registering party
/// chose. Since the adoption story a registry tells is entirely about *when*
/// things were registered, that is not a small hole.
///
/// LEZ maintains clock accounts at fixed addresses, written by the sequencer.
/// This program reads one as an input and refuses it unless the account is owned
/// by exactly this program id — verified against
/// `artifacts/lez/programs/clock.bin` at v0.2.4 and against the live account's
/// own `program_owner`. Without the pin, a caller could supply an account owned
/// by a program they wrote and choose the time after all.
const CLOCK_PROGRAM_ID: [u32; 8] = [
    96_247_601,
    2_082_502_477,
    822_865_082,
    1_048_693_993,
    3_544_189_898,
    772_921_104,
    1_694_408_900,
    4_234_239_033,
];

/// The three addresses the clock program writes: every block, every 10, every 50.
/// Checked as well as the owner, so a caller cannot point at some *other*
/// account the clock program happens to own.
const CLOCK_ACCOUNT_IDS: [&[u8; 32]; 3] = [
    b"/LEZ/ClockProgramAccount/0000001",
    b"/LEZ/ClockProgramAccount/0000010",
    b"/LEZ/ClockProgramAccount/0000050",
];

/// Read chain time from a supplied clock account, or refuse.
fn read_clock(clock: &AccountWithMetadata) -> Result<lez_registry_core::Stamp, SpelError> {
    if clock.account.program_owner != CLOCK_PROGRAM_ID {
        return Err(SpelError::custom(
            E_BAD_CLOCK,
            "clock account is not owned by the LEZ clock program",
        ));
    }
    let id = clock.account_id.value();
    if !CLOCK_ACCOUNT_IDS.contains(&id) {
        return Err(SpelError::custom(
            E_BAD_CLOCK,
            "not one of the LEZ clock accounts",
        ));
    }
    let d = &clock.account.data;
    if d.len() < 16 {
        return Err(SpelError::custom(E_BAD_CLOCK, "clock account is too short"));
    }
    let mut b = [0u8; 8];
    b.copy_from_slice(&d[0..8]);
    let block = u64::from_le_bytes(b);
    b.copy_from_slice(&d[8..16]);
    let millis = u64::from_le_bytes(b);
    Ok(lez_registry_core::Stamp { block, millis })
}

/// A registry entry, stored as the core crate defines it. The on-chain layout
/// is `lez_registry_core::Entry` verbatim rather than a parallel struct, so a
/// client that decodes an account gets the type the program wrote.
#[account_type]
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug)]
pub struct EntryAccount {
    pub entry: Vec<u8>,
}

/// One slot of the sequence directory: which program registered nth.
///
/// Exists because the chain cannot enumerate. Resolution never needs this;
/// discovery does, and without it an indexer has no way to learn which program
/// ids were ever registered.
#[account_type]
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug)]
pub struct DirectoryAccount {
    pub program_id: [u8; 32],
}

/// Registry-wide counters.
#[account_type]
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, Default)]
pub struct GlobalAccount {
    pub total_entries: u64,
    pub total_verified: u64,
}

#[lez_program]
mod lez_registry {
    #[allow(unused_imports)]
    use super::*;

    fn write(account: &mut Account, bytes: &[u8]) -> Result<(), SpelError> {
        account.data = bytes
            .to_vec()
            .try_into()
            .map_err(|_| SpelError::custom(E_BAD_ENTRY, "state does not fit the account buffer"))?;
        Ok(())
    }

    fn load_entry(account: &Account) -> Result<lez_registry_core::Entry, SpelError> {
        let wrapper = EntryAccount::try_from_slice(&account.data)
            .map_err(|_| SpelError::custom(E_BAD_ENTRY, "entry account failed to decode"))?;
        borsh::from_slice(&wrapper.entry)
            .map_err(|_| SpelError::custom(E_BAD_ENTRY, "entry payload failed to decode"))
    }

    fn store_entry(
        account: &mut Account,
        entry: &lez_registry_core::Entry,
    ) -> Result<(), SpelError> {
        let payload = borsh::to_vec(entry)
            .map_err(|_| SpelError::custom(E_BAD_ENTRY, "entry failed to encode"))?;
        let wrapper = borsh::to_vec(&EntryAccount { entry: payload })
            .map_err(|_| SpelError::custom(E_BAD_ENTRY, "entry wrapper failed to encode"))?;
        write(account, &wrapper)
    }

    /// Create the singleton global-state account. Permissionless: it holds only
    /// counters, so there is nothing for a first caller to capture.
    #[instruction]
    pub fn initialize(
        #[account(init, pda = [literal("lez-registry:global:v1")])] mut global: AccountWithMetadata,
    ) -> SpelResult {
        let bytes = borsh::to_vec(&GlobalAccount::default())
            .map_err(|_| SpelError::custom(E_BAD_ENTRY, "global failed to encode"))?;
        write(&mut global.account, &bytes)?;
        Ok(SpelOutput::execute(vec![global], vec![]))
    }

    /// Register a program.
    ///
    /// The entry account is a PDA whose seed the caller supplies and this
    /// program re-derives from the program id. A caller who computes it wrongly
    /// lands on an address the derivation does not produce, and the check below
    /// refuses — so the address cannot be chosen, only computed.
    ///
    /// `canonical` decides which of two derivations applies. The canonical
    /// address is a function of the program id alone, so there is exactly one
    /// and a stranger cannot occupy it; a third-party entry is keyed by its
    /// publisher, so any number of them coexist without colliding.
    #[instruction]
    pub fn register(
        ctx: ProgramContext,
        #[account(init, pda = arg("entry_seed"))] mut entry: AccountWithMetadata,
        #[account(mut, pda = [literal("lez-registry:global:v1")])] mut global: AccountWithMetadata,
        #[account(init, pda = arg("seq_seed"))] mut directory: AccountWithMetadata,
        clock: AccountWithMetadata,
        #[account(signer)] author: AccountWithMetadata,
        entry_seed: [u8; 32],
        seq_seed: [u8; 32],
        program_id: [u8; 32],
        canonical: u8,
        name: String,
        version: String,
        description: String,
    ) -> SpelResult {
        if global.account.program_owner != ctx.self_program_id {
            return Err(SpelError::custom(
                E_NOT_ANCHORED,
                "global state is not owned by this program",
            ));
        }

        let author_id = *author.account_id.value();
        let publisher = if canonical == 1 {
            lez_registry_core::Publisher::Canonical
        } else {
            lez_registry_core::Publisher::ThirdParty
        };

        // Re-derive rather than trust. The seed is an argument because the
        // runtime needs it to place the account, but a supplied seed that does
        // not match the derivation would let a caller put an entry anywhere.
        let expected = match publisher {
            lez_registry_core::Publisher::Canonical => lez_registry_core::entry_seed(&program_id),
            lez_registry_core::Publisher::ThirdParty => {
                lez_registry_core::third_party_entry_seed(&program_id, &author_id)
            }
        };
        if expected != entry_seed {
            return Err(SpelError::custom(
                E_SEED_MISMATCH,
                "entry_seed is not the address this program id derives",
            ));
        }

        // The directory slot is not the caller's to choose either: it is the
        // counter's current value. Trusting the supplied seed would let a caller
        // overwrite an earlier slot and hide an entry from every indexer.
        let mut g = GlobalAccount::try_from_slice(&global.account.data)
            .map_err(|_| SpelError::custom(E_BAD_ENTRY, "global failed to decode"))?;
        if lez_registry_core::seq_seed(g.total_entries) != seq_seed {
            return Err(SpelError::custom(
                E_SEED_MISMATCH,
                "seq_seed is not the slot this registration derives",
            ));
        }

        // `init` already refuses an existing account, so a duplicate lands here
        // rather than silently overwriting someone's entry.
        if !entry.account.data.is_empty() {
            return Err(SpelError::custom(
                E_ALREADY_REGISTERED,
                "an entry already exists at this address",
            ));
        }

        let stamp = read_clock(&clock)?;
        let mut e =
            lez_registry_core::Entry::new(program_id, stamp, name, version, author_id, publisher)
                .map_err(|_| SpelError::custom(E_BAD_ENTRY, "entry fields are empty or too long"))?;
        e.description = description;
        e.validate()
            .map_err(|_| SpelError::custom(E_BAD_ENTRY, "entry fields are empty or too long"))?;
        store_entry(&mut entry.account, &e)?;

        let slot = borsh::to_vec(&DirectoryAccount { program_id })
            .map_err(|_| SpelError::custom(E_BAD_ENTRY, "directory slot failed to encode"))?;
        write(&mut directory.account, &slot)?;

        g.total_entries = g
            .total_entries
            .checked_add(1)
            .ok_or_else(|| SpelError::custom(E_BAD_ENTRY, "entry counter overflow"))?;
        let bytes = borsh::to_vec(&g)
            .map_err(|_| SpelError::custom(E_BAD_ENTRY, "global failed to encode"))?;
        write(&mut global.account, &bytes)?;

        Ok(SpelOutput::execute(
            vec![entry, global, directory, clock, author],
            vec![],
        ))
    }

    /// Update metadata. Author only, and only while unfrozen — both enforced by
    /// the core's `require_writable_by`, so no instruction can implement half
    /// the rule.
    #[instruction]
    pub fn update(
        ctx: ProgramContext,
        #[account(pda = arg("entry_seed"))] mut entry: AccountWithMetadata,
        #[account(signer)] author: AccountWithMetadata,
        entry_seed: [u8; 32],
        version: String,
        description: String,
        idl: [u8; 32],
    ) -> SpelResult {
        let _ = entry_seed;
        if entry.account.program_owner != ctx.self_program_id {
            return Err(SpelError::custom(
                E_NOT_ANCHORED,
                "no entry at this address",
            ));
        }
        let mut e = load_entry(&entry.account)?;
        e.require_writable_by(author.account_id.value())
            .map_err(|err| match err {
                lez_registry_core::RegistryError::Immutable => {
                    SpelError::custom(E_IMMUTABLE, "entry is frozen")
                }
                _ => SpelError::custom(E_NOT_AUTHOR, "signer is not this entry's author"),
            })?;

        e.version = version;
        e.description = description;
        // An all-zero identifier means "no IDL", which is distinguishable from a
        // real Storage identifier because Storage identifiers are content
        // hashes and a hash of anything is not zero.
        e.idl = if idl == [0u8; 32] { None } else { Some(idl) };
        e.validate()
            .map_err(|_| SpelError::custom(E_BAD_ENTRY, "entry fields are empty or too long"))?;
        store_entry(&mut entry.account, &e)?;
        Ok(SpelOutput::execute(vec![entry, author], vec![]))
    }

    /// Attach a source claim to an entry. Author only, because the canonical
    /// entry is the author's; a stranger who wants to assert something about
    /// this program registers their own third-party entry and claims there.
    ///
    /// This deliberately proves nothing. It records that someone said where the
    /// source is, which is the input to a rebuild, not a substitute for one.
    #[instruction]
    pub fn claim_source(
        ctx: ProgramContext,
        #[account(pda = arg("entry_seed"))] mut entry: AccountWithMetadata,
        #[account(signer)] author: AccountWithMetadata,
        entry_seed: [u8; 32],
        repository: String,
        commit: [u8; 20],
        source_digest: [u8; 32],
        image_digest: [u8; 32],
        toolchain: String,
        network_disabled: u8,
    ) -> SpelResult {
        let _ = entry_seed;
        if entry.account.program_owner != ctx.self_program_id {
            return Err(SpelError::custom(
                E_NOT_ANCHORED,
                "no entry at this address",
            ));
        }
        let mut e = load_entry(&entry.account)?;
        e.require_writable_by(author.account_id.value())
            .map_err(|err| match err {
                lez_registry_core::RegistryError::Immutable => {
                    SpelError::custom(E_IMMUTABLE, "entry is frozen")
                }
                _ => SpelError::custom(E_NOT_AUTHOR, "signer is not this entry's author"),
            })?;

        let claim = lez_registry_core::SourceClaim {
            repository,
            commit,
            source_digest,
            build: lez_registry_core::BuildEnvironment {
                image_digest,
                toolchain,
                // A u8 rather than a bool because the instruction encoding has no
                // bool; anything other than 1 is read as "the network was up",
                // so a caller who fumbles the flag gets the safe answer.
                network_disabled: network_disabled == 1,
            },
        };
        e.verification = lez_registry_core::Verification::claim(claim)
            .map_err(|_| SpelError::custom(E_BAD_CLAIM, "claim fields are empty or too long"))?;
        store_entry(&mut entry.account, &e)?;
        Ok(SpelOutput::execute(vec![entry, author], vec![]))
    }

    /// Record that a rebuild of the claimed source reproduced this entry's
    /// bytecode.
    ///
    /// **Anyone may call this**, and that is the design rather than an oversight.
    /// Restricting it to the author would make `Verified` mean "the author says
    /// so", which is what the state exists to improve on. Opening it means the
    /// signer is written down as the attester, so the badge always carries a name
    /// and a reader decides whose word it is worth.
    ///
    /// A chain cannot compile, so nothing here checks that the rebuild happened.
    /// What it checks is that the claimed environment had the network disabled —
    /// the one condition under which a matching rebuild is evidence at all.
    #[instruction]
    pub fn record_rebuild(
        ctx: ProgramContext,
        #[account(pda = arg("entry_seed"))] mut entry: AccountWithMetadata,
        #[account(mut, pda = [literal("lez-registry:global:v1")])] mut global: AccountWithMetadata,
        clock: AccountWithMetadata,
        #[account(signer)] attester: AccountWithMetadata,
        entry_seed: [u8; 32],
        rebuilt: [u8; 32],
    ) -> SpelResult {
        let _ = entry_seed;
        if entry.account.program_owner != ctx.self_program_id
            || global.account.program_owner != ctx.self_program_id
        {
            return Err(SpelError::custom(
                E_NOT_ANCHORED,
                "entry or global state is not owned by this program",
            ));
        }
        let mut e = load_entry(&entry.account)?;
        let was_verified = e.verification.is_verified();
        let subject = e.program_id;

        let stamp = read_clock(&clock)?;
        let outcome =
            e.verification
                .record_rebuild(rebuilt, subject, *attester.account_id.value(), stamp);

        // Returning Err here would discard the write: an erroring instruction
        // aborts the transaction, so the post-state never lands. That was this
        // instruction's first shape, and reading the account back after a
        // refusal showed the entry still `Claimed` with no attester -- the
        // "recorded refusal" existed only in the source.
        //
        // So the two outcomes are split by whether there is anything to record.
        // A networked build IS an outcome: Unreproducible, with the attester
        // named, committed by returning Ok. Nobody can grief an entry this way,
        // because the verdict is a function of the claim the author wrote --
        // anyone reading it can already see the build was networked.
        // Two of the three failures are *outcomes* and have to survive: a
        // networked build and a mismatch both leave a state a reader needs. An
        // erroring instruction aborts its transaction, so persisting them means
        // returning Ok -- see the comment on the earlier shape of this code.
        let recorded = matches!(
            outcome,
            Err(lez_registry_core::RegistryError::NetworkedBuild)
                | Err(lez_registry_core::RegistryError::ArtifactMismatch)
        );
        if let Err(err) = outcome {
            if !recorded {
                return match err {
                    lez_registry_core::RegistryError::AlreadyVerified => Err(SpelError::custom(
                        E_NOT_VERIFIED,
                        "this entry is already verified; a mismatch assertion cannot erase it",
                    )),
                    _ => Err(SpelError::custom(E_BAD_CLAIM, "no claim to rebuild")),
                };
            }
        }

        store_entry(&mut entry.account, &e)?;

        if recorded {
            // `global` and `clock` are returned unmodified rather than dropped.
            // The post-state must carry every account the instruction declares --
            // omitting one is refused by the runtime with the same silent message
            // as every other failure, which cost a deploy cycle to find.
            //
            // The counter deliberately does not move: neither Unreproducible nor
            // Mismatched is a verification, and total_verified must never grow
            // for one.
            return Ok(SpelOutput::execute(
                vec![entry, global, clock, attester],
                vec![],
            ));
        }

        // Only the transition counts. Re-attesting an already-verified entry adds
        // an attester, not a verified program, and a counter that grew each time
        // would overstate the registry's own coverage.
        if !was_verified {
            let mut g = GlobalAccount::try_from_slice(&global.account.data)
                .map_err(|_| SpelError::custom(E_BAD_ENTRY, "global failed to decode"))?;
            g.total_verified = g
                .total_verified
                .checked_add(1)
                .ok_or_else(|| SpelError::custom(E_BAD_ENTRY, "verified counter overflow"))?;
            let bytes = borsh::to_vec(&g)
                .map_err(|_| SpelError::custom(E_BAD_ENTRY, "global failed to encode"))?;
            write(&mut global.account, &bytes)?;
        }

        Ok(SpelOutput::execute(
            vec![entry, global, clock, attester],
            vec![],
        ))
    }

    /// Freeze an entry. One-way: no instruction in this program lowers the flag,
    /// which is the only thing that makes immutability worth anything.
    #[instruction]
    pub fn freeze(
        ctx: ProgramContext,
        #[account(pda = arg("entry_seed"))] mut entry: AccountWithMetadata,
        #[account(signer)] author: AccountWithMetadata,
        entry_seed: [u8; 32],
    ) -> SpelResult {
        let _ = entry_seed;
        if entry.account.program_owner != ctx.self_program_id {
            return Err(SpelError::custom(
                E_NOT_ANCHORED,
                "no entry at this address",
            ));
        }
        let mut e = load_entry(&entry.account)?;
        e.freeze(author.account_id.value())
            .map_err(|err| match err {
                lez_registry_core::RegistryError::Immutable => {
                    SpelError::custom(E_IMMUTABLE, "entry is already frozen")
                }
                _ => SpelError::custom(E_NOT_AUTHOR, "signer is not this entry's author"),
            })?;
        store_entry(&mut entry.account, &e)?;
        Ok(SpelOutput::execute(vec![entry, author], vec![]))
    }
}
