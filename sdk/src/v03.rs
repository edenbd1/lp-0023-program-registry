//! Reading a program on LEZ v0.3, where a program is an account.
//!
//! On LEZ v0.3 a deployed program lives at a *header account* the
//! deployer chose and signed for. The header sits in that account's
//! `program_loader` shard and records the `image_id` the runtime recomputed from
//! the bytecode segments, never a caller-supplied value, plus whether the
//! header is frozen. So the check a verifier needs, "does my rebuild produce the
//! bytecode running at this program id?", is one account read on 0.3:
//! [`Client::program_header`], then [`ProgramHeader::matches`].
//!
//! `getAccount` on 0.3 returns `{ nonce, data: { shards: { <program b58>: [bytes] } } }`.
//! The loader's account id is `[0xFE; 32]`
//! (`lee/state_machine/core/src/program/mod.rs`, `PROGRAM_LOADER_ACCOUNT_ID`).

use crate::{base58, Client, Error, HeaderView, Result};
use lez_registry_core::AccountId;

/// `PROGRAM_LOADER_ACCOUNT_ID` on LEZ 0.3.
pub const PROGRAM_LOADER: AccountId = [0xFE; 32];

/// A program header as the 0.3 loader stores it: borsh of
/// `{ image_id: [u32; 8], program_first_segment: AccountId, immutable: bool }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgramHeader {
    /// The ImageID as bytes: the 32 bytes `lez-registry image-id` prints.
    pub image_id: [u8; 32],
    pub first_segment: AccountId,
    /// Once true the header can never be updated again, so a verification of
    /// it can never go stale.
    pub immutable: bool,
}

impl ProgramHeader {
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != 65 || bytes[64] > 1 {
            return None;
        }
        let mut image_id = [0u8; 32];
        image_id.copy_from_slice(&bytes[..32]);
        let mut first_segment = [0u8; 32];
        first_segment.copy_from_slice(&bytes[32..64]);
        Some(Self {
            image_id,
            first_segment,
            immutable: bytes[64] == 1,
        })
    }

    /// Whether a rebuilt ImageID is the one running at this program id.
    pub fn matches(&self, rebuilt_image_id: &[u8; 32]) -> bool {
        &self.image_id == rebuilt_image_id
    }

    /// The part of the header the registry records and checks on chain.
    #[must_use]
    pub const fn view(&self) -> HeaderView {
        HeaderView {
            image_id: self.image_id,
            immutable: self.immutable,
        }
    }
}

/// The bytes of one program's shard of an account, from a 0.3 `getAccount`
/// result. `None` when that program has written nothing there.
pub fn shard(result: &serde_json::Value, program: &AccountId) -> Option<Vec<u8>> {
    let arr = result
        .get("data")?
        .get("shards")?
        .get(base58(program))?
        .as_array()?;
    arr.iter()
        .map(|v| v.as_u64().and_then(|n| u8::try_from(n).ok()))
        .collect()
}

impl Client {
    /// The program header at `program` on a 0.3 zone, or `None` when that
    /// account holds no header (not a program, or not deployed yet).
    pub fn program_header(&self, program: &AccountId) -> Result<Option<ProgramHeader>> {
        let resp = self.get_account_json(program)?;
        let Some(result) = resp.get("result") else {
            return Ok(None);
        };
        match shard(result, &PROGRAM_LOADER) {
            None => Ok(None),
            Some(b) => ProgramHeader::decode(&b)
                .map(Some)
                .ok_or_else(|| Error::Malformed("loader shard is not a program header".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The registry's own header, read from a local v0.3.0 standalone
    /// sequencer after `wallet program-loader deploy` (the e2e run in
    /// `evidence/e2e-v03-local.txt`); `lez-registry image-id` prints
    /// 0e3fe974e184e40062811f28c4d8c17df208bf80ee0ce7ee09148aa0f8dfa53d for that binary.
    const LOADER_SHARD: [u8; 65] = [
        14, 63, 233, 116, 225, 132, 228, 0, 98, 129, 31, 40, 196, 216, 193, 125, 242, 8, 191, 128,
        238, 12, 231, 238, 9, 20, 138, 160, 248, 223, 165, 61, 229, 174, 242, 14, 1, 20, 124, 37,
        122, 142, 197, 208, 45, 70, 202, 15, 71, 173, 45, 78, 168, 195, 50, 53, 37, 205, 231, 202,
        250, 124, 112, 50, 0,
    ];

    fn hexbytes(s: &str) -> [u8; 32] {
        let mut o = [0u8; 32];
        for (i, c) in s.as_bytes().chunks(2).enumerate() {
            o[i] = u8::from_str_radix(std::str::from_utf8(c).unwrap(), 16).unwrap();
        }
        o
    }

    #[test]
    fn a_header_decodes_and_matches_the_rebuilt_image_id() {
        let h = ProgramHeader::decode(&LOADER_SHARD).expect("a real header");
        assert!(!h.immutable);
        assert!(h.matches(&hexbytes(
            "0e3fe974e184e40062811f28c4d8c17df208bf80ee0ce7ee09148aa0f8dfa53d"
        )));
        let mut other = h.image_id;
        other[0] ^= 1;
        assert!(!h.matches(&other), "one flipped bit is a different program");
    }

    #[test]
    fn anything_else_in_the_loader_shard_is_refused() {
        assert_eq!(ProgramHeader::decode(&LOADER_SHARD[..64]), None);
        let mut bad = LOADER_SHARD;
        bad[64] = 2;
        assert_eq!(ProgramHeader::decode(&bad), None, "immutable is a bool");
    }

    #[test]
    fn the_shard_is_read_by_the_owning_programs_account_id() {
        let json: serde_json::Value = serde_json::json!({
            "nonce": 1,
            "data": { "shards": {
                "11111111111111111111111111111111": [232, 3, 0, 0],
                base58(&PROGRAM_LOADER): LOADER_SHARD.to_vec(),
            }}
        });
        assert_eq!(
            shard(&json, &PROGRAM_LOADER).as_deref(),
            Some(&LOADER_SHARD[..])
        );
        assert_eq!(shard(&json, &[7u8; 32]), None);
    }
}
