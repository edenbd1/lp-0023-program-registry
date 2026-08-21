// SPDX-License-Identifier: MIT OR Apache-2.0
//! Resolve a LEZ program registry entry.
//!
//! The one thing worth knowing about this SDK is what it does **not** contain: a
//! search index, a cache, or a server to ask. An entry's address is a pure
//! function of the registry's program id and the program id you are asking
//! about, so resolution is a hash and a single account read. There is nothing in
//! the path that can be stale, censor a program, or go down separately from the
//! chain.
//!
//! A search index exists — see the `indexer` — but it is an accelerator for
//! *browsing*, never a dependency for *resolving*. Anything you found through the
//! index can be re-derived here and checked against the chain.

use lez_registry_core as core_;
pub use lez_registry_core::{
    AccountId, Entry, GlobalState, ProgramId, Publisher, VerificationState,
};
use sha2::{Digest, Sha256};

/// Domain separator LEZ hashes into every program-derived address.
///
/// Copied from the runtime rather than invented: `lee/state_machine/core`,
/// `AccountId::for_public_pda`. The golden test below pins it against an address
/// that exists on the public testnet, so a runtime change breaks the test rather
/// than silently returning addresses nothing lives at.
const PDA_PREFIX: &[u8; 32] = b"/LEE/v0.2/AccountId/PDA/\x00\x00\x00\x00\x00\x00\x00\x00";

/// Derive a public program-derived address.
#[must_use]
pub fn pda(program_id: &ProgramId, seed: &[u8; 32]) -> AccountId {
    let mut buf = [0u8; 96];
    buf[..32].copy_from_slice(PDA_PREFIX);
    buf[32..64].copy_from_slice(program_id);
    buf[64..].copy_from_slice(seed);
    Sha256::digest(buf).into()
}

/// Where a program's canonical entry lives in a given registry.
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

/// Where the `n`th registration recorded which program it was.
#[must_use]
pub fn directory_address(registry: &ProgramId, n: u64) -> AccountId {
    pda(registry, &core_::seq_seed(n))
}

/// The registry's singleton counters account.
#[must_use]
pub fn global_address(registry: &ProgramId) -> AccountId {
    pda(registry, &core_::global_seed())
}

/// Base58, for pasting into an explorer or a `spel` command.
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

#[derive(Debug)]
pub enum Error {
    Transport(String),
    Malformed(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(m) => write!(f, "transport: {m}"),
            Self::Malformed(m) => write!(f, "malformed response: {m}"),
        }
    }
}
impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// A read-only client for one registry deployment.
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

    /// Raw account data, or `None` when the account holds nothing.
    ///
    /// A derived address always *exists* as an address; what distinguishes a
    /// registered program from an unregistered one is whether anything was
    /// written there. Empty and absent are the same answer and are returned as
    /// the same value.
    pub fn account_data(&self, id: &AccountId) -> Result<Option<Vec<u8>>> {
        let body = serde_json::json!({
            "jsonrpc": "2.0", "id": 1,
            "method": "getAccount",
            "params": [base58(id)],
        });
        let resp: serde_json::Value = ureq::post(&self.endpoint)
            .set("content-type", "application/json")
            .send_string(&body.to_string())
            .map_err(|e| Error::Transport(e.to_string()))?
            .into_string()
            .map_err(|e| Error::Transport(e.to_string()))
            .and_then(|t| serde_json::from_str(&t).map_err(|e| Error::Malformed(e.to_string())))?;

        let Some(result) = resp.get("result") else {
            return Ok(None);
        };
        let Some(data) = result.get("data").and_then(|d| d.as_array()) else {
            return Ok(None);
        };
        if data.is_empty() {
            return Ok(None);
        }
        let bytes: Option<Vec<u8>> = data
            .iter()
            .map(|v| v.as_u64().and_then(|n| u8::try_from(n).ok()))
            .collect();
        bytes
            .ok_or_else(|| Error::Malformed("account data is not a byte array".into()))
            .map(Some)
    }

    /// Resolve a program's canonical entry. One hash, one read, no index.
    pub fn entry(&self, program: &ProgramId) -> Result<Option<Entry>> {
        let addr = entry_address(&self.registry, program);
        self.entry_at(&addr)
    }

    /// Resolve one publisher's entry about a program.
    pub fn third_party_entry(
        &self,
        program: &ProgramId,
        publisher: &AccountId,
    ) -> Result<Option<Entry>> {
        let addr = third_party_entry_address(&self.registry, program, publisher);
        self.entry_at(&addr)
    }

    fn entry_at(&self, addr: &AccountId) -> Result<Option<Entry>> {
        let Some(raw) = self.account_data(addr)? else {
            return Ok(None);
        };
        // On chain the entry is wrapped: a borsh Vec<u8> whose contents are the
        // Entry. Unwrap one layer before decoding.
        let payload: Vec<u8> =
            borsh::from_slice(&raw).map_err(|e| Error::Malformed(e.to_string()))?;
        borsh::from_slice(&payload)
            .map(Some)
            .map_err(|e| Error::Malformed(e.to_string()))
    }

    /// What registered `n`th, if anything did: the program id and the seed of
    /// the entry it created.
    pub fn slot_at(&self, n: u64) -> Result<Option<core_::DirectorySlot>> {
        let addr = directory_address(&self.registry, n);
        let Some(raw) = self.account_data(&addr)? else {
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
    /// entry that fails to decode is skipped rather than aborting the walk — one
    /// bad account must not make the rest of the registry unreadable.
    pub fn walk(&self) -> Result<Vec<Entry>> {
        let total = self.global()?.map_or(0, |g| g.total_entries);
        let mut out = Vec::new();
        for n in 0..total {
            let Some(slot) = self.slot_at(n)? else {
                continue;
            };
            // The slot's seed, not the program id: deriving from the program id
            // would resolve every third-party entry to the canonical one and
            // quietly drop it from the listing.
            let addr = pda(&self.registry, &slot.entry_seed);
            if let Some(entry) = self.entry_at(&addr)? {
                out.push(entry);
            }
        }
        Ok(out)
    }

    /// The registry's counters.
    pub fn global(&self) -> Result<Option<GlobalState>> {
        let addr = global_address(&self.registry);
        let Some(raw) = self.account_data(&addr)? else {
            return Ok(None);
        };
        borsh::from_slice(&raw)
            .map(Some)
            .map_err(|e| Error::Malformed(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The registry deployed at block 17428, its entry, and its global account.
    /// These are addresses that exist on the public testnet, so this test fails
    /// if the derivation drifts from the runtime's — which is the only way to
    /// notice, since a wrong derivation still returns a perfectly well-formed
    /// address that simply has nothing at it.
    /// The registry deployed at block 17862. One place, in hex, because the
    /// addresses below are *derived* from it — writing both the id and the
    /// addresses by hand meant they could disagree, and three times in one day
    /// they did after a redeploy.
    const REGISTRY_HEX: &str = "c59dd9c30e93fd2ec6c89f4d3da94bf1084f822a1f1afb4703253ddd92101396";

    fn registry() -> ProgramId {
        let mut out = [0u8; 32];
        for (i, b) in out.iter_mut().enumerate() {
            *b = u8::from_str_radix(&REGISTRY_HEX[i * 2..i * 2 + 2], 16).expect("valid hex");
        }
        out
    }

    #[test]
    fn the_derivation_reproduces_a_live_testnet_address() {
        assert_eq!(
            base58(&entry_address(&registry(), &registry())),
            "ECfj83o2bMh48F318oStb5qvA9ep7BfRd2v59V4AyQqg",
            "canonical entry of the registry's own program"
        );
        assert_eq!(
            base58(&global_address(&registry())),
            "5sKC3dtdTfSCGc715gFyb41anbn1iLtg6U47WQWnYE4X",
            "the singleton global account"
        );
    }

    #[test]
    fn base58_keeps_leading_zeroes() {
        // A leading zero byte is significant and encodes as '1'. Dropping it
        // yields a shorter string that decodes to a different account.
        let mut b = [0u8; 32];
        b[31] = 1;
        let s = base58(&b);
        assert_eq!(s.len(), 32, "31 leading zero bytes plus one digit");
        assert!(s.starts_with("1111"));
        assert!(s.ends_with('2'), "0x01 is the second base58 digit");
    }

    #[test]
    fn a_third_party_entry_never_collides_with_the_canonical_one() {
        let publisher = [7u8; 32];
        assert_ne!(
            entry_address(&registry(), &registry()),
            third_party_entry_address(&registry(), &registry(), &publisher),
            "a stranger must not be able to occupy the canonical address"
        );
    }

    #[test]
    fn two_registries_give_the_same_program_different_addresses() {
        // Entries are scoped to the registry that holds them, so a second
        // deployment cannot answer for the first.
        let other = [9u8; 32];
        assert_ne!(
            entry_address(&registry(), &registry()),
            entry_address(&other, &registry())
        );
    }
}
