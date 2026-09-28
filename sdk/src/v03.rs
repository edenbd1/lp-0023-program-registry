//! Reading a program on LEZ 0.3, where a program is an account.
//!
//! On 0.3 (v0.3.0-rc1) a deployed program lives at a *header account* the
//! deployer chose and signed for. The header sits in that account's
//! `program_loader` shard and records the `image_id` the runtime recomputed from
//! the bytecode segments — never a caller-supplied value — plus whether the
//! header is frozen. So the check a verifier needs, "does my rebuild produce the
//! bytecode running at this program id?", is one account read on 0.3:
//! [`Client::program_header`], then [`ProgramHeader::matches`].
//!
//! `getAccount` on 0.3 returns `{ nonce, data: { shards: { <program b58>: [bytes] } } }`.
//! The loader's account id is `[0xFE; 32]`
//! (`lee/state_machine/core/src/program/mod.rs`, `PROGRAM_LOADER_ACCOUNT_ID`).

use crate::{base58, Client, Error, Result};
use lez_registry_core::AccountId;

/// `PROGRAM_LOADER_ACCOUNT_ID` on LEZ 0.3.
pub const PROGRAM_LOADER: AccountId = [0xFE; 32];

/// A program header as the 0.3 loader stores it: borsh of
/// `{ image_id: [u32; 8], program_first_segment: AccountId, immutable: bool }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgramHeader {
    /// The ImageID as bytes — the same 32 bytes `spel program-id` prints as
    /// "ImageID (hex bytes)".
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

    /// A header read from a local v0.3.0-rc1 standalone sequencer after
    /// deploying `ping_receiver.bin`, whose `spel program-id` prints
    /// ImageID (hex bytes) 9956be7d0f541db26627fa077e369f7a103cc0735d673fb25b698881894e5cea.
    const LOADER_SHARD: [u8; 65] = [
        153, 86, 190, 125, 15, 84, 29, 178, 102, 39, 250, 7, 126, 54, 159, 122, 16, 60, 192, 115,
        93, 103, 63, 178, 91, 105, 136, 129, 137, 78, 92, 234, 30, 32, 252, 228, 24, 232, 194, 217,
        146, 101, 169, 155, 246, 82, 182, 105, 130, 25, 231, 247, 148, 187, 160, 30, 222, 203, 11,
        0, 68, 180, 85, 197, 0,
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
            "9956be7d0f541db26627fa077e369f7a103cc0735d673fb25b698881894e5cea"
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

    /// Against a live 0.3 zone: `LEZ_V03_ENDPOINT` and `LEZ_V03_PROGRAM` (b58).
    #[test]
    #[ignore = "needs a LEZ 0.3 sequencer"]
    fn a_live_header_reads_back() {
        // Opt-in: the public zone is still v0.2.4, so the e2e job that runs
        // every ignored test has no 0.3 sequencer to point this at. It runs
        // only when both variables name one, and says so when it does not.
        let (Ok(ep), Ok(prog)) = (
            std::env::var("LEZ_V03_ENDPOINT"),
            std::env::var("LEZ_V03_PROGRAM"),
        ) else {
            eprintln!("skipped: set LEZ_V03_ENDPOINT and LEZ_V03_PROGRAM to a 0.3 zone and a deployed program");
            return;
        };
        let pid = crate::from_base58(&prog).expect("a base58 account id");
        let c = Client::new(ep, [0u8; 32]);
        let h = c.program_header(&pid).expect("answered").expect("a header");
        println!("image_id {:02x?} immutable {}", h.image_id, h.immutable);
    }
}
