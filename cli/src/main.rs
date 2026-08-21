// SPDX-License-Identifier: MIT OR Apache-2.0
//! Registry CLI.
//!
//! It computes addresses and verifies builds; it does not sign transactions.
//! Submission goes through `spel` with the registry's IDL, which is the tool the
//! ecosystem already has — reimplementing transaction construction here would
//! be a second thing to keep correct for no gain.
//!
//! Everything it prints comes from `lez-registry-core`, the same crate the
//! on-chain program links, so an address this tool shows is the address the
//! program will insist on.

use lez_registry_core as core_;
use std::process::ExitCode;

const BASE58: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

fn to_base58(bytes: &[u8; 32]) -> String {
    let mut n = [0u8; 32];
    n.copy_from_slice(bytes);
    let mut out = Vec::new();
    let mut value = n.to_vec();
    while value.iter().any(|&b| b != 0) {
        let mut rem = 0u32;
        for byte in value.iter_mut() {
            let cur = rem * 256 + u32::from(*byte);
            *byte = (cur / 58) as u8;
            rem = cur % 58;
        }
        out.push(BASE58[rem as usize]);
    }
    for &b in bytes.iter() {
        if b != 0 {
            break;
        }
        out.push(BASE58[0]);
    }
    out.reverse();
    String::from_utf8(out).expect("base58 alphabet is ascii")
}

fn from_hex(s: &str) -> Option<[u8; 32]> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
        out[i] = u8::from_str_radix(std::str::from_utf8(chunk).ok()?, 16).ok()?;
    }
    Some(out)
}

fn usage() -> ExitCode {
    eprintln!(
        "\
lez-registry — addresses and build verification for the LEZ program registry

  deployment-digest <FILE>
        SHA256(len_le ‖ bytes) — what the DEPLOY TRANSACTION hashes to. Use it
        to check that the file you hold is the file that was deployed.

        This is NOT the program id. A program is called by its RISC0 ImageID,
        derived from the ELF rather than from the packaged file; read that with
        `spel program-id`. Keying a registry on the wrong one of these would
        resolve nothing, so this tool refuses to blur them.

  entry-address <PROGRAM_ID_HEX> [--publisher <ACCOUNT_ID_HEX>]
        Where the entry lives. Without --publisher this is the canonical
        address, a function of the program id alone — so a client resolves a
        program by computing, never by asking an index. With --publisher it is
        that party's third-party entry, which is why a stranger can never
        occupy the canonical one.

  global-address
        The singleton registry-state address.

  verify <FILE> --expect <PROGRAM_ID_HEX>
        Does this bytecode's deployment digest match the one given? Necessary
        and not sufficient: see `verify --explain`.

  verify --explain
        Why a hash match is not proof of provenance."
    );
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first().map(String::as_str) else {
        return usage();
    };

    match cmd {
        "deployment-digest" => {
            let Some(path) = args.get(1) else {
                return usage();
            };
            match std::fs::read(path) {
                Ok(bytes) => {
                    let id = core_::deployment_digest(&bytes);
                    println!("{}", hex(&id));
                    println!("  {} bytes", bytes.len());
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("cannot read {path}: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        "entry-address" => {
            let Some(pid) = args.get(1).and_then(|s| from_hex(s)) else {
                eprintln!("program id must be 64 hex characters");
                return ExitCode::FAILURE;
            };
            let publisher = args
                .iter()
                .position(|a| a == "--publisher")
                .and_then(|i| args.get(i + 1))
                .and_then(|s| from_hex(s));
            let seed = match publisher {
                Some(p) => core_::third_party_entry_seed(&pid, &p),
                None => core_::entry_seed(&pid),
            };
            println!("{}", hex(&seed));
            println!("  base58 {}", to_base58(&seed));
            println!(
                "  {}",
                if publisher.is_some() {
                    "third-party entry — one per publisher, so these never collide"
                } else {
                    "canonical entry — exactly one, derivable from the program id alone"
                }
            );
            ExitCode::SUCCESS
        }
        "global-address" => {
            let seed = core_::global_seed();
            println!("{}", hex(&seed));
            println!("  base58 {}", to_base58(&seed));
            ExitCode::SUCCESS
        }
        "verify" => verify(&args),
        _ => usage(),
    }
}

fn verify(args: &[String]) -> ExitCode {
    if args.iter().any(|a| a == "--explain") {
        println!(
            "\
A hash match proves the bytecode you have is the bytecode deployed. It does not
prove where that bytecode came from.

Solana's verified builds were subverted by a repository whose build.rs
downloaded the already-deployed binary at compile time and emitted it. The
rebuild matched perfectly and the source proved nothing.

So this registry records whether a rebuild ran with the network disabled, and a
claim built with network access cannot reach Verified — it stops at
Unreproducible. That is a different state, not a softer word for the same one.

Verification is bound to one artifact, which here costs no machinery: an entry
is keyed by its ImageID, so an upgraded program is a different key and therefore
a different entry, starting Unclaimed. There is no way to inherit a badge earned
by code that is no longer running, and no Stale state to display, because the
bytecode under an entry cannot be replaced.

Two numbers, kept apart on purpose:
  ProgramId         the RISC0 ImageID -- what a program is called by
  deployment digest SHA256(len_le || bytes) -- what the deploy tx hashes to
The second is not the first. This tool checks the second and says so."
        );
        return ExitCode::SUCCESS;
    }
    let Some(path) = args.get(1) else {
        return usage();
    };
    let Some(expected) = args
        .iter()
        .position(|a| a == "--expect")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| from_hex(s))
    else {
        eprintln!("--expect <PROGRAM_ID_HEX> is required");
        return ExitCode::FAILURE;
    };
    let Ok(bytes) = std::fs::read(path) else {
        eprintln!("cannot read {path}");
        return ExitCode::FAILURE;
    };
    let got = core_::deployment_digest(&bytes);
    if got == expected {
        println!("match   {}", hex(&got));
        println!("  the bytecode in {path} is what is deployed.");
        println!("  this is necessary, not sufficient — run `verify --explain`.");
        ExitCode::SUCCESS
    } else {
        println!("MISMATCH");
        println!("  built    {}", hex(&got));
        println!("  expected {}", hex(&expected));
        ExitCode::FAILURE
    }
}

fn hex(b: &[u8; 32]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
