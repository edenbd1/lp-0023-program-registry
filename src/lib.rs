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

/// A LEZ program identifier.
///
/// On LEZ v0.3 a program is an **account**: the header account its deployer
/// chose and signed for, which calls dispatch to and PDAs derive from. The
/// registry keys entries by that account id. The bytecode behind it is named
/// separately by its [`ImageId`], which the runtime recomputes from the segment
/// chain and records in the header.
pub type ProgramId = [u8; 32];

/// The RISC0 ImageID of a guest ELF, as the 32 bytes the loader stores in a
/// program header (`[u32; 8]`, little-endian words).
///
/// Not a hash of the deployed file: see [`deployment_digest`].
pub type ImageId = [u8; 32];

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
pub type ContentId = [u8; CONTENT_ID_LEN];

/// Length of a Logos Storage content identifier, in bytes.
///
/// **38, not 32**, and the difference is not academic: a real identifier decodes
/// to `01` (CIDv1) ‖ a varint codec ‖ a multihash, and the multihash carries its
/// own code and length before the 32-byte digest. Two live examples from a Logos
/// Storage node —
/// `zDvZRwzm9Ni32iyvoy868YVrBHFz8vo3oZirrCKhFMMn9aXuinfQ` and
/// `zDzSvJTf49uAN2BZhoMhZmDVSv7KDVeUVXTXvDc6QNnU1TKJYD55` — decode to 38 bytes
/// each, with different codecs (`819a03`, `839a03`).
///
/// A 32-byte field would have held the digest and silently dropped the codec, so
/// nothing could reconstruct the identifier well enough to fetch anything. The
/// bytes are stored whole and rendered as multibase base58btc (a leading `z`),
/// which is the form a Storage node accepts.
pub const CONTENT_ID_LEN: usize = 38;

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
    /// The deployer's entry: signed by the program's own header account, the
    /// key the loader required to create the program. On LEZ v0.3 that is a
    /// fact the registry checks at registration, not an assertion.
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

/// What the registry knows about an entry's build, as states a UI must be able
/// to tell apart.
///
/// LEZ v0.3 made programs account-addressed: a mutable header can be repointed
/// at new bytecode after a verification was honestly earned. So there is a
/// `Stale` state, reached only by [`Entry::sync_header`] observing that the
/// header's ImageID is no longer the one the verdict was about. A frozen
/// (immutable) header can never go stale, which is why the entry records the
/// header's `immutable` flag next to the verdict.
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
    /// A verdict was recorded, then the program's header was repointed at
    /// different bytecode. The verdict is kept for the record but is about an
    /// image that no longer runs, so it must be rebuilt against the new one.
    Stale,
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
    /// `rebuilt` is the ImageID the rebuild produced; `subject` is the ImageID
    /// the program's header records on chain. The on-chain program only passes
    /// a `subject` it has just checked against the header's loader shard, so
    /// the comparison is between a caller's attestation and a chain fact.
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
    /// The program's header account id: what calls dispatch to.
    pub program_id: ProgramId,
    /// The ImageID the program's header recorded when this entry last looked,
    /// checked on chain against the header's loader shard.
    pub image_id: ImageId,
    /// Whether that header was immutable. A frozen header can never be
    /// repointed, so a verdict about it can never go stale.
    pub header_immutable: bool,
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
            image_id: [0u8; 32],
            header_immutable: false,
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

    /// Record the program header this entry is about, as read from chain.
    #[must_use]
    pub fn with_header(mut self, image_id: ImageId, immutable: bool) -> Self {
        self.image_id = image_id;
        self.header_immutable = immutable;
        self
    }

    /// Bring the entry in line with the program's header as it is on chain now.
    ///
    /// Returns whether the ImageID changed. When it did, any verdict about the
    /// old image (verified, mismatched, unreproducible) becomes
    /// [`VerificationState::Stale`]: it was true of bytecode that no longer
    /// runs at this program id. The claim itself is kept, so a rebuild against
    /// the new image can be recorded without the author re-claiming.
    ///
    /// Allowed on a frozen entry. Freezing stops the *author's* assertions from
    /// changing; the header is a chain fact, and refusing to record it would let
    /// a frozen entry keep showing a verdict about replaced bytecode.
    pub fn sync_header(&mut self, image_id: ImageId, immutable: bool) -> bool {
        let changed = self.image_id != image_id;
        if changed {
            self.image_id = image_id;
            if matches!(
                self.verification.state,
                VerificationState::Verified
                    | VerificationState::Mismatched
                    | VerificationState::Unreproducible
            ) {
                self.verification.state = VerificationState::Stale;
            }
        }
        self.header_immutable = immutable;
        changed
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
    /// How many registrations the directory holds, which is also the index the
    /// next registration must name.
    ///
    /// There is no verified counter. On v0.3 an apply sees one shard, so the
    /// global account cannot know what verdict an entry's apply just reached;
    /// a counter kept from caller-declared transitions would be one more thing
    /// a caller could make wrong. Indexers count verified entries from the walk.
    pub total_entries: u64,
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

/// One slot of the sequence directory: what registered `n`th, and where it lives.
///
/// The seed is stored as well as the program id, and it has to be. A program id
/// alone only leads back to the *canonical* entry, so an indexer walking the
/// directory would silently skip every third-party entry — and third-party
/// entries are exactly the ones a registry must not be able to hide. Found by
/// walking a registry that held one and getting the canonical entry back twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct DirectorySlot {
    pub program_id: ProgramId,
    /// The seed of the entry account this registration created.
    pub entry_seed: [u8; 32],
}

/// Decode a multibase base58btc string — the `z…` form a Logos Storage node
/// hands you — into content-identifier bytes.
///
/// `no_std` and allocation-free: the output is fixed-size because a content
/// identifier is. Returns `None` rather than a partial result on anything
/// malformed, since a half-decoded identifier fetches the wrong object or
/// nothing, and both are worse than an error.
#[must_use]
pub fn decode_content_id(multibase: &str) -> Option<ContentId> {
    const ALPHABET: &[u8; 58] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    let bytes = multibase.as_bytes();
    // The leading `z` is multibase's tag for base58btc. Anything else is a
    // different encoding and guessing would be worse than refusing.
    if bytes.first() != Some(&b'z') {
        return None;
    }
    let mut out = [0u8; CONTENT_ID_LEN];
    let mut len = 0usize;
    for &c in &bytes[1..] {
        let mut carry = ALPHABET.iter().position(|a| *a == c)?;
        for byte in out[..len].iter_mut().rev() {
            carry += 58 * (*byte as usize);
            *byte = (carry & 0xFF) as u8;
            carry >>= 8;
        }
        while carry > 0 {
            if len >= CONTENT_ID_LEN {
                // Longer than a content identifier can be. Refuse rather than
                // truncate: a truncated identifier is a valid-looking one that
                // points somewhere else.
                return None;
            }
            // Shift the accumulated bytes right to make room at the front.
            let mut i = len;
            while i > 0 {
                out[i] = out[i - 1];
                i -= 1;
            }
            out[0] = (carry & 0xFF) as u8;
            len += 1;
            carry >>= 8;
        }
    }
    // Leading zero bytes encode as '1' and carry no value through the loop above.
    for &c in &bytes[1..] {
        if c != b'1' {
            break;
        }
        if len >= CONTENT_ID_LEN {
            return None;
        }
        let mut i = len;
        while i > 0 {
            out[i] = out[i - 1];
            i -= 1;
        }
        out[0] = 0;
        len += 1;
    }
    if len != CONTENT_ID_LEN {
        return None;
    }
    // Every Logos Storage identifier is a CIDv1, so the first byte is 0x01.
    // Checking it turns a plausible-but-wrong string into an error here rather
    // than into a fetch that returns nothing later.
    if out[0] != 0x01 {
        return None;
    }
    Some(out)
}

/// The registry's instructions, as the v0.3 guest decodes them.
///
/// One definition, borsh-encoded, shared by the guest, the SDK and the CLI, so
/// a transaction the SDK builds is by construction one the guest decodes. The
/// IDL in `idl/lez_registry.idl.json` describes this enum in SPEL's IDL format,
/// and `tests/idl_drift.rs` fails if the two disagree.
///
/// Variants are append-only: borsh writes the variant index as the first byte.
pub mod instruction {
    use super::*;

    /// Which entry an instruction is about: the program, and for a third-party
    /// entry its publisher. `publisher: None` is the deployer's entry.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
    pub struct EntryKey {
        pub program: ProgramId,
        pub publisher: Option<AccountId>,
    }

    impl EntryKey {
        /// The PDA seed of the entry account this key names.
        #[must_use]
        pub fn seed(&self) -> [u8; 32] {
            match &self.publisher {
                None => entry_seed(&self.program),
                Some(p) => third_party_entry_seed(&self.program, p),
            }
        }
    }

    /// A program header as the client read it from chain. The guest checks it
    /// against the header's loader shard before trusting a byte of it.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
    pub struct HeaderView {
        pub image_id: ImageId,
        pub immutable: bool,
    }

    #[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
    pub enum Instruction {
        /// Create the global counter. Permissionless; refuses a second time.
        /// Accounts: `[global]`.
        Initialize,
        /// Register a program. Accounts:
        /// `[entry, global, directory, program@loader, author]`.
        ///
        /// `key.publisher` is `None` for the deployer's entry, which requires
        /// `author` to be the program's own header account, signing. Otherwise
        /// it must equal `author`, who signs.
        Register {
            key: EntryKey,
            header: HeaderView,
            seq_index: u64,
            at: Stamp,
            name: String,
            version: String,
            description: String,
            tags: Vec<String>,
            /// Multibase `z...` Logos Storage identifier of the IDL, or empty.
            idl: String,
            /// The code pointer: source repository, commit and build config.
            source: Option<SourceClaim>,
        },
        /// Change version, description and IDL pointer. Author only.
        /// Accounts: `[entry, author]`.
        Update {
            key: EntryKey,
            version: String,
            description: String,
            idl: String,
        },
        /// Replace the source claim; resets the verdict to `Claimed`.
        /// Author only. Accounts: `[entry, author]`.
        ClaimSource { key: EntryKey, claim: SourceClaim },
        /// Record a rebuild's ImageID against the header on chain. Anyone may
        /// attest; the attester is recorded. Accounts:
        /// `[entry, program@loader, attester]`.
        RecordRebuild {
            key: EntryKey,
            header: HeaderView,
            rebuilt: ImageId,
            at: Stamp,
        },
        /// Re-read the header; a changed ImageID makes a verdict `Stale`.
        /// Permissionless. Accounts: `[entry, program@loader]`.
        SyncHeader { key: EntryKey, header: HeaderView },
        /// Freeze the entry's metadata forever. Author only.
        /// Accounts: `[entry, author]`.
        Freeze { key: EntryKey },
    }

    impl Instruction {
        /// Variant names in declaration order, snake_case as the IDL names them.
        pub const NAMES: [&'static str; 7] = [
            "initialize",
            "register",
            "update",
            "claim_source",
            "record_rebuild",
            "sync_header",
            "freeze",
        ];

        #[must_use]
        pub const fn index(&self) -> u8 {
            match self {
                Self::Initialize => 0,
                Self::Register { .. } => 1,
                Self::Update { .. } => 2,
                Self::ClaimSource { .. } => 3,
                Self::RecordRebuild { .. } => 4,
                Self::SyncHeader { .. } => 5,
                Self::Freeze { .. } => 6,
            }
        }
    }

    /// How far behind the including block a stamp may be, in blocks and in
    /// milliseconds. The guest sets these as the transaction's validity
    /// windows, which the runtime enforces: a stamp is never in the future and
    /// never more than this much in the past.
    pub const STAMP_BLOCK_WINDOW: u64 = 600;
    pub const STAMP_MILLIS_WINDOW: u64 = 600_000;
}
