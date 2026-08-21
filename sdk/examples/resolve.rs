// SPDX-License-Identifier: MIT OR Apache-2.0
//! The integration path most consumers take, end to end.
//!
//! A wallet about to render a transaction, or an explorer decoding one, has one
//! question: **what is this program, and is the source it names the source it
//! was built from?** This is that, in about twenty lines of real code.
//!
//! Run it: `cargo run --example resolve -p lez-registry-sdk -- <program-id-hex>`

use lez_registry_sdk::{base58, entry_address, Client, ProgramId, VerificationState};

/// The registry to ask. A consumer would put their own here, or take it from
/// configuration; there is nothing special about this one except that it is the
/// one running on the public testnet.
const REGISTRY: &str = "f59751e932e856f7565f34e79ae726791794dc68e55f2413bc0375008cc07bf8";
const ENDPOINT: &str = "https://testnet.lez.logos.co";

fn hex32(s: &str) -> Option<ProgramId> {
    let s = s.trim().trim_start_matches("0x");
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

fn main() {
    let arg = std::env::args().nth(1).unwrap_or_else(|| REGISTRY.into());
    let Some(program) = hex32(&arg) else {
        eprintln!("usage: resolve <program-id-hex>");
        std::process::exit(2);
    };
    let registry = hex32(REGISTRY).expect("built-in registry id is valid hex");

    // 1. Where the answer lives. This is a pure function of the two program ids
    //    -- no request, no index, nothing that can be down or out of date.
    let addr = entry_address(&registry, &program);
    println!("entry account  {}", base58(&addr));

    // 2. Read it.
    let client = Client::new(ENDPOINT, registry);
    let entry = match client.entry(&program) {
        Ok(Some(e)) => e,
        // Not an error. A derived address always exists; what separates a
        // registered program from an unregistered one is whether anything was
        // written there. A wallet renders this as "unknown program", which is a
        // truthful thing to tell a user about to sign.
        Ok(None) => {
            println!("not registered — render this as an unknown program");
            return;
        }
        Err(e) => {
            eprintln!("could not reach the registry: {e}");
            std::process::exit(1);
        }
    };

    println!("name           {} {}", entry.name, entry.version);
    println!("description    {}", entry.description);
    if let Some(idl) = entry.idl {
        // The IDL itself lives on Logos Storage; the entry holds the identifier.
        // Fetch it to decode calldata into named arguments.
        println!("idl            {}", base58_cid(&idl));
    }

    // 3. The part that decides what a UI is allowed to say.
    match entry.verification.state {
        VerificationState::Verified if entry.verification.is_self_attested(&entry.author) => {
            println!("verification   verified, but only by the party that shipped it");
        }
        VerificationState::Verified => {
            println!("verification   verified by an independent rebuild");
        }
        VerificationState::Mismatched => {
            println!("verification   MISMATCHED — a source is published and does not rebuild");
        }
        VerificationState::Unreproducible => {
            println!(
                "verification   a rebuild was tried with the network up, so it proves nothing"
            );
        }
        VerificationState::Claimed => {
            println!("verification   a source is claimed, nobody rebuilt it")
        }
        VerificationState::Unclaimed => println!("verification   no source claimed"),
    }
    if let Some(who) = entry.verification.attester {
        println!("attested by    {}", base58(&who));
    }
    if let Some(claim) = &entry.verification.claim {
        println!("source         {}", claim.repository);
    }

    // Say this wherever the state is shown. Verified means the deployed bytecode
    // was produced from the stated source; it says nothing about whether that
    // source is safe, audited or benign.
    println!("\nVerified is not safe. It means the bytecode matches the source, nothing more.");
}

/// A content identifier renders as multibase base58btc — the `z…` a Logos
/// Storage node accepts.
fn base58_cid(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    let mut digits: Vec<u8> = Vec::new();
    for &byte in bytes {
        let mut carry = u32::from(byte);
        for d in &mut digits {
            carry += u32::from(*d) << 8;
            *d = (carry % 58) as u8;
            carry /= 58;
        }
        while carry > 0 {
            digits.push((carry % 58) as u8);
            carry /= 58;
        }
    }
    let mut out = String::from("z");
    out.extend(digits.iter().rev().map(|d| ALPHABET[*d as usize] as char));
    out
}
