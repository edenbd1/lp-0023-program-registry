// SPDX-License-Identifier: MIT OR Apache-2.0
//! Sign a registry transaction for LEZ v0.3, in process.
//!
//! A v0.3 public transaction is
//!
//! ```text
//! LeeTransaction::Public(PublicTransaction {
//!     message: Message { program_account_id, shard_selectors, nonces,
//!                        instruction_data, fee: Option<FeeDeclaration> },
//!     witness_set: Vec<(Signature[64], PublicKey[32])>,
//! })
//! ```
//!
//! borsh-encoded, and `sendTransaction` takes that as base64. Each witness is a
//! BIP-340 Schnorr signature over `sha256(PREFIX ‖ borsh(message))`, and nonces
//! pair with witnesses by position. The fee payer co-signs like any other
//! signer; it need not be one of the transaction's accounts.
//!
//! This is written out rather than linked from `lee` so that a wallet, the
//! Basecamp module or a script can register a program without pulling the whole
//! LEZ state machine in. The layout is taken from `lee/state_machine/src`
//! (`public_transaction/message.rs`, `witness_set.rs`, `fees.rs`,
//! `signature/`) at v0.3.0 and is exercised against a real v0.3.0 sequencer by
//! `scripts/e2e-v03-local.sh`, which accepts nothing that is not byte-exact.

use crate::tx::BuiltTransaction;
use crate::AccountId;
use k256::schnorr::SigningKey;
use sha2::{Digest, Sha256};

const MESSAGE_PREFIX: &[u8; 32] = b"/LEE/v0.3/Message/Public/\x00\x00\x00\x00\x00\x00\x00";
const PUBLIC_ACCOUNT_PREFIX: &[u8; 32] = b"/LEE/v0.3/AccountId/Public/\x00\x00\x00\x00\x00";

/// The wallet's default gas limit (`DEFAULT_GAS_LIMIT`, v0.3.0).
pub const DEFAULT_GAS_LIMIT: u64 = 2_000_000;

/// The wallet's fee cap for a gas limit (`max_fee_for`, v0.3.0): enough to cover
/// the gas plus 100 kB of data at a base fee of 64.
#[must_use]
pub const fn max_fee_for(gas_limit: u64) -> u128 {
    (gas_limit as u128 + 100_000) * 64
}

/// A public-account signing key.
pub struct Keypair {
    key: SigningKey,
    secret: [u8; 32],
}

impl Keypair {
    /// From a 32-byte secret, the form `wallet account import public
    /// --private-key <hex>` takes. `None` if it is not a valid secp256k1 scalar.
    #[must_use]
    pub fn from_secret(secret: &[u8; 32]) -> Option<Self> {
        SigningKey::from_bytes(secret).ok().map(|key| Self {
            key,
            secret: *secret,
        })
    }

    /// From 64 hex characters.
    #[must_use]
    pub fn from_hex(s: &str) -> Option<Self> {
        crate::from_hex32(s).and_then(|b| Self::from_secret(&b))
    }

    /// A fresh random key, from the operating system's RNG.
    #[must_use]
    pub fn generate() -> Self {
        loop {
            let mut b = [0u8; 32];
            if getrandom::getrandom(&mut b).is_ok() {
                if let Some(k) = Self::from_secret(&b) {
                    return k;
                }
            }
        }
    }

    /// The 32-byte secret, hex, for handing to `wallet account import`.
    #[must_use]
    pub fn secret_hex(&self) -> String {
        crate::hex(&self.secret)
    }

    /// The x-only public key.
    #[must_use]
    pub fn public_key(&self) -> [u8; 32] {
        self.key.verifying_key().to_bytes().into()
    }

    /// The public account id this key controls.
    #[must_use]
    pub fn account_id(&self) -> AccountId {
        account_id_of(&self.public_key())
    }

    /// BIP-340 over a 32-byte prehash. The auxiliary randomness is derived from
    /// the secret and the message, which BIP-340 permits; it only masks the
    /// nonce and never has to be random for the signature to be sound.
    #[must_use]
    pub fn sign(&self, prehash: &[u8; 32]) -> [u8; 64] {
        let mut h = Sha256::new();
        h.update(b"provenance/aux");
        h.update(self.secret);
        h.update(prehash);
        let aux: [u8; 32] = h.finalize().into();
        self.key
            .sign_prehash_with_aux_rand(prehash, &aux)
            .expect("a valid key signs any 32-byte prehash")
            .to_bytes()
    }
}

/// `AccountId::from(&PublicKey)` at v0.3.0.
#[must_use]
pub fn account_id_of(public_key: &[u8; 32]) -> AccountId {
    let mut h = Sha256::new();
    h.update(PUBLIC_ACCOUNT_PREFIX);
    h.update(public_key);
    h.finalize().into()
}

/// The signed fee fields of a charged transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeeDeclaration {
    pub payer: AccountId,
    pub gas_limit: u64,
    pub tip: u64,
    pub max_fee: u128,
}

impl FeeDeclaration {
    /// The wallet's defaults, paid by `payer`.
    #[must_use]
    pub const fn default_for(payer: AccountId) -> Self {
        Self {
            payer,
            gas_limit: DEFAULT_GAS_LIMIT,
            tip: 0,
            max_fee: max_fee_for(DEFAULT_GAS_LIMIT),
        }
    }
}

fn put_u32(out: &mut Vec<u8>, n: usize) {
    out.extend_from_slice(&u32::try_from(n).expect("length fits u32").to_le_bytes());
}

/// borsh of the v0.3 public `Message`.
#[must_use]
pub fn message_bytes(tx: &BuiltTransaction, nonces: &[u128], fee: Option<&FeeDeclaration>) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&tx.registry);
    put_u32(&mut out, tx.accounts.len());
    for a in &tx.accounts {
        out.extend_from_slice(&a.selector.account);
        out.extend_from_slice(&a.selector.program);
    }
    put_u32(&mut out, nonces.len());
    for n in nonces {
        out.extend_from_slice(&n.to_le_bytes());
    }
    put_u32(&mut out, tx.instruction_data.len());
    out.extend_from_slice(&tx.instruction_data);
    match fee {
        None => out.push(0),
        Some(f) => {
            out.push(1);
            out.extend_from_slice(&f.payer);
            out.extend_from_slice(&f.gas_limit.to_le_bytes());
            out.extend_from_slice(&f.tip.to_le_bytes());
            out.extend_from_slice(&f.max_fee.to_le_bytes());
        }
    }
    out
}

/// `Message::hash` at v0.3.0: what every witness signs.
#[must_use]
pub fn message_hash(message: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(MESSAGE_PREFIX);
    h.update(message);
    h.finalize().into()
}

/// The signing order for a transaction: its signing accounts in declared
/// order, then the fee payer if it is not already one of them. Nonces are
/// fetched and passed in this same order.
#[must_use]
pub fn signing_order(tx: &BuiltTransaction, payer: Option<&AccountId>) -> Vec<AccountId> {
    let mut ids = tx.signer_ids();
    if let Some(p) = payer {
        if !ids.contains(p) {
            ids.push(*p);
        }
    }
    ids
}

/// Why a transaction could not be signed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignError {
    /// No key was supplied for an account that must sign.
    MissingKey(AccountId),
    /// The nonce list does not pair up with the signers.
    NonceCount { signers: usize, nonces: usize },
}

impl std::fmt::Display for SignError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingKey(id) => write!(f, "no key for signer {}", crate::base58(id)),
            Self::NonceCount { signers, nonces } => {
                write!(f, "{signers} signers but {nonces} nonces")
            }
        }
    }
}

impl std::error::Error for SignError {}

/// Sign `tx` and return the bytes `sendTransaction` takes (before base64).
///
/// `keys` must hold a key for every account in [`signing_order`]; `nonces`
/// are those accounts' current nonces in that order.
pub fn sign_transaction(
    tx: &BuiltTransaction,
    keys: &[&Keypair],
    nonces: &[u128],
    fee: Option<&FeeDeclaration>,
) -> Result<Vec<u8>, SignError> {
    let order = signing_order(tx, fee.map(|f| &f.payer));
    if order.len() != nonces.len() {
        return Err(SignError::NonceCount {
            signers: order.len(),
            nonces: nonces.len(),
        });
    }
    let message = message_bytes(tx, nonces, fee);
    let hash = message_hash(&message);
    let mut out = Vec::with_capacity(message.len() + 8 + order.len() * 96);
    out.push(0); // LeeTransaction::Public
    out.extend_from_slice(&message);
    put_u32(&mut out, order.len());
    for id in &order {
        let key = keys
            .iter()
            .find(|k| &k.account_id() == id)
            .ok_or(SignError::MissingKey(*id))?;
        out.extend_from_slice(&key.sign(&hash));
        out.extend_from_slice(&key.public_key());
    }
    Ok(out)
}

/// Standard base64 with padding, as the sequencer's `borsh_base64` expects.
#[must_use]
pub fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The genesis test account of LEZ's standalone config: this secret is
    /// published in the LEZ Justfile, and its account id is the one the debug
    /// sequencer config funds.
    #[test]
    fn the_published_test_key_controls_the_funded_genesis_account() {
        let k = Keypair::from_hex(
            "7f273098f25b71e6c005a9519f2678da8d1c7f01f6a27778e2d9948abdf901fb",
        )
        .unwrap();
        assert_eq!(
            crate::base58(&k.account_id()),
            "CbgR6tj5kWx5oziiFptM7jMvrQeYY3Mzaao6ciuhSr2r"
        );
    }

    #[test]
    fn a_signature_verifies_under_bip340() {
        use k256::schnorr::signature::hazmat::PrehashVerifier;
        let k = Keypair::from_secret(&[7u8; 32]).unwrap();
        let msg = message_hash(b"anything");
        let sig = k.sign(&msg);
        let vk = k256::schnorr::VerifyingKey::from_bytes(&k.public_key()).unwrap();
        let sig = k256::schnorr::Signature::try_from(sig.as_slice()).unwrap();
        assert!(vk.verify_prehash(&msg, &sig).is_ok());
    }

    #[test]
    fn base64_matches_the_rfc_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn the_message_prefix_is_32_bytes_like_the_runtime_asserts() {
        assert_eq!(MESSAGE_PREFIX.len(), 32);
        assert_eq!(PUBLIC_ACCOUNT_PREFIX.len(), 32);
    }
}
