// SPDX-License-Identifier: MIT OR Apache-2.0
//! Build registry transactions for LEZ v0.3.
//!
//! A v0.3 public transaction names the program it calls, a list of **shard
//! selectors** `(account, program)` (which program's shard of which account the
//! call may see), the signers' nonces, the borsh instruction, and an optional
//! fee declaration. This module produces the program-defined parts: the
//! selectors in the order the registry reads them, each address re-derived
//! rather than chosen, and the borsh of [`Instruction`], the same enum the guest
//! decodes. [`crate::sign`] turns a [`BuiltTransaction`] into bytes to submit.
//!
//! Field limits are the program's own, so a value refused here is a value the
//! chain would refuse, caught without paying a fee for the lesson.

use crate::{directory_address, global_address, key_address, PROGRAM_LOADER};
use lez_registry_core as core_;
use lez_registry_core::instruction::{EntryKey, HeaderView, Instruction};
pub use lez_registry_core::Publisher;
use lez_registry_core::{AccountId, ImageId, ProgramId, SourceClaim, Stamp};

/// Which program's shard of which account a call may see:
/// `ProgramShardSelector { account_id, program_account_id }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selector {
    pub account: AccountId,
    pub program: AccountId,
}

/// Which declared account a slot is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The registry entry PDA.
    Entry,
    /// The singleton global counter PDA.
    Global,
    /// The sequence-directory slot PDA for this registration.
    Directory,
    /// The registered program's header, in the loader's shard (read-only).
    Program,
    /// The signing author (the program itself for a deployer entry).
    Author,
    /// The signing party attesting to a rebuild.
    Attester,
}

impl Role {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Entry => "entry",
            Self::Global => "global",
            Self::Directory => "directory",
            Self::Program => "program",
            Self::Author => "author",
            Self::Attester => "attester",
        }
    }
}

/// One input of a built transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountMeta {
    pub role: Role,
    pub selector: Selector,
    /// Whether this account must sign. The runtime marks an input authorized
    /// exactly when its account id signed the transaction.
    pub signer: bool,
    /// Whether the registry writes its shard here (informational, mirrors the IDL).
    pub writable: bool,
}

/// A registry transaction, resolved but unsigned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltTransaction {
    /// The registry program account this transaction calls.
    pub registry: ProgramId,
    /// Instruction name, as the IDL spells it.
    pub instruction: &'static str,
    /// Borsh variant index of the instruction.
    pub instruction_index: u8,
    /// Inputs in the order the program reads them.
    pub accounts: Vec<AccountMeta>,
    /// `borsh(Instruction)`.
    pub instruction_data: Vec<u8>,
}

impl BuiltTransaction {
    fn new(registry: &ProgramId, ix: &Instruction, accounts: Vec<AccountMeta>) -> Self {
        Self {
            registry: *registry,
            instruction: Instruction::NAMES[ix.index() as usize],
            instruction_index: ix.index(),
            accounts,
            instruction_data: borsh::to_vec(ix).expect("borsh serialization is infallible"),
        }
    }

    /// The shard selectors, in declared order.
    #[must_use]
    pub fn selectors(&self) -> Vec<Selector> {
        self.accounts.iter().map(|a| a.selector).collect()
    }

    /// The accounts that must sign, in declared order, each once.
    #[must_use]
    pub fn signer_ids(&self) -> Vec<AccountId> {
        let mut out: Vec<AccountId> = Vec::new();
        for a in self.accounts.iter().filter(|a| a.signer) {
            if !out.contains(&a.selector.account) {
                out.push(a.selector.account);
            }
        }
        out
    }

    /// The entry account this transaction touches, if it touches one.
    #[must_use]
    pub fn entry_address(&self) -> Option<AccountId> {
        self.accounts
            .iter()
            .find(|a| a.role == Role::Entry)
            .map(|a| a.selector.account)
    }
}

/// Why a transaction could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildError {
    /// A metadata field was empty or over the length the account layout reserves.
    Field(core_::RegistryError),
    /// The IDL pointer was neither empty nor a valid multibase content id.
    BadIdl,
    /// A third-party key must name the author who signs it.
    PublisherIsNotAuthor,
}

impl core::fmt::Display for BuildError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Field(e) => write!(f, "field is empty or too long: {e:?}"),
            Self::BadIdl => write!(
                f,
                "idl is not a multibase base58btc content identifier (or empty to clear)"
            ),
            Self::PublisherIsNotAuthor => {
                write!(f, "a third-party entry is keyed by its signing author")
            }
        }
    }
}

impl std::error::Error for BuildError {}

fn own(registry: &ProgramId, account: AccountId, role: Role, writable: bool) -> AccountMeta {
    AccountMeta {
        role,
        selector: Selector {
            account,
            program: *registry,
        },
        signer: false,
        writable,
    }
}

fn signer(registry: &ProgramId, account: AccountId, role: Role) -> AccountMeta {
    AccountMeta {
        role,
        selector: Selector {
            account,
            program: *registry,
        },
        signer: true,
        writable: false,
    }
}

fn header_input(program: &ProgramId, signs: bool) -> AccountMeta {
    AccountMeta {
        role: Role::Program,
        selector: Selector {
            account: *program,
            program: PROGRAM_LOADER,
        },
        signer: signs,
        writable: false,
    }
}

fn check_idl(idl: &str) -> Result<(), BuildError> {
    if !idl.is_empty() && core_::decode_content_id(idl).is_none() {
        return Err(BuildError::BadIdl);
    }
    Ok(())
}

/// `initialize`: create the global counter.
#[must_use]
pub fn build_initialize(registry: &ProgramId) -> BuiltTransaction {
    BuiltTransaction::new(
        registry,
        &Instruction::Initialize,
        vec![own(registry, global_address(registry), Role::Global, true)],
    )
}

/// A request to register a program.
///
/// `deployer: true` builds the deployer's entry, which the program accepts only
/// when `author` is the program's own header account, signing. Otherwise the
/// entry is a third-party entry keyed by `author`.
///
/// `header` is the program's header as read from chain
/// ([`crate::Client::program_header`]); the guest refuses unless it is exactly
/// what the loader holds. `seq_index` is the registry's current
/// `total_entries`. `at` is a recent [`crate::Client::clock`] reading.
#[derive(Debug, Clone)]
pub struct RegisterRequest<'a> {
    pub program: &'a ProgramId,
    pub author: &'a AccountId,
    pub deployer: bool,
    pub header: HeaderView,
    pub seq_index: u64,
    pub at: Stamp,
    pub name: &'a str,
    pub version: &'a str,
    pub description: &'a str,
    pub tags: &'a [String],
    pub idl: &'a str,
    pub source: Option<SourceClaim>,
}

impl RegisterRequest<'_> {
    /// The key of the entry this request creates.
    #[must_use]
    pub fn key(&self) -> EntryKey {
        EntryKey {
            program: *self.program,
            publisher: if self.deployer {
                None
            } else {
                Some(*self.author)
            },
        }
    }
}

/// `register`.
pub fn build_register(
    registry: &ProgramId,
    req: &RegisterRequest<'_>,
) -> Result<BuiltTransaction, BuildError> {
    let publisher = if req.deployer {
        Publisher::Canonical
    } else {
        Publisher::ThirdParty
    };
    // Validate with the program's own type, so the CLI cannot disagree with the
    // chain about what fits.
    let mut e = core_::Entry::new(
        *req.program,
        req.at,
        req.name.to_string(),
        req.version.to_string(),
        *req.author,
        publisher,
    )
    .map_err(BuildError::Field)?;
    e.description = req.description.to_string();
    e.tags = req.tags.to_vec();
    e.validate().map_err(BuildError::Field)?;
    check_idl(req.idl)?;
    if let Some(c) = &req.source {
        c.validate().map_err(BuildError::Field)?;
    }

    let key = req.key();
    let ix = Instruction::Register {
        key,
        header: req.header,
        seq_index: req.seq_index,
        at: req.at,
        name: req.name.to_string(),
        version: req.version.to_string(),
        description: req.description.to_string(),
        tags: req.tags.to_vec(),
        idl: req.idl.to_string(),
        source: req.source.clone(),
    };
    let accounts = vec![
        own(registry, key_address(registry, &key), Role::Entry, true),
        own(registry, global_address(registry), Role::Global, true),
        own(
            registry,
            directory_address(registry, req.seq_index),
            Role::Directory,
            true,
        ),
        // For a deployer entry the program's account is also the author, so
        // it signs; the runtime then marks both of its selectors authorized.
        header_input(req.program, req.deployer),
        signer(registry, *req.author, Role::Author),
    ];
    Ok(BuiltTransaction::new(registry, &ix, accounts))
}

/// `update`: version, description and IDL pointer. Author only.
pub fn build_update(
    registry: &ProgramId,
    key: &EntryKey,
    author: &AccountId,
    version: &str,
    description: &str,
    idl: &str,
) -> Result<BuiltTransaction, BuildError> {
    if version.is_empty() {
        return Err(BuildError::Field(core_::RegistryError::Empty));
    }
    if version.len() > core_::MAX_VERSION || description.len() > core_::MAX_DESCRIPTION {
        return Err(BuildError::Field(core_::RegistryError::TooLong));
    }
    check_idl(idl)?;
    let ix = Instruction::Update {
        key: *key,
        version: version.to_string(),
        description: description.to_string(),
        idl: idl.to_string(),
    };
    Ok(BuiltTransaction::new(
        registry,
        &ix,
        vec![
            own(registry, key_address(registry, key), Role::Entry, true),
            signer(registry, *author, Role::Author),
        ],
    ))
}

/// `claim_source`: replace the source claim. Author only.
pub fn build_claim_source(
    registry: &ProgramId,
    key: &EntryKey,
    author: &AccountId,
    claim: SourceClaim,
) -> Result<BuiltTransaction, BuildError> {
    claim.validate().map_err(BuildError::Field)?;
    let ix = Instruction::ClaimSource { key: *key, claim };
    Ok(BuiltTransaction::new(
        registry,
        &ix,
        vec![
            own(registry, key_address(registry, key), Role::Entry, true),
            signer(registry, *author, Role::Author),
        ],
    ))
}

/// `record_rebuild`: attest that a rebuild produced `rebuilt`.
#[must_use]
pub fn build_record_rebuild(
    registry: &ProgramId,
    key: &EntryKey,
    header: HeaderView,
    rebuilt: ImageId,
    at: Stamp,
    attester: &AccountId,
) -> BuiltTransaction {
    let ix = Instruction::RecordRebuild {
        key: *key,
        header,
        rebuilt,
        at,
    };
    BuiltTransaction::new(
        registry,
        &ix,
        vec![
            own(registry, key_address(registry, key), Role::Entry, true),
            header_input(&key.program, false),
            signer(registry, *attester, Role::Attester),
        ],
    )
}

/// `sync_header`: re-read the header. Permissionless.
#[must_use]
pub fn build_sync_header(registry: &ProgramId, key: &EntryKey, header: HeaderView) -> BuiltTransaction {
    let ix = Instruction::SyncHeader { key: *key, header };
    BuiltTransaction::new(
        registry,
        &ix,
        vec![
            own(registry, key_address(registry, key), Role::Entry, true),
            header_input(&key.program, false),
        ],
    )
}

/// `freeze`: make the entry's metadata permanent. Author only.
#[must_use]
pub fn build_freeze(registry: &ProgramId, key: &EntryKey, author: &AccountId) -> BuiltTransaction {
    let ix = Instruction::Freeze { key: *key };
    BuiltTransaction::new(
        registry,
        &ix,
        vec![
            own(registry, key_address(registry, key), Role::Entry, true),
            signer(registry, *author, Role::Author),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{entry_address, pda, third_party_entry_address};

    const REGISTRY: ProgramId = [0x42; 32];
    const PROGRAM: ProgramId = [0x11; 32];
    const AUTHOR: AccountId = [0x22; 32];

    fn header() -> HeaderView {
        HeaderView {
            image_id: [0x33; 32],
            immutable: false,
        }
    }

    fn req(deployer: bool) -> RegisterRequest<'static> {
        RegisterRequest {
            program: &PROGRAM,
            author: if deployer { &PROGRAM } else { &AUTHOR },
            deployer,
            header: header(),
            seq_index: 3,
            at: Stamp {
                block: 10,
                millis: 1_000,
            },
            name: "prog",
            version: "1.0.0",
            description: "a program",
            tags: &[],
            idl: "",
            source: None,
        }
    }

    #[test]
    fn a_deployer_entry_is_signed_by_the_program_account_itself() {
        let tx = build_register(&REGISTRY, &req(true)).unwrap();
        assert_eq!(tx.instruction, "register");
        assert_eq!(tx.instruction_index, 1);
        assert_eq!(tx.instruction_data[0], 1);
        assert_eq!(tx.entry_address(), Some(entry_address(&REGISTRY, &PROGRAM)));
        assert_eq!(tx.signer_ids(), vec![PROGRAM], "one signer, once");
        let roles: Vec<_> = tx.accounts.iter().map(|a| a.role).collect();
        assert_eq!(
            roles,
            vec![Role::Entry, Role::Global, Role::Directory, Role::Program, Role::Author]
        );
        assert_eq!(tx.accounts[3].selector.program, PROGRAM_LOADER);
        assert_eq!(tx.accounts[2].selector.account, directory_address(&REGISTRY, 3));
    }

    #[test]
    fn a_third_party_entry_lands_at_the_publisher_keyed_address() {
        let tx = build_register(&REGISTRY, &req(false)).unwrap();
        assert_eq!(
            tx.entry_address(),
            Some(third_party_entry_address(&REGISTRY, &PROGRAM, &AUTHOR))
        );
        assert_eq!(tx.signer_ids(), vec![AUTHOR]);
        assert!(!tx.accounts[3].signer, "the header is only read");
    }

    #[test]
    fn the_instruction_bytes_decode_as_the_guest_decodes_them() {
        let tx = build_register(&REGISTRY, &req(true)).unwrap();
        let ix: Instruction = borsh::from_slice(&tx.instruction_data).unwrap();
        match ix {
            Instruction::Register { key, seq_index, .. } => {
                assert_eq!(key.program, PROGRAM);
                assert_eq!(key.publisher, None);
                assert_eq!(seq_index, 3);
            }
            other => panic!("decoded {other:?}"),
        }
    }

    #[test]
    fn over_long_fields_and_bad_idls_are_refused_before_submission() {
        let long = "x".repeat(core_::MAX_NAME + 1);
        let mut r = req(true);
        r.name = &long;
        assert!(matches!(build_register(&REGISTRY, &r), Err(BuildError::Field(_))));
        let mut r = req(true);
        r.idl = "not-a-cid";
        assert_eq!(build_register(&REGISTRY, &r), Err(BuildError::BadIdl));
        let key = req(true).key();
        assert_eq!(
            build_update(&REGISTRY, &key, &PROGRAM, "", "", ""),
            Err(BuildError::Field(core_::RegistryError::Empty))
        );
        assert!(build_update(
            &REGISTRY,
            &key,
            &PROGRAM,
            "2",
            "",
            "zDvZRwzm9Ni32iyvoy868YVrBHFz8vo3oZirrCKhFMMn9aXuinfQ"
        )
        .is_ok());
    }

    #[test]
    fn every_instruction_addresses_the_entry_it_names() {
        let key = EntryKey {
            program: PROGRAM,
            publisher: Some(AUTHOR),
        };
        let at = Stamp { block: 1, millis: 1 };
        let txs = [
            build_update(&REGISTRY, &key, &AUTHOR, "2", "", "").unwrap(),
            build_record_rebuild(&REGISTRY, &key, header(), [0; 32], at, &[9; 32]),
            build_sync_header(&REGISTRY, &key, header()),
            build_freeze(&REGISTRY, &key, &AUTHOR),
        ];
        for tx in &txs {
            assert_eq!(tx.entry_address(), Some(pda(&REGISTRY, &key.seed())));
        }
        assert!(txs[2].signer_ids().is_empty(), "sync_header is permissionless");
        assert_eq!(txs[1].signer_ids(), vec![[9; 32]]);
    }

    #[test]
    fn initialize_is_the_first_variant_and_takes_only_the_global_account() {
        let tx = build_initialize(&REGISTRY);
        assert_eq!(tx.instruction_data, vec![0]);
        assert_eq!(tx.accounts.len(), 1);
        assert_eq!(tx.accounts[0].selector.account, global_address(&REGISTRY));
    }
}
