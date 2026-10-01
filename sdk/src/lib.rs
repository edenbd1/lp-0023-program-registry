// SPDX-License-Identifier: MIT OR Apache-2.0
//! Resolve, register and verify LEZ programs in the Provenance registry, on LEZ
//! v0.3.
//!
//! The one thing worth knowing about this SDK is what it does **not** contain: a
//! search index, a cache, or a server to ask. An entry's address is a pure
//! function of the registry's program account and the program you are asking
//! about, so resolution is a hash and a single account read. There is nothing in
//! the path that can be stale, censor a program, or go down separately from the
//! chain.
//!
//! On v0.3 an account holds one **shard** per program, and the registry's data
//! is the registry program's shard of each account it uses. A program itself is
//! an account too: its header lives in the `program_loader` shard of the
//! program's account, and [`Client::program_header`] reads it.
//!
//! A search index exists (see the `indexer`), but it is an accelerator for
//! *browsing*, never a dependency for *resolving*.

use lez_registry_core as core_;
pub use lez_registry_core::{
    instruction::{EntryKey, HeaderView, Instruction},
    AccountId, Entry, GlobalState, ImageId, ProgramId, Publisher, SourceClaim, Stamp,
    VerificationState,
};
use sha2::{Digest, Sha256};

pub mod sign;
pub mod tx;
pub mod v03;
pub use sign::{FeeDeclaration, Keypair};
pub use tx::{
    build_claim_source, build_freeze, build_initialize, build_record_rebuild, build_register,
    build_sync_header, build_update, AccountMeta, BuildError, BuiltTransaction, RegisterRequest,
    Role, Selector,
};
pub use v03::{ProgramHeader, PROGRAM_LOADER};

/// Domain separator LEZ hashes into every program-derived address.
///
/// Copied from the runtime: `lee/state_machine/core`, `AccountId::for_public_pda`
/// at v0.3.0. The prefix still says v0.2, and that is the runtime's string, not
/// a leftover here. The end-to-end run writes to exactly these addresses, so a
/// change in the runtime breaks it rather than silently returning addresses
/// nothing lives at.
const PDA_PREFIX: &[u8; 32] = b"/LEE/v0.2/AccountId/PDA/\x00\x00\x00\x00\x00\x00\x00\x00";

/// The native token program's account id, whose shard of an account is its
/// balance (`u128`, little-endian).
pub const NATIVE_TOKEN: AccountId = [0u8; 32];

/// The every-block clock account, `/LEZ/ClockProgramAccount/0000001`.
pub const CLOCK_01: AccountId = *b"/LEZ/ClockProgramAccount/0000001";

/// The clock program's account id: `sha256("/LEE-BuiltinProgram/v1/AccountId" ‖ "clock")`.
#[must_use]
pub fn clock_program() -> AccountId {
    let mut h = Sha256::new();
    h.update(b"/LEE-BuiltinProgram/v1/AccountId");
    h.update(b"clock");
    h.finalize().into()
}

/// Derive a public program-derived address.
#[must_use]
pub fn pda(program_id: &ProgramId, seed: &[u8; 32]) -> AccountId {
    let mut buf = [0u8; 96];
    buf[..32].copy_from_slice(PDA_PREFIX);
    buf[32..64].copy_from_slice(program_id);
    buf[64..].copy_from_slice(seed);
    Sha256::digest(buf).into()
}

/// Where a program's deployer entry lives in a given registry.
#[must_use]
pub fn entry_address(registry: &ProgramId, program: &ProgramId) -> AccountId {
    pda(registry, &core_::entry_seed(program))
}

/// Where a particular publisher's entry about a program lives.
#[must_use]
pub fn third_party_entry_address(
    registry: &ProgramId,
    program: &ProgramId,
    publisher: &AccountId,
) -> AccountId {
    pda(registry, &core_::third_party_entry_seed(program, publisher))
}

/// Where the entry an [`EntryKey`] names lives.
#[must_use]
pub fn key_address(registry: &ProgramId, key: &EntryKey) -> AccountId {
    pda(registry, &key.seed())
}

/// Where the `n`th registration recorded which program it was.
#[must_use]
pub fn directory_address(registry: &ProgramId, n: u64) -> AccountId {
    pda(registry, &core_::seq_seed(n))
}

/// The registry's singleton counter account.
#[must_use]
pub fn global_address(registry: &ProgramId) -> AccountId {
    pda(registry, &core_::global_seed())
}

/// Base58, as the wallet and the RPC print account ids.
#[must_use]
pub fn base58(bytes: &[u8; 32]) -> String {
    const ALPHABET: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    let mut digits: Vec<u8> = Vec::with_capacity(45);
    for &byte in bytes {
        let mut carry = u32::from(byte);
        for digit in &mut digits {
            carry += u32::from(*digit) << 8;
            *digit = (carry % 58) as u8;
            carry /= 58;
        }
        while carry > 0 {
            digits.push((carry % 58) as u8);
            carry /= 58;
        }
    }
    // Leading zero bytes are significant in base58 and must survive as '1's.
    let leading = bytes.iter().take_while(|b| **b == 0).count();
    let mut out = String::with_capacity(leading + digits.len());
    out.extend(std::iter::repeat_n('1', leading));
    out.extend(digits.iter().rev().map(|d| ALPHABET[*d as usize] as char));
    out
}

/// Decode a base58 account id back to 32 bytes. `None` on a bad character or a
/// length that is not 32 bytes.
#[must_use]
pub fn from_base58(s: &str) -> Option<[u8; 32]> {
    const ALPHABET: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    let s = s
        .strip_prefix("Public/")
        .unwrap_or(s);
    let mut bytes: Vec<u8> = Vec::with_capacity(32);
    for c in s.bytes() {
        let mut carry = ALPHABET.iter().position(|&a| a == c)? as u32;
        for byte in &mut bytes {
            carry += u32::from(*byte) * 58;
            *byte = (carry & 0xff) as u8;
            carry >>= 8;
        }
        while carry > 0 {
            bytes.push((carry & 0xff) as u8);
            carry >>= 8;
        }
    }
    for c in s.bytes() {
        if c != b'1' {
            break;
        }
        bytes.push(0);
    }
    if bytes.len() != 32 {
        return None;
    }
    bytes.reverse();
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Some(out)
}

/// Lower-case hex of any byte string.
#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Parse 64 hex characters (an optional `0x` is accepted) into 32 bytes.
#[must_use]
pub fn from_hex32(s: &str) -> Option<[u8; 32]> {
    let s = s.trim().strip_prefix("0x").unwrap_or(s.trim());
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(s.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

/// Parse an account id given as base58 (optionally `Public/`-prefixed) or hex.
#[must_use]
pub fn parse_account(s: &str) -> Option<[u8; 32]> {
    from_base58(s).or_else(|| from_hex32(s))
}

#[derive(Debug)]
pub enum Error {
    Transport(String),
    Malformed(String),
    /// The sequencer answered with a JSON-RPC error.
    Rpc(String),
    /// A transaction was included but its effect is not on chain: LEZ v0.3
    /// charges and reverts a failed program call rather than rejecting it.
    Reverted(String),
    Timeout(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(m) => write!(f, "transport: {m}"),
            Self::Malformed(m) => write!(f, "malformed response: {m}"),
            Self::Rpc(m) => write!(f, "sequencer refused: {m}"),
            Self::Reverted(m) => write!(f, "reverted: {m}"),
            Self::Timeout(m) => write!(f, "timed out: {m}"),
        }
    }
}
impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// A transaction the sequencer included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Submitted {
    pub hash: String,
    pub block: u64,
}

/// A client for one registry deployment on one sequencer.
pub struct Client {
    endpoint: String,
    registry: ProgramId,
}

impl Client {
    #[must_use]
    pub fn new(endpoint: impl Into<String>, registry: ProgramId) -> Self {
        Self {
            endpoint: endpoint.into(),
            registry,
        }
    }

    #[must_use]
    pub const fn registry(&self) -> &ProgramId {
        &self.registry
    }

    #[must_use]
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// One JSON-RPC call; the `result`, or [`Error::Rpc`] with the sequencer's
    /// own message.
    pub fn call(&self, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        let body = serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": method, "params": params,
        });
        let text = ureq::post(&self.endpoint)
            .set("content-type", "application/json")
            .send_string(&body.to_string())
            .map_err(|e| Error::Transport(e.to_string()))?
            .into_string()
            .map_err(|e| Error::Transport(e.to_string()))?;
        let v: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| Error::Malformed(e.to_string()))?;
        if let Some(err) = v.get("error") {
            return Err(Error::Rpc(err.to_string()));
        }
        Ok(v.get("result").cloned().unwrap_or(serde_json::Value::Null))
    }

    /// The raw `getAccount` JSON-RPC response for `id`.
    pub(crate) fn get_account_json(&self, id: &AccountId) -> Result<serde_json::Value> {
        let result = self.call("getAccount", serde_json::json!([base58(id)]))?;
        Ok(serde_json::json!({ "result": result }))
    }

    /// One program's shard of an account, or `None` when that program has
    /// written nothing there.
    pub fn shard(&self, account: &AccountId, program: &AccountId) -> Result<Option<Vec<u8>>> {
        let resp = self.get_account_json(account)?;
        let Some(result) = resp.get("result") else {
            return Ok(None);
        };
        Ok(v03::shard(result, program).filter(|b| !b.is_empty()))
    }

    /// The registry's shard of an account: what the registry wrote there.
    ///
    /// A derived address always *exists* as an address; what distinguishes a
    /// registered program from an unregistered one is whether anything was
    /// written there. Empty and absent are the same answer.
    pub fn account_data(&self, id: &AccountId) -> Result<Option<Vec<u8>>> {
        self.shard(id, &self.registry)
    }

    /// Resolve a program's deployer entry. One hash, one read, no index.
    pub fn entry(&self, program: &ProgramId) -> Result<Option<Entry>> {
        self.entry_at(&entry_address(&self.registry, program))
    }

    /// Resolve one publisher's entry about a program.
    pub fn third_party_entry(
        &self,
        program: &ProgramId,
        publisher: &AccountId,
    ) -> Result<Option<Entry>> {
        self.entry_at(&third_party_entry_address(&self.registry, program, publisher))
    }

    /// Resolve whichever entry a key names.
    pub fn entry_by_key(&self, key: &EntryKey) -> Result<Option<Entry>> {
        self.entry_at(&key_address(&self.registry, key))
    }

    /// Decode the entry at an entry account.
    pub fn entry_at(&self, addr: &AccountId) -> Result<Option<Entry>> {
        let Some(raw) = self.account_data(addr)? else {
            return Ok(None);
        };
        borsh::from_slice(&raw)
            .map(Some)
            .map_err(|e| Error::Malformed(e.to_string()))
    }

    /// What registered `n`th, if anything did.
    pub fn slot_at(&self, n: u64) -> Result<Option<core_::DirectorySlot>> {
        let Some(raw) = self.account_data(&directory_address(&self.registry, n))? else {
            return Ok(None);
        };
        borsh::from_slice::<core_::DirectorySlot>(&raw)
            .map(Some)
            .map_err(|e| Error::Malformed(e.to_string()))
    }

    /// Every entry, in registration order.
    ///
    /// Two reads per entry and no query: slot `n` and the entry it names are both
    /// derived before the chain is asked anything. A slot that is empty or an
    /// entry that fails to decode is skipped rather than aborting the walk.
    pub fn walk(&self) -> Result<Vec<Entry>> {
        let total = self.global()?.map_or(0, |g| g.total_entries);
        let mut out = Vec::new();
        for n in 0..total {
            let Some(slot) = self.slot_at(n)? else {
                continue;
            };
            // The slot's seed, not the program id: deriving from the program id
            // would resolve every third-party entry to the deployer's.
            let addr = pda(&self.registry, &slot.entry_seed);
            if let Ok(Some(entry)) = self.entry_at(&addr) {
                out.push(entry);
            }
        }
        Ok(out)
    }

    /// The registry's counter.
    pub fn global(&self) -> Result<Option<GlobalState>> {
        let Some(raw) = self.account_data(&global_address(&self.registry))? else {
            return Ok(None);
        };
        borsh::from_slice(&raw)
            .map(Some)
            .map_err(|e| Error::Malformed(e.to_string()))
    }

    /// Chain time as the sequencer's every-block clock account holds it.
    pub fn clock(&self) -> Result<Stamp> {
        let raw = self
            .shard(&CLOCK_01, &clock_program())?
            .ok_or_else(|| Error::Malformed("the clock account holds no clock".into()))?;
        if raw.len() != 16 {
            return Err(Error::Malformed("clock shard is not 16 bytes".into()));
        }
        let mut b = [0u8; 8];
        b.copy_from_slice(&raw[..8]);
        let block = u64::from_le_bytes(b);
        b.copy_from_slice(&raw[8..]);
        Ok(Stamp {
            block,
            millis: u64::from_le_bytes(b),
        })
    }

    /// The native LGO balance of an account.
    pub fn balance(&self, id: &AccountId) -> Result<u128> {
        let Some(raw) = self.shard(id, &NATIVE_TOKEN)? else {
            return Ok(0);
        };
        let arr: [u8; 16] = raw
            .as_slice()
            .try_into()
            .map_err(|_| Error::Malformed("balance shard is not 16 bytes".into()))?;
        Ok(u128::from_le_bytes(arr))
    }

    /// Current nonces of `ids`, in order.
    pub fn nonces(&self, ids: &[AccountId]) -> Result<Vec<u128>> {
        let ids: Vec<String> = ids.iter().map(base58).collect();
        let v = self.call("getAccountsNonces", serde_json::json!([ids]))?;
        v.as_array()
            .ok_or_else(|| Error::Malformed("nonces are not an array".into()))?
            .iter()
            .map(|n| {
                n.as_u64()
                    .map(u128::from)
                    .or_else(|| n.as_str().and_then(|s| s.parse().ok()))
                    .ok_or_else(|| Error::Malformed(format!("nonce {n} is not a number")))
            })
            .collect()
    }

    /// The last block the sequencer produced.
    pub fn last_block(&self) -> Result<u64> {
        self.call("getLastBlockId", serde_json::json!([]))?
            .as_u64()
            .ok_or_else(|| Error::Malformed("block id is not a number".into()))
    }

    /// Submit a signed transaction (the bytes [`sign::sign_transaction`]
    /// produces). Returns the transaction hash as the sequencer prints it.
    pub fn send(&self, tx: &[u8]) -> Result<String> {
        let v = self.call("sendTransaction", serde_json::json!([sign::base64(tx)]))?;
        Ok(match v {
            serde_json::Value::String(s) => s,
            other => other.to_string(),
        })
    }

    /// The block a transaction was included in, if it was.
    pub fn transaction_block(&self, hash: &str) -> Result<Option<u64>> {
        let v = self.call("getTransaction", serde_json::json!([hash]))?;
        Ok(v.as_array()
            .and_then(|a| a.get(1))
            .and_then(serde_json::Value::as_u64))
    }

    /// Sign and submit `tx`, then wait for it to be included.
    ///
    /// `keys` must cover every account in `tx.signer_ids()`; `payer`, if given,
    /// co-signs as the fee payer (the wallet's default gas limit and fee cap).
    /// Nonces are read from the sequencer immediately before signing.
    ///
    /// Inclusion is not success on v0.3: a program call that fails is charged
    /// and reverted, and still lands in a block. Read the state you expected to
    /// change afterwards; the CLI does exactly that for every command.
    pub fn submit(
        &self,
        tx: &BuiltTransaction,
        keys: &[&Keypair],
        payer: Option<&Keypair>,
    ) -> Result<Submitted> {
        self.submit_with_gas(tx, keys, payer, sign::DEFAULT_GAS_LIMIT)
    }

    /// [`Client::submit`] with an explicit gas limit (executed cycles).
    pub fn submit_with_gas(
        &self,
        tx: &BuiltTransaction,
        keys: &[&Keypair],
        payer: Option<&Keypair>,
        gas_limit: u64,
    ) -> Result<Submitted> {
        let fee = payer.map(|p| FeeDeclaration::with_gas(p.account_id(), gas_limit));
        let order = sign::signing_order(tx, fee.as_ref().map(|f| &f.payer));
        let nonces = self.nonces(&order)?;
        let mut all: Vec<&Keypair> = keys.to_vec();
        if let Some(p) = payer {
            all.push(p);
        }
        let bytes = sign::sign_transaction(tx, &all, &nonces, fee.as_ref())
            .map_err(|e| Error::Malformed(e.to_string()))?;
        let hash = self.send(&bytes)?;
        let block = self.wait_included(&hash, std::time::Duration::from_secs(90))?;
        Ok(Submitted { hash, block })
    }

    /// Wait until a transaction is included, polling every 500 ms.
    pub fn wait_included(&self, hash: &str, timeout: std::time::Duration) -> Result<u64> {
        let start = std::time::Instant::now();
        loop {
            if let Some(block) = self.transaction_block(hash)? {
                return Ok(block);
            }
            if start.elapsed() > timeout {
                return Err(Error::Timeout(format!("transaction {hash} not included")));
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REGISTRY: ProgramId = [0x42; 32];

    #[test]
    fn from_base58_is_the_inverse_of_base58() {
        let addr = entry_address(&REGISTRY, &[7u8; 32]);
        let s = base58(&addr);
        assert_eq!(from_base58(&s), Some(addr));
        assert_eq!(from_base58(&format!("Public/{s}")), Some(addr));
        assert_eq!(from_base58("too-short"), None);
        assert_eq!(from_base58("0OIl"), None, "0, O, I, l are not base58 digits");
    }

    #[test]
    fn base58_keeps_leading_zeroes() {
        let mut b = [0u8; 32];
        b[31] = 1;
        let s = base58(&b);
        assert_eq!(s.len(), 32, "31 leading zero bytes plus one digit");
        assert!(s.ends_with('2'));
        assert_eq!(base58(&NATIVE_TOKEN), "11111111111111111111111111111111");
    }

    #[test]
    fn the_system_accounts_match_what_the_sequencer_prints() {
        // Read off a v0.3.0 standalone sequencer's getAccount responses.
        assert_eq!(base58(&CLOCK_01), "4BdcjoXkq786TMWcBGGHqcxeLYMZmn17rL4eM9ZyRWNU");
        assert_eq!(
            base58(&clock_program()),
            "2hwaRW8sMnyKLLRbDRw8k68tiv9bnfq98oxcdtcp157Z"
        );
        assert_eq!(
            base58(&PROGRAM_LOADER),
            "JAQ5MVHbCkSYRzXunsrNuM2m1LS859PGveHfoYPAmcvZ"
        );
    }

    #[test]
    fn a_third_party_entry_never_collides_with_the_deployer_entry() {
        let program = [3u8; 32];
        assert_ne!(
            entry_address(&REGISTRY, &program),
            third_party_entry_address(&REGISTRY, &program, &[7u8; 32]),
        );
    }

    #[test]
    fn two_registries_give_the_same_program_different_addresses() {
        assert_ne!(
            entry_address(&REGISTRY, &[3u8; 32]),
            entry_address(&[9u8; 32], &[3u8; 32])
        );
    }

    #[test]
    fn a_key_resolves_to_the_same_address_as_the_named_functions() {
        let p = [3u8; 32];
        let a = [4u8; 32];
        assert_eq!(
            key_address(&REGISTRY, &EntryKey { program: p, publisher: None }),
            entry_address(&REGISTRY, &p)
        );
        assert_eq!(
            key_address(&REGISTRY, &EntryKey { program: p, publisher: Some(a) }),
            third_party_entry_address(&REGISTRY, &p, &a)
        );
    }

    #[test]
    fn hex_parsing_round_trips() {
        let b = [0xab; 32];
        assert_eq!(from_hex32(&hex(&b)), Some(b));
        assert_eq!(from_hex32(&format!("0x{}", hex(&b))), Some(b));
        assert_eq!(parse_account(&base58(&b)), Some(b));
        assert_eq!(from_hex32("abc"), None);
    }
}
