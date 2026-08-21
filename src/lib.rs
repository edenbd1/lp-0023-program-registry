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
    /// The bytecode this claim is about. Verification is **version-bound**: a
    /// claim is about one artifact and says nothing about any other.
    pub artifact: ProgramId,
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
    /// A claim was verified against an artifact that is no longer the deployed
    /// one. Not "verified", not "unverified": *stale*, and it names the artifact
    /// it was true of.
    Stale { verified_artifact: ProgramId },
}

/// The verification attached to an entry, kept as its own type so that no code
/// path can set a state without also saying which artifact it is about.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Verification {
    pub state: VerificationState,
    /// The claim this verification concerns, if any.
    pub claim: Option<SourceClaim>,
}

impl Verification {
    pub const fn unclaimed() -> Self {
        Self {
            state: VerificationState::Unclaimed,
            claim: None,
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
        })
    }

    /// Record the outcome of a rebuild.
    ///
    /// `rebuilt` is the program id the rebuild produced. It must equal the
    /// artifact the claim is about *and* the artifact deployed, and the build
    /// must have been offline. Any other combination lands somewhere that is not
    /// `Verified`, deliberately.
    pub fn record_rebuild(&mut self, rebuilt: ProgramId, deployed: ProgramId) -> Result<()> {
        let claim = match &self.claim {
            Some(c) => c,
            None => return Err(RegistryError::Empty),
        };
        if claim.artifact != deployed {
            // The claim is about something other than what is deployed. That is
            // staleness, not failure, and the two deserve different words.
            self.state = VerificationState::Stale {
                verified_artifact: claim.artifact,
            };
            return Ok(());
        }
        if rebuilt != deployed {
            return Err(RegistryError::ArtifactMismatch);
        }
        if !claim.build.network_disabled {
            self.state = VerificationState::Unreproducible;
            return Err(RegistryError::NetworkedBuild);
        }
        self.state = VerificationState::Verified;
        Ok(())
    }

    /// Called when the deployed artifact changes. Verification is bound to a
    /// version, so an upgrade cannot inherit the old one's status.
    pub fn on_artifact_changed(&mut self, new_artifact: ProgramId) {
        match &self.state {
            VerificationState::Verified => {
                let was = self.claim.as_ref().map_or([0u8; 32], |c| c.artifact);
                self.state = VerificationState::Stale {
                    verified_artifact: was,
                };
            }
            VerificationState::Claimed | VerificationState::Unreproducible => {
                let still_about_it = self
                    .claim
                    .as_ref()
                    .is_some_and(|c| c.artifact == new_artifact);
                if !still_about_it {
                    self.state = VerificationState::Unclaimed;
                    self.claim = None;
                }
            }
            VerificationState::Unclaimed | VerificationState::Stale { .. } => {}
        }
    }

    /// True only for a claim rebuilt offline against the deployed artifact.
    /// Everything else, including `Stale`, is false — because "it was verified
    /// once" is not a property a user can act on.
    pub const fn is_verified(&self) -> bool {
        matches!(self.state, VerificationState::Verified)
    }
}

/// One registry entry.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Entry {
    pub program_id: ProgramId,
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
        name: String,
        version: String,
        author: AccountId,
        publisher: Publisher,
    ) -> Result<Self> {
        let e = Self {
            program_id,
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
    let mut h = Sha256::new();
    h.update(GLOBAL_SEED);
    h.finalize().into()
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
