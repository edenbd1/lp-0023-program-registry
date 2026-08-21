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
        "explain-entry" => explain_entry(&args),
        "idl" => idl_cmd(&args),
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

/// Decode an entry and report what its state supports, rather than whether it
/// carries a badge.
fn explain_entry(args: &[String]) -> ExitCode {
    let Some(raw) = args.get(1) else {
        eprintln!(
            "explain-entry <HEX> — the `entry` field from `spel inspect --type EntryAccount`"
        );
        return ExitCode::FAILURE;
    };
    let bytes = match parse_hex_blob(raw.trim()) {
        Some(b) => b,
        None => {
            eprintln!("not hex");
            return ExitCode::FAILURE;
        }
    };
    let e: core_::Entry = match borsh::from_slice(&bytes) {
        Ok(e) => e,
        Err(err) => {
            eprintln!("does not decode as an Entry: {err}");
            return ExitCode::FAILURE;
        }
    };

    println!("{} {}", e.name, e.version);
    println!("  program id   {}", hex(&e.program_id));
    println!("  author       {}", hex(&e.author));
    println!(
        "  publisher    {}",
        match e.publisher {
            core_::Publisher::Canonical => "canonical",
            core_::Publisher::ThirdParty => "third party",
        }
    );
    if e.frozen {
        println!("  frozen       yes — metadata is immutable");
    }

    match &e.verification.state {
        core_::VerificationState::Unclaimed => {
            println!("  verification unclaimed");
            println!("               Nobody has said where the source is. This is the honest");
            println!("               default, not a failure.");
        }
        core_::VerificationState::Claimed => {
            println!("  verification claimed, NOT verified");
            println!("               A source is asserted and nobody has rebuilt it. Typing a");
            println!("               URL is not evidence.");
        }
        core_::VerificationState::Mismatched => {
            println!("  verification MISMATCHED — a source was published and does not rebuild");
            println!("               This is a stronger negative signal than no claim at all:");
            println!("               somebody said where the source is, and it does not produce");
            println!("               this bytecode. Do not read it as merely unverified.");
            if let Some(a) = e.verification.attester {
                println!("               found by {}", hex(&a));
            }
        }
        core_::VerificationState::Unreproducible => {
            println!("  verification unreproducible");
            println!("               A rebuild was recorded but the claimed build had network");
            println!("               access, so a match would prove nothing about the source.");
            // Who tried matters most in exactly this case: it separates a claim
            // nobody has looked at from one that was examined and did not hold.
            if let Some(a) = e.verification.attester {
                println!("               attempted by {}", hex(&a));
            }
        }
        core_::VerificationState::Verified => {
            let attester = e.verification.attester;
            if e.verification.is_self_attested(&e.author) {
                println!("  verification verified — BY ITS OWN AUTHOR");
                println!("               The only party vouching for this build is the party");
                println!("               that shipped it. Useful, and not an independent check.");
            } else {
                println!("  verification verified");
                match attester {
                    Some(a) => println!("               attested by {}", hex(&a)),
                    // Unreachable through this crate's own API, which always
                    // writes the attester alongside the state. Printed rather
                    // than asserted because the data may come from a chain
                    // account written by some other implementation.
                    None => println!("               attested by nobody — treat as unverified"),
                }
            }
        }
    }
    println!(
        "  registered   block {} ({} ms)",
        e.registered_at.block, e.registered_at.millis
    );
    // Printed next to the verdict so staleness is the reader's to judge rather
    // than something this tool decides on their behalf.
    match e.verification.last_checked {
        Some(s) => println!("  last checked block {} ({} ms)", s.block, s.millis),
        None => println!("  last checked never"),
    }
    if let Some(c) = &e.verification.claim {
        println!("  repository   {}", c.repository);
        println!("  toolchain    {}", c.build.toolchain);
        println!("  offline      {}", c.build.network_disabled);
    }
    ExitCode::SUCCESS
}

fn parse_hex_blob(s: &str) -> Option<Vec<u8>> {
    let s = s.trim_start_matches("0x");
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

/// Check a content identifier, and resolve it when told where to look.
fn idl_cmd(args: &[String]) -> ExitCode {
    let Some(raw) = args.get(1) else {
        eprintln!("idl <CONTENT_ID> [--from <DIR-OR-URL-PREFIX>]");
        return ExitCode::FAILURE;
    };
    let Some(id) = core_::decode_content_id(raw.trim()) else {
        eprintln!(
            "not a multibase base58btc content identifier.\n\
             A Logos Storage identifier starts with `z` and decodes to {} bytes.",
            core_::CONTENT_ID_LEN
        );
        return ExitCode::FAILURE;
    };
    println!("content id   {}", raw.trim());
    println!("  bytes      {}", hex_n(&id));
    println!("  cid version {}", id[0]);
    println!("  codec      {}", hex_n(&id[1..4]));
    println!("  digest     {}", hex_n(&id[6..]));

    let Some(from) = args
        .iter()
        .position(|a| a == "--from")
        .and_then(|i| args.get(i + 1))
    else {
        println!("\n  no --from given, so nothing was fetched. The identifier is well formed;");
        println!("  that is a different claim from the IDL being available.");
        return ExitCode::SUCCESS;
    };

    // A directory is the offline backend and an http(s) prefix is the online
    // one. Both are just "where the bytes are"; the check that matters is the
    // same either way.
    let body = if from.starts_with("http://") || from.starts_with("https://") {
        eprintln!("\n  fetching over http is not wired in this build; pass a directory");
        return ExitCode::FAILURE;
    } else {
        match std::fs::read(std::path::Path::new(from).join(raw.trim())) {
            Ok(b) => b,
            Err(e) => {
                // Unresolvable is a normal outcome, not a crash: an entry whose
                // IDL cannot be fetched still has good on-chain metadata.
                println!("\n  IDL unavailable: {e}");
                println!("  The entry's on-chain metadata is unaffected.");
                return ExitCode::SUCCESS;
            }
        }
    };

    match serde_json_parses(&body) {
        true => {
            println!("\n  resolved, {} bytes, parses as JSON", body.len());
            ExitCode::SUCCESS
        }
        false => {
            println!(
                "\n  resolved, {} bytes, but it does NOT parse as JSON",
                body.len()
            );
            println!("  Treat the IDL as unavailable rather than trusting a partial parse.");
            ExitCode::SUCCESS
        }
    }
}

/// Minimal JSON well-formedness check.
///
/// Deliberately not a parser into a typed IDL: the criterion is that a client
/// can tell a usable IDL from an unusable one, and pulling a JSON crate into a
/// tool whose whole argument is "few dependencies you have to trust" would be a
/// poor trade for that.
fn serde_json_parses(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let text = text.trim();
    if !(text.starts_with('{') || text.starts_with('[')) {
        return false;
    }
    let (mut depth, mut in_string, mut escaped) = (0i32, false, false);
    for c in text.chars() {
        if in_string {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' | '[' => depth += 1,
            '}' | ']' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0 && !in_string
}

fn hex_n(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
