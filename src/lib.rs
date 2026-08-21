// SPDX-License-Identifier: MIT OR Apache-2.0
//! Registry entry types, address derivation and build-verification logic for
//! the LEZ program registry.
//!
//! WHY THIS IS A SEPARATE CRATE WITH NO I/O
//!
//! The on-chain program, the CLI and the SDK all have to agree on what an entry
//! address is and what "verified" means. If each computed it, they would agree
//! until the day they did not, and the day they did not would be the day a user
//! trusted a wrong answer. So the rules live here once, `no_std`-compatible,
//! and the program links the same code the CLI does.
//!
//! WHAT VERIFICATION IS, AND WHAT IT IS NOT
//!
//! Verification proves one thing: that rebuilding a stated source at a stated
//! commit with a stated toolchain produces the bytecode that is deployed. It
//! does **not** prove the program is safe, that the author is who they say, or
//! that the source does what its README claims. The type system says so — see
//! [`Verification`] — because a UI that renders "verified" as a green tick
//! teaches users the opposite.
//!
//! AND A HASH MATCH IS NOT PROVENANCE
//!
//! Solana's verified builds were subverted by a repository whose `build.rs`
//! downloaded the already-deployed binary and emitted it, so the rebuild matched
//! trivially while the source proved nothing. Any registry that only compares
//! hashes inherits that hole. [`BuildEnvironment`] therefore records whether the
//! rebuild ran with the network disabled, and a claim built with network access
//! can never reach [`VerificationState::Verified`] — it stops at
//! [`VerificationState::Unreproducible`], which is a different word on screen.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use borsh::{BorshDeserialize, BorshSerialize};
use sha2::{Digest, Sha256};

/// A LEZ program identifier: the RISC0 ImageID of its guest ELF.
///
/// Not a hash of the deployed file — see [`deployment_digest`] for why the two
/// are different numbers and what each one is good for.
pub type ProgramId = [u8; 32];

/// A 32-byte account identifier.
pub type AccountId = [u8; 32];

/// A point in chain time: the block, and the millisecond timestamp the sequencer
/// wrote for it.
///
/// **Never supplied by a caller.** A zkVM guest has no clock, which invites the
/// shortcut of letting the client pass a number — and a client-supplied
/// timestamp is a client-supplied lie waiting to happen, in a registry whose
/// adoption is measured by when things were registered. LEZ maintains clock
/// accounts at fixed addresses (`/LEZ/ClockProgramAccount/00000{01,10,50}`)
/// written by the sequencer; the program reads one of those as an input account
/// and checks its owner before believing a byte of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Stamp {
    pub block: u64,
    /// Milliseconds since the Unix epoch, as the sequencer records it.
    pub millis: u64,
}

/// A Logos Storage content identifier. The registry stores only this on chain;
/// the IDL JSON itself lives in Storage, because an IDL is kilobytes and an
/// on-chain kilobyte is paid for by everyone who syncs the zone.
pub type ContentId = [u8; 32];

/// Every way the registry refuses. Each maps to one documented on-chain error
/// code; none of them is "an error occurred".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryError {
    /// A required field was empty.
    Empty,
    /// A field exceeded the length the account layout reserves for it.
    TooLong,
    /// The caller is not the account recorded as this entry's author.
    NotAuthor,
    /// The entry was frozen and can no longer change.
    Immutable,
    /// A source claim was offered for bytecode that is not what is deployed.
    ArtifactMismatch,
    /// A mismatch verdict was submitted against an entry that is already
    /// verified. Refused: see `record_rebuild`.
    AlreadyVerified,
    /// The rebuild ran with network access, so a hash match proves nothing.
    NetworkedBuild,
}

type Result<T> = core::result::Result<T, RegistryError>;

/// Field limits. They exist in one place so the account layout, the CLI's
/// validation and the SDK's types cannot drift apart.
pub const MAX_NAME: usize = 64;
pub const MAX_VERSION: usize = 32;
pub const MAX_DESCRIPTION: usize = 512;
pub const MAX_TAGS: usize = 8;
pub const MAX_TAG: usize = 24;
pub const MAX_SOURCE_URL: usize = 256;
pub const MAX_TOOLCHAIN: usize = 64;

/// Who published an entry, which is the single most load-bearing distinction in
/// the registry.
///
/// A program's own deploy authority saying "this is my program" is a different
/// claim from a stranger saying it, and rendering them alike is how a registry
/// becomes an attack surface. The registry records which one it is and refuses
/// to rank them; deciding whether a third party is trustworthy is curation, and
/// curation is explicitly out of scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Publisher {
    /// Published by an account that proved control of the deployed program.
    Canonical,
    /// Published by anyone else. Not lesser, not greater — different.
    ThirdParty,
}

/// How a rebuild was performed. The network flag is not bookkeeping: it is the
/// difference between a hash match that means something and one that does not.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct BuildEnvironment {
    /// The pinned builder image, by digest rather than by tag. A tag is a moving
    /// target and a verification pinned to a moving target verifies nothing.
    pub image_digest: [u8; 32],
    /// Guest toolchain version, e.g. "1.88.0".
    pub toolchain: String,
    /// Whether the rebuild ran with the network unavailable. False makes the
    /// build unreproducible by definition, because the build may have fetched
    /// the answer.
    pub network_disabled: bool,
}

impl BuildEnvironment {
    pub fn validate(&self) -> Result<()> {
        if self.toolchain.is_empty() {
            return Err(RegistryError::Empty);
        }
        if self.toolchain.len() > MAX_TOOLCHAIN {
            return Err(RegistryError::TooLong);
        }
        Ok(())
    }
}

/// What an author asserts about where their program came from.
///
/// The repository URL is a **liveness dependency on a third party**: GitHub can
/// disappear and the claim becomes uncheckable. So the claim also carries
/// `source_digest`, a hash over the source tree as built. A mirror that
/// reproduces that digest is as good as the original host, which turns the URL
/// from a dependency into a hint.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct SourceClaim {
    pub repository: String,
    /// The commit the bytecode was built from, as raw bytes so the format is
    /// not assumed to be git's.
    pub commit: [u8; 20],
    /// Hash over the source tree actually compiled. Survives the repository.
    pub source_digest: [u8; 32],
    pub build: BuildEnvironment,
}

impl SourceClaim {
    pub fn validate(&self) -> Result<()> {
        if self.repository.is_empty() {
            return Err(RegistryError::Empty);
        }
        if self.repository.len() > MAX_SOURCE_URL {
            return Err(RegistryError::TooLong);
        }
        self.build.validate()
    }
}

/// What the registry knows about an entry's build, as four states a UI must be
/// able to tell apart.
///
/// There is no `Stale`, and its absence is a design decision rather than an
/// omission. Registries for account-addressed chains need one, because the code
/// at an address can be replaced under a verification that was honestly earned.
/// Here an entry is keyed by its ImageID, so the bytecode *is* the key: nothing
/// can be swapped beneath it, and a verification that was ever true stays true.
/// Modelling a state that cannot be reached would mean writing branches no test
/// can exercise and a UI legend for something a user will never see.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum VerificationState {
    /// No source has been claimed. The honest default.
    Unclaimed,
    /// A source is claimed and nobody has rebuilt it yet.
    Claimed,
    /// Someone rebuilt the claim, offline, and the bytecode matched.
    Verified,
    /// The rebuild happened but cannot count — it ran with the network up, so
    /// the build could have fetched the artifact it was meant to reproduce.
    Unreproducible,
    /// A claim was rebuilt and the bytecode did **not** match.
    ///
    /// This is not `Unclaimed` with extra steps, and collapsing the two would be
    /// the single most misleading thing this type could do. "Nobody has said
    /// where the source is" and "somebody said, and it does not reproduce" are
    /// opposite signals: the first is a gap, the second is evidence against.
    Mismatched,
}

/// The verification attached to an entry, kept as its own type so a state and
/// the claim it is about cannot be set independently and drift.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Verification {
    pub state: VerificationState,
    /// The claim this verification concerns, if any.
    pub claim: Option<SourceClaim>,
    /// Block and timestamp of the last rebuild recorded against this entry,
    /// whatever its outcome.
    ///
    /// Read from the sequencer's clock account, not from the caller. Exposed so
    /// a client can judge staleness itself rather than being told a verdict with
    /// no date on it.
    pub last_checked: Option<Stamp>,
    /// Who attested to the rebuild.
    ///
    /// A chain cannot compile anything, so this registry cannot verify a build;
    /// it can only record that a named party says they did. Storing the party is
    /// what keeps `Verified` from being a claim the registry itself is making and
    /// cannot support. `None` until a rebuild is recorded.
    pub attester: Option<AccountId>,
}

impl Verification {
    pub const fn unclaimed() -> Self {
        Self {
            state: VerificationState::Unclaimed,
            claim: None,
            last_checked: None,
            attester: None,
        }
    }

    /// Attach a source claim. The result is `Claimed`: asserting is not proving,
    /// and a registry that conflated the two would let anyone mint trust by
    /// typing a URL.
    pub fn claim(source: SourceClaim) -> Result<Self> {
        source.validate()?;
        Ok(Self {
            state: VerificationState::Claimed,
            claim: Some(source),
            last_checked: None,
            attester: None,
        })
    }

    /// Record the outcome of a rebuild.
    ///
    /// `rebuilt` is the ImageID the rebuild produced; `subject` is the entry's
    /// own key. There is deliberately no third "currently deployed" argument to
    /// reconcile against: on a content-addressed chain the key *is* the
    /// bytecode, so such an argument could only ever equal `subject`, and a
    /// parameter that can only take one value is a place for a caller to be
    /// wrong.
    pub fn record_rebuild(
        &mut self,
        rebuilt: ProgramId,
        subject: ProgramId,
        attester: AccountId,
        at: Stamp,
    ) -> Result<()> {
        let claim = match &self.claim {
            Some(c) => c,
            None => return Err(RegistryError::Empty),
        };
        // A contradicting *assertion* must not erase a demonstrated *fact*.
        //
        // `rebuilt` is whatever the caller says their rebuild produced, and the
        // chain cannot check it. If a mismatch verdict could overwrite Verified,
        // any account could un-verify any entry for the cost of one transaction,
        // by typing the wrong 32 bytes. So a verified entry keeps its state and
        // the caller is told why.
        //
        // Invalidation still exists and is the author's to trigger: changing the
        // source claim resets the entry to Claimed. That is the version binding
        // the design needs, and it belongs to whoever the claim is about rather
        // than to any passer-by.
        if rebuilt != subject && matches!(self.state, VerificationState::Verified) {
            return Err(RegistryError::AlreadyVerified);
        }

        // Every outcome below is dated and attributed, including the failures.
        // An undated verdict cannot be judged for staleness, and an unattributed
        // one cannot be judged at all.
        self.last_checked = Some(at);
        self.attester = Some(attester);
        if rebuilt != subject {
            // Recorded, not refused. "Somebody published a source that does not
            // reproduce" is a stronger negative signal than "nobody published
            // one", and a registry that returned an error here would leave the
            // entry looking merely unverified.
            self.state = VerificationState::Mismatched;
            return Err(RegistryError::ArtifactMismatch);
        }
        if !claim.build.network_disabled {
            // Set the state as well as returning the error: a build that could
            // have downloaded the very artifact it was meant to reproduce must
            // leave a mark, or the next caller reads `Claimed` and cannot tell
            // this was already attempted.
            self.state = VerificationState::Unreproducible;
            return Err(RegistryError::NetworkedBuild);
        }
        self.state = VerificationState::Verified;
        Ok(())
    }

    /// True only for a claim rebuilt offline against this entry's own bytecode.
    /// Once true it stays true: the subject cannot change underneath it.
    ///
    /// Read it with [`Self::attester`] in hand. This answers "was a matching
    /// offline rebuild recorded", not "is this program trustworthy", and those
    /// differ by exactly how much you trust whoever recorded it.
    pub const fn is_verified(&self) -> bool {
        matches!(self.state, VerificationState::Verified)
    }

    /// Whether the only party vouching for this build is the party that shipped
    /// it.
    ///
    /// Not an accusation and not a failure: an author who rebuilds their own
    /// release reproducibly has done something useful. It is weaker evidence than
    /// an independent rebuild, and a UI that rendered both as one tick would be
    /// hiding the difference that matters most.
    pub fn is_self_attested(&self, author: &AccountId) -> bool {
        self.is_verified() && self.attester.as_ref() == Some(author)
    }
}

/// One registry entry.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Entry {
    pub program_id: ProgramId,
    /// When this entry was registered, read from the sequencer's clock.
    ///
    /// The adoption story a registry tells — how many programs, registered over
    /// how long — is only as good as this field, so it is not a number the
    /// registering client gets to choose.
    pub registered_at: Stamp,
    pub name: String,
    pub version: String,
    pub author: AccountId,
    pub publisher: Publisher,
    pub description: String,
    pub tags: Vec<String>,
    /// Logos Storage identifier for the IDL JSON, if one is published.
    pub idl: Option<ContentId>,
    pub verification: Verification,
    /// Once true, nothing about this entry may change again. One-way: there is
    /// no instruction that lowers it, which is what makes it worth anything.
    pub frozen: bool,
}

impl Entry {
    pub fn new(
        program_id: ProgramId,
        registered_at: Stamp,
        name: String,
        version: String,
        author: AccountId,
        publisher: Publisher,
    ) -> Result<Self> {
        let e = Self {
            program_id,
            registered_at,
            name,
            version,
            author,
            publisher,
            description: String::new(),
            tags: Vec::new(),
            idl: None,
            verification: Verification::unclaimed(),
            frozen: false,
        };
        e.validate()?;
        Ok(e)
    }

    pub fn validate(&self) -> Result<()> {
        if self.name.is_empty() || self.version.is_empty() {
            return Err(RegistryError::Empty);
        }
        if self.name.len() > MAX_NAME
            || self.version.len() > MAX_VERSION
            || self.description.len() > MAX_DESCRIPTION
            || self.tags.len() > MAX_TAGS
        {
            return Err(RegistryError::TooLong);
        }
        if self.tags.iter().any(|t| t.is_empty() || t.len() > MAX_TAG) {
            return Err(RegistryError::TooLong);
        }
        Ok(())
    }

    /// Only the recorded author may change an entry, and only while it is not
    /// frozen. Both checks live here so no instruction can forget one.
    pub fn require_writable_by(&self, caller: &AccountId) -> Result<()> {
        if self.frozen {
            return Err(RegistryError::Immutable);
        }
        if &self.author != caller {
            return Err(RegistryError::NotAuthor);
        }
        Ok(())
    }

    /// Freeze. One-way by construction: nothing in this crate sets it back.
    pub fn freeze(&mut self, caller: &AccountId) -> Result<()> {
        self.require_writable_by(caller)?;
        self.frozen = true;
        Ok(())
    }
}

/// Registry-wide state, as a singleton.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct GlobalState {
    pub total_entries: u64,
    pub total_verified: u64,
}

/// Domain separators. Distinct prefixes stop one kind of address ever colliding
/// with another, which a bare hash of concatenated fields does not.
pub const ENTRY_SEED: &[u8] = b"lez-registry:entry:v1";
pub const GLOBAL_SEED: &[u8] = b"lez-registry:global:v1";
/// Seed prefix for the sequence directory.
pub const SEQ_SEED: &[u8] = b"lez-registry:seq:v1";

/// The seed for an entry's account, derived from the program id alone.
///
/// From the program id alone, deliberately: a client resolving a program must be
/// able to compute the address it needs without asking an index. An index that
/// is required for lookup is a service the registry would then depend on, and
/// "no mandatory external services" is a requirement rather than a preference.
///
/// The publisher is *not* in the seed. One canonical entry per program is the
/// point; third-party entries live at [`third_party_entry_seed`], keyed by the
/// publisher, so a stranger can never occupy the canonical address.
#[must_use]
pub fn entry_seed(program_id: &ProgramId) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(ENTRY_SEED);
    h.update(program_id);
    h.finalize().into()
}

/// The seed for a third-party entry about someone else's program.
#[must_use]
pub fn third_party_entry_seed(program_id: &ProgramId, publisher: &AccountId) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(ENTRY_SEED);
    h.update(b":third-party:");
    h.update(program_id);
    h.update(publisher);
    h.finalize().into()
}

/// The singleton global-state seed.
#[must_use]
pub fn global_seed() -> [u8; 32] {
    // NOT a hash of GLOBAL_SEED, though it was one until the SDK derived an
    // address the chain had nothing at. The program declares this account as
    // `pda = [literal("lez-registry:global:v1")]`, and SPEL turns a literal seed
    // into 32 bytes by right-padding with zeroes rather than by hashing. Hashing
    // here produced a perfectly well-formed address that simply was not the one
    // the program writes to -- the failure mode a derivation bug always has.
    let mut seed = [0u8; 32];
    seed[..GLOBAL_SEED.len()].copy_from_slice(GLOBAL_SEED);
    seed
}

/// The content address of a **deployment**: `SHA256(u32_le(len) ‖ bytecode)`.
///
/// THIS IS NOT THE PROGRAM ID, AND THE DIFFERENCE COST US A BUG
///
/// It is what the deploy transaction hashes to — deploying
/// `lez_registry.bin` produced transaction
/// `447263aa8e01b2d3b5cbb4c603b61194d5909b0158f3901e88d282202d9cb604`, which is
/// exactly this function's output on that file. Useful, and *not* the identifier
/// a program is called by.
///
/// The **ProgramId is the RISC0 ImageID**, derived from the ELF's own structure
/// rather than by hashing the packaged file, and for the same binary it is
/// `f6a765c2a316da6344de3cccfb39868732a4102f93ff9f0cc8d55be2d7c9e577`. A
/// registry keyed on the wrong one of these would resolve nothing, so this crate
/// names them apart and never computes the ImageID itself: read it with
/// `spel program-id`, or with risc0's `binfmt`, which is where the definition
/// lives.
///
/// Keeping the deployment digest is still worth it — it is how you check that
/// the file you hold is the file that was deployed, independently of any
/// explorer.
#[must_use]
pub fn deployment_digest(bytecode: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update((bytecode.len() as u32).to_le_bytes());
    h.update(bytecode);
    h.finalize().into()
}

/// Address of the sequence-directory slot for the `n`th registration.
///
/// The chain offers no way to enumerate accounts: there is no
/// `getProgramAccounts`, no accounts-by-owner, nothing (probed against the live
/// sequencer — every such method returns `Method not found`). Resolution does not
/// need one, because an entry's address is a function of the program id. But
/// *discovery* does: without enumeration, an indexer that wants to list the
/// registry has no way to learn which program ids exist.
///
/// So the registry keeps its own directory. Registration `n` writes the program
/// id here, and `n` is simply the value `total_entries` had at the time. An
/// indexer reads that counter and walks `0..total`, deriving every address and
/// asking the chain for nothing it cannot compute first.
///
/// This is a directory, not an authority. Everything it yields is re-derivable
/// and re-checkable against the entry itself.
#[must_use]
pub fn seq_seed(n: u64) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(SEQ_SEED);
    h.update(n.to_le_bytes());
    h.finalize().into()
}

/// One slot of the sequence directory: which program registered `n`th.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct DirectorySlot {
    pub program_id: ProgramId,
}
