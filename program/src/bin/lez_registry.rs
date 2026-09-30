// SPDX-License-Identifier: MIT OR Apache-2.0
//
// The Provenance registry, on LEZ v0.3.
//
// WHAT THIS PROGRAM IS FOR
//
// A LEZ program is identified by an opaque 32-byte account id. Nothing in it
// says what the program does, who wrote it, what its interface is, or whether
// the source you are reading is the source that is running. This program is
// where those answers live, so that a wallet showing a transaction, an explorer
// decoding one, or a developer choosing a dependency can all ask the same place.
//
// HOW v0.3 SHAPES IT
//
// A v0.3 guest runs twice per effect. `plan` sees the instruction and the
// account ids with their `is_authorized` bits, never account contents, and
// emits effects. `apply` sees one effect and one shard. So every rule that
// needs data is checked in `apply`, every rule about who signed is checked in
// `plan`, and where two shards must agree (the directory index and the global
// counter) the instruction names the value and each apply refuses unless its
// own shard agrees.
//
// Two facts are read from other programs' shards with read-only effects:
// the program's header in the `program_loader` shard, which is how the
// registry checks the recorded ImageID and frozen flag on chain, and nothing
// else. Time comes from the transaction's validity windows, which the runtime
// enforces against the block that includes it.
//
// WHY THE RULES ARE NOT WRITTEN HERE
//
// Every check about an entry delegates to `lez-registry-core`, the same crate
// the CLI and the SDK link. The instruction enum lives there too, so a
// transaction the SDK builds is by construction one this program decodes.

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::{
    account::AccountId,
    program::{
        run_program, AccountMeta, BlockValidityWindow, PdaSeed, Plan, PlanInput, ProgramHeader,
        TimestampValidityWindow, PROGRAM_LOADER_ACCOUNT_ID,
    },
};
use lez_registry_core::{
    self as core_,
    instruction::{EntryKey, HeaderView, Instruction, STAMP_BLOCK_WINDOW, STAMP_MILLIS_WINDOW},
    DirectorySlot, Entry, GlobalState, ImageId, RegistryError, SourceClaim, Stamp,
    Verification,
};

/// What one apply does to one shard. Planned from the instruction, applied
/// against the shard's current bytes.
#[derive(BorshSerialize, BorshDeserialize)]
enum Effect {
    /// Global account: create the counter at zero.
    InitGlobal,
    /// Global account: refuse unless the counter is `expect`, then advance it.
    NextSlot { expect: u64 },
    /// Directory slot: write once.
    WriteSlot(DirectorySlot),
    /// Entry account: write once.
    CreateEntry(Box<Entry>),
    /// Program header (loader shard, read-only): refuse unless it is exactly this.
    HeaderIs(HeaderView),
    UpdateMeta {
        author: [u8; 32],
        version: String,
        description: String,
        idl: Option<core_::ContentId>,
    },
    ClaimSource {
        author: [u8; 32],
        claim: SourceClaim,
    },
    RecordRebuild {
        attester: [u8; 32],
        header: HeaderView,
        rebuilt: ImageId,
        at: Stamp,
    },
    SyncHeader(HeaderView),
    Freeze {
        author: [u8; 32],
    },
}

// Error codes, as the IDL lists them. A guest failure is a panic, and the
// sequencer reports its message, so each one starts with its code.
const E_BAD_ENTRY: &str = "E9001";
const E_NOT_INITIALIZED: &str = "E9002";
const E_NOT_AUTHOR: &str = "E9003";
const E_IMMUTABLE: &str = "E9004";
const E_WRONG_ACCOUNT: &str = "E9005";
const E_BAD_CLAIM: &str = "E9006";
const E_ALREADY_VERIFIED: &str = "E9007";
const E_ALREADY_REGISTERED: &str = "E9008";
const E_HEADER_MISMATCH: &str = "E9009";
const E_NOT_DEPLOYER: &str = "E9010";
const E_NOT_SIGNED: &str = "E9011";
const E_SLOT_MISMATCH: &str = "E9012";

fn main() {
    run_program(plan, apply)
}

fn pda(me: AccountId, seed: [u8; 32]) -> AccountId {
    AccountId::for_public_pda(&me, &PdaSeed::new(seed))
}

fn expect_at(account: &AccountMeta, expected: AccountId, what: &str) {
    assert!(
        account.account_id == expected,
        "{E_WRONG_ACCOUNT} {what} is not the address this instruction derives"
    );
}

fn expect_signed(account: &AccountMeta, what: &str) {
    assert!(account.is_authorized, "{E_NOT_SIGNED} {what} must sign");
}

fn expect_header(account: &AccountMeta, key: &EntryKey) {
    assert!(
        account.account_id.value() == &key.program
            && account.program_account_id == PROGRAM_LOADER_ACCOUNT_ID,
        "{E_WRONG_ACCOUNT} the header input must be the program's loader shard"
    );
}

fn the<const N: usize>(input: &PlanInput) -> [AccountMeta; N] {
    <[AccountMeta; N]>::try_from(input.accounts.clone()).unwrap_or_else(|_| {
        panic!("{E_WRONG_ACCOUNT} expected {N} accounts")
    })
}

/// The runtime-enforced bound on a caller-supplied stamp: the including block
/// must be at or after it, and no more than the window after it.
fn stamp_windows(plan: &mut Plan, at: Stamp) {
    let blocks: BlockValidityWindow = (
        Some(at.block),
        Some(at.block.saturating_add(STAMP_BLOCK_WINDOW)),
    )
        .try_into()
        .unwrap_or_else(|_| panic!("{E_BAD_ENTRY} stamp block window is empty"));
    let millis: TimestampValidityWindow = (
        Some(at.millis),
        Some(at.millis.saturating_add(STAMP_MILLIS_WINDOW)),
    )
        .try_into()
        .unwrap_or_else(|_| panic!("{E_BAD_ENTRY} stamp time window is empty"));
    plan.block_window(blocks);
    plan.timestamp_window(millis);
}

fn decode_idl(idl: &str) -> Option<core_::ContentId> {
    if idl.is_empty() {
        None
    } else {
        Some(core_::decode_content_id(idl).unwrap_or_else(|| {
            panic!("{E_BAD_ENTRY} idl is not a multibase base58btc content identifier")
        }))
    }
}

fn plan(input: &PlanInput, ix: Instruction) -> Plan {
    let me = input.self_account_id;
    let mut plan = Plan::new(input);
    match ix {
        Instruction::Initialize => {
            let [global] = the::<1>(input);
            expect_at(&global, pda(me, core_::global_seed()), "global");
            plan.effect(&global, &Effect::InitGlobal);
        }
        Instruction::Register {
            key,
            header,
            seq_index,
            at,
            name,
            version,
            description,
            tags,
            idl,
            source,
        } => {
            let [entry, global, directory, program, author] = the::<5>(input);
            expect_at(&entry, pda(me, key.seed()), "entry");
            expect_at(&global, pda(me, core_::global_seed()), "global");
            expect_at(&directory, pda(me, core_::seq_seed(seq_index)), "directory");
            expect_header(&program, &key);
            expect_signed(&author, "author");
            let author_id = *author.account_id.value();
            let publisher = match key.publisher {
                // The deployer's entry. The loader refused to create this
                // program's header unless this same account signed, so its
                // signature here is the deploy authority's.
                None => {
                    assert!(
                        author_id == key.program,
                        "{E_NOT_DEPLOYER} a deployer entry must be signed by the program's own header account"
                    );
                    core_::Publisher::Canonical
                }
                Some(p) => {
                    assert!(
                        p == author_id,
                        "{E_WRONG_ACCOUNT} a third-party entry is keyed by its signer"
                    );
                    core_::Publisher::ThirdParty
                }
            };
            let mut e = Entry::new(key.program, at, name, version, author_id, publisher)
                .unwrap_or_else(|_| panic!("{E_BAD_ENTRY} entry fields are empty or too long"))
                .with_header(header.image_id, header.immutable);
            e.description = description;
            e.tags = tags;
            e.idl = decode_idl(&idl);
            if let Some(claim) = source {
                e.verification = Verification::claim(claim)
                    .unwrap_or_else(|_| panic!("{E_BAD_CLAIM} source claim is empty or too long"));
            }
            e.validate()
                .unwrap_or_else(|_| panic!("{E_BAD_ENTRY} entry fields are empty or too long"));

            stamp_windows(&mut plan, at);
            plan.inspect(&program, PROGRAM_LOADER_ACCOUNT_ID, &Effect::HeaderIs(header));
            plan.effect(&global, &Effect::NextSlot { expect: seq_index });
            plan.effect(
                &directory,
                &Effect::WriteSlot(DirectorySlot {
                    program_id: key.program,
                    entry_seed: key.seed(),
                }),
            );
            plan.effect(&entry, &Effect::CreateEntry(Box::new(e)));
        }
        Instruction::Update {
            key,
            version,
            description,
            idl,
        } => {
            let [entry, author] = the::<2>(input);
            expect_at(&entry, pda(me, key.seed()), "entry");
            expect_signed(&author, "author");
            plan.effect(
                &entry,
                &Effect::UpdateMeta {
                    author: *author.account_id.value(),
                    version,
                    description,
                    idl: decode_idl(&idl),
                },
            );
        }
        Instruction::ClaimSource { key, claim } => {
            let [entry, author] = the::<2>(input);
            expect_at(&entry, pda(me, key.seed()), "entry");
            expect_signed(&author, "author");
            plan.effect(
                &entry,
                &Effect::ClaimSource {
                    author: *author.account_id.value(),
                    claim,
                },
            );
        }
        Instruction::RecordRebuild {
            key,
            header,
            rebuilt,
            at,
        } => {
            let [entry, program, attester] = the::<3>(input);
            expect_at(&entry, pda(me, key.seed()), "entry");
            expect_header(&program, &key);
            // Signed, or anyone could put words in someone else's mouth: the
            // attester is what a reader weighs a verdict by.
            expect_signed(&attester, "attester");
            stamp_windows(&mut plan, at);
            plan.inspect(&program, PROGRAM_LOADER_ACCOUNT_ID, &Effect::HeaderIs(header));
            plan.effect(
                &entry,
                &Effect::RecordRebuild {
                    attester: *attester.account_id.value(),
                    header,
                    rebuilt,
                    at,
                },
            );
        }
        Instruction::SyncHeader { key, header } => {
            let [entry, program] = the::<2>(input);
            expect_at(&entry, pda(me, key.seed()), "entry");
            expect_header(&program, &key);
            plan.inspect(&program, PROGRAM_LOADER_ACCOUNT_ID, &Effect::HeaderIs(header));
            plan.effect(&entry, &Effect::SyncHeader(header));
        }
        Instruction::Freeze { key } => {
            let [entry, author] = the::<2>(input);
            expect_at(&entry, pda(me, key.seed()), "entry");
            expect_signed(&author, "author");
            plan.effect(
                &entry,
                &Effect::Freeze {
                    author: *author.account_id.value(),
                },
            );
        }
    }
    plan
}

fn encode<T: BorshSerialize>(value: &T) -> Vec<u8> {
    borsh::to_vec(value).expect("borsh serialization is infallible")
}

fn load_entry(pre: &[u8]) -> Entry {
    assert!(!pre.is_empty(), "{E_BAD_ENTRY} no entry at this address");
    borsh::from_slice(pre).unwrap_or_else(|_| panic!("{E_BAD_ENTRY} entry failed to decode"))
}

fn writable(e: &Entry, who: &[u8; 32]) {
    match e.require_writable_by(who) {
        Ok(()) => {}
        Err(RegistryError::Immutable) => panic!("{E_IMMUTABLE} entry is frozen"),
        Err(_) => panic!("{E_NOT_AUTHOR} signer is not this entry's author"),
    }
}

fn image_bytes(h: &ProgramHeader) -> ImageId {
    let mut out = [0u8; 32];
    for (chunk, word) in out.chunks_exact_mut(4).zip(h.image_id.iter()) {
        chunk.copy_from_slice(&word.to_le_bytes());
    }
    out
}

fn apply(effect: Effect, pre: &[u8]) -> Option<Vec<u8>> {
    match effect {
        Effect::InitGlobal => {
            assert!(pre.is_empty(), "{E_ALREADY_REGISTERED} already initialized");
            Some(encode(&GlobalState::default()))
        }
        Effect::NextSlot { expect } => {
            assert!(!pre.is_empty(), "{E_NOT_INITIALIZED} registry is not initialized");
            let mut g: GlobalState = borsh::from_slice(pre)
                .unwrap_or_else(|_| panic!("{E_BAD_ENTRY} global failed to decode"));
            // The directory slot is not the caller's to choose: it is the
            // counter's current value, or an earlier slot could be overwritten
            // and an entry hidden from every indexer.
            assert!(
                g.total_entries == expect,
                "{E_SLOT_MISMATCH} seq_index is not the next directory slot"
            );
            g.total_entries = g
                .total_entries
                .checked_add(1)
                .unwrap_or_else(|| panic!("{E_BAD_ENTRY} entry counter overflow"));
            Some(encode(&g))
        }
        Effect::WriteSlot(slot) => {
            assert!(pre.is_empty(), "{E_SLOT_MISMATCH} directory slot already written");
            Some(encode(&slot))
        }
        Effect::CreateEntry(entry) => {
            assert!(
                pre.is_empty(),
                "{E_ALREADY_REGISTERED} an entry already exists at this address"
            );
            Some(encode(entry.as_ref()))
        }
        Effect::HeaderIs(view) => {
            let h = ProgramHeader::from_bytes(pre).unwrap_or_else(|| {
                panic!("{E_HEADER_MISMATCH} this account holds no program header")
            });
            assert!(
                image_bytes(&h) == view.image_id && h.immutable == view.immutable,
                "{E_HEADER_MISMATCH} the program header on chain is not the one supplied"
            );
            // Read-only: a foreign shard can never be written by this program.
            None
        }
        Effect::UpdateMeta {
            author,
            version,
            description,
            idl,
        } => {
            let mut e = load_entry(pre);
            writable(&e, &author);
            e.version = version;
            e.description = description;
            e.idl = idl;
            e.validate()
                .unwrap_or_else(|_| panic!("{E_BAD_ENTRY} entry fields are empty or too long"));
            Some(encode(&e))
        }
        Effect::ClaimSource { author, claim } => {
            let mut e = load_entry(pre);
            writable(&e, &author);
            // Replacing the claim resets the verdict: a verification is about
            // one claim, and a new claim has not been rebuilt by anyone.
            e.verification = Verification::claim(claim)
                .unwrap_or_else(|_| panic!("{E_BAD_CLAIM} claim fields are empty or too long"));
            Some(encode(&e))
        }
        Effect::RecordRebuild {
            attester,
            header,
            rebuilt,
            at,
        } => {
            let mut e = load_entry(pre);
            // The header was just checked on chain by the HeaderIs effect, so
            // the subject of the verdict is a chain fact. If it moved since the
            // entry last looked, the old verdict goes stale first.
            e.sync_header(header.image_id, header.immutable);
            let subject = e.image_id;
            match e.verification.record_rebuild(rebuilt, subject, attester, at) {
                // A mismatch and a networked build are outcomes, not failures:
                // an erroring apply would abort the transaction and the verdict
                // would never land.
                Ok(())
                | Err(RegistryError::ArtifactMismatch)
                | Err(RegistryError::NetworkedBuild) => Some(encode(&e)),
                Err(RegistryError::AlreadyVerified) => panic!(
                    "{E_ALREADY_VERIFIED} this entry is verified; a mismatch assertion cannot erase it"
                ),
                Err(_) => panic!("{E_BAD_CLAIM} no source claim to rebuild"),
            }
        }
        Effect::SyncHeader(view) => {
            let mut e = load_entry(pre);
            e.sync_header(view.image_id, view.immutable);
            Some(encode(&e))
        }
        Effect::Freeze { author } => {
            let mut e = load_entry(pre);
            match e.freeze(&author) {
                Ok(()) => Some(encode(&e)),
                Err(RegistryError::Immutable) => panic!("{E_IMMUTABLE} entry is already frozen"),
                Err(_) => panic!("{E_NOT_AUTHOR} signer is not this entry's author"),
            }
        }
    }
}
