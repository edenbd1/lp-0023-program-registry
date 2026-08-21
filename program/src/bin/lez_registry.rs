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

/// A registry entry, stored as the core crate defines it. The on-chain layout
/// is `lez_registry_core::Entry` verbatim rather than a parallel struct, so a
/// client that decodes an account gets the type the program wrote.
#[account_type]
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug)]
pub struct EntryAccount {
    pub entry: Vec<u8>,
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

    fn store_entry(account: &mut Account, entry: &lez_registry_core::Entry) -> Result<(), SpelError> {
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
        #[account(init, pda = [literal("lez-registry:global:v1")])]
        mut global: AccountWithMetadata,
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
        #[account(mut, pda = [literal("lez-registry:global:v1")])]
        mut global: AccountWithMetadata,
        #[account(signer)] author: AccountWithMetadata,
        entry_seed: [u8; 32],
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
            lez_registry_core::Publisher::Canonical => {
                lez_registry_core::entry_seed(&program_id)
            }
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

        // `init` already refuses an existing account, so a duplicate lands here
        // rather than silently overwriting someone's entry.
        if !entry.account.data.is_empty() {
            return Err(SpelError::custom(
                E_ALREADY_REGISTERED,
                "an entry already exists at this address",
            ));
        }

        let mut e = lez_registry_core::Entry::new(program_id, name, version, author_id, publisher)
            .map_err(|_| SpelError::custom(E_BAD_ENTRY, "entry fields are empty or too long"))?;
        e.description = description;
        e.validate()
            .map_err(|_| SpelError::custom(E_BAD_ENTRY, "entry fields are empty or too long"))?;
        store_entry(&mut entry.account, &e)?;

        let mut g = GlobalAccount::try_from_slice(&global.account.data)
            .map_err(|_| SpelError::custom(E_BAD_ENTRY, "global failed to decode"))?;
        g.total_entries = g
            .total_entries
            .checked_add(1)
            .ok_or_else(|| SpelError::custom(E_BAD_ENTRY, "entry counter overflow"))?;
        let bytes = borsh::to_vec(&g)
            .map_err(|_| SpelError::custom(E_BAD_ENTRY, "global failed to encode"))?;
        write(&mut global.account, &bytes)?;

        Ok(SpelOutput::execute(vec![entry, global, author], vec![]))
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
            return Err(SpelError::custom(E_NOT_ANCHORED, "no entry at this address"));
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
            return Err(SpelError::custom(E_NOT_ANCHORED, "no entry at this address"));
        }
        let mut e = load_entry(&entry.account)?;
        e.freeze(author.account_id.value()).map_err(|err| match err {
            lez_registry_core::RegistryError::Immutable => {
                SpelError::custom(E_IMMUTABLE, "entry is already frozen")
            }
            _ => SpelError::custom(E_NOT_AUTHOR, "signer is not this entry's author"),
        })?;
        store_entry(&mut entry.account, &e)?;
        Ok(SpelOutput::execute(vec![entry, author], vec![]))
    }
}
