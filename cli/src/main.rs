// SPDX-License-Identifier: MIT OR Apache-2.0
//! `lez-registry`: the Provenance registry CLI, for LEZ v0.3.
//!
//! Register, update, look up, search and verify programs. Every address it
//! prints or writes to is derived by `lez-registry-core`, the crate the on-chain
//! program links, and every transaction is built and signed by
//! `lez-registry-sdk`. Keys are 32-byte secrets in hex files, the form
//! `wallet account import public --private-key` takes, so the same account can
//! be driven from the LEZ wallet and from here.
//!
//! On v0.3 a program call that fails is charged and reverted but still lands in
//! a block, so after every write this tool reads the chain back and reports
//! what actually changed. "included" alone is never printed as success.

mod local;

use lez_registry_core as core_;
use lez_registry_sdk::{
    self as sdk, base58, hex, parse_account, BuiltTransaction, Client, EntryKey, Keypair,
};
use std::process::ExitCode;

const DEFAULT_ENDPOINT: &str = "https://testnet.lez.logos.co";
/// Gas (executed cycles) a registry call may use. A register runs the guest
/// five times (one plan, four applies); override with PROVENANCE_GAS_LIMIT.
const DEFAULT_GAS: u64 = 4_000_000;

fn usage() -> ExitCode {
    eprintln!(
        "\
lez-registry: Provenance, the program registry for LEZ v0.3

Connection (every chain command):
  --rpc <URL>           sequencer RPC (env LEZ_RPC, default {DEFAULT_ENDPOINT})
  --registry <ACCOUNT>  registry program account (env PROVENANCE_REGISTRY)
Keys are files holding a 64-hex-character secret:
  --key <FILE>          the signing author or attester
  --payer <FILE>        the fee payer, an account holding LGO
Accounts are base58 (as the wallet prints them, `Public/` optional) or hex.

Read
  lookup <PROGRAM> [--publisher <ACCOUNT>]   resolve an entry: one hash, one read
  list                                       every entry, in registration order
  search <TEXT>                              match name, version, description, tags, author
  program-header <PROGRAM>                   the header the loader holds: ImageID, frozen flag
  entry-address <PROGRAM> [--publisher <ACCOUNT>]
  global-address

Write
  init --payer <FILE>
  register <PROGRAM> --name <N> --version <V> [--description <D>] [--tag <T>]...
           [--idl <CID>] [--repo <URL> --commit <HEX40> --source-digest <HEX>
            --image-digest <HEX> --toolchain <T> [--offline]]
           --key <FILE> --payer <FILE> [--third-party] [--submit-anyway]
        Without --third-party this is the deployer's entry, and --key must be the
        program's own header key: the chain refuses anything else, and so does
        this tool unless --submit-anyway asks to watch the chain do it.
  update <PROGRAM> [--publisher <ACCOUNT>] --version <V> [--description <D>] [--idl <CID>]
           --key <FILE> --payer <FILE>
  claim-source <PROGRAM> [--publisher <ACCOUNT>] --repo <URL> --commit <HEX40>
           --source-digest <HEX> --image-digest <HEX> --toolchain <T> [--offline]
           --key <FILE> --payer <FILE>
  record-rebuild <PROGRAM> [--publisher <ACCOUNT>] (--file <BIN> | --image <HEX>)
           --key <FILE> --payer <FILE>
  sync-header <PROGRAM> [--publisher <ACCOUNT>] --payer <FILE>
  freeze <PROGRAM> [--publisher <ACCOUNT>] --key <FILE> --payer <FILE>

Verify locally
  image-id <BIN>                             the RISC0 ImageID of a built guest
  verify <BIN> --program <PROGRAM> [--mark]  does your rebuild run at that program id?
  local-verified                             your own marks, never on chain
  idl <CID> [--from <DIR>]                   check an IDL pointer, and resolve it offline

Keys and accounts
  keygen [--out <FILE>]                      a fresh public-account key
  account-id --key <FILE>                    the account a key controls
  balance <ACCOUNT>                          native LGO balance

Environment: PROVENANCE_GAS_LIMIT overrides the gas limit (default {DEFAULT_GAS})."
    );
    ExitCode::from(2)
}

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1))
}

fn flags<'a>(args: &'a [String], name: &str) -> Vec<&'a String> {
    args.iter()
        .enumerate()
        .filter(|(_, a)| *a == name)
        .filter_map(|(i, _)| args.get(i + 1))
        .collect()
}

fn has(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

fn fail(msg: impl std::fmt::Display) -> ExitCode {
    eprintln!("error: {msg}");
    ExitCode::FAILURE
}

fn client(args: &[String]) -> Result<Client, String> {
    let rpc = flag(args, "--rpc")
        .cloned()
        .or_else(|| std::env::var("LEZ_RPC").ok())
        .unwrap_or_else(|| DEFAULT_ENDPOINT.into());
    let reg = flag(args, "--registry")
        .cloned()
        .or_else(|| std::env::var("PROVENANCE_REGISTRY").ok())
        .ok_or("--registry <ACCOUNT> (or PROVENANCE_REGISTRY) is required")?;
    let reg = parse_account(&reg).ok_or("--registry is not a 32-byte account id")?;
    Ok(Client::new(rpc, reg))
}

/// A client for commands that read the chain but not the registry
/// (`balance`, `program-header`, `verify`): the registry may be unset.
fn chain_client(args: &[String]) -> Result<Client, String> {
    client(args).or_else(|_| {
        let rpc = flag(args, "--rpc")
            .cloned()
            .or_else(|| std::env::var("LEZ_RPC").ok())
            .unwrap_or_else(|| DEFAULT_ENDPOINT.into());
        Ok(Client::new(rpc, [0u8; 32]))
    })
}

fn key_from(args: &[String], name: &str) -> Result<Keypair, String> {
    let path = flag(args, name).ok_or(format!("{name} <FILE> is required"))?;
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?;
    Keypair::from_hex(text.trim()).ok_or(format!("{path} does not hold a 64-hex-character secret"))
}

fn program_arg(args: &[String]) -> Result<[u8; 32], String> {
    args.get(1)
        .and_then(|s| parse_account(s))
        .ok_or_else(|| "the first argument must be a program account id".into())
}

/// The entry key a command names: the deployer's entry unless `--publisher`.
fn key_arg(args: &[String]) -> Result<EntryKey, String> {
    let program = program_arg(args)?;
    let publisher = match flag(args, "--publisher") {
        Some(p) => Some(parse_account(p).ok_or("--publisher is not an account id")?),
        None => None,
    };
    Ok(EntryKey { program, publisher })
}

fn hex_n<const N: usize>(s: &str, what: &str) -> Result<[u8; N], String> {
    let s = s.trim().strip_prefix("0x").unwrap_or(s.trim());
    if s.len() != N * 2 {
        return Err(format!("{what} must be {} hex characters", N * 2));
    }
    let mut out = [0u8; N];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).map_err(|_| format!("{what} is not hex"))?;
    }
    Ok(out)
}

/// A source claim from flags, if `--repo` was given.
fn claim_from(args: &[String]) -> Result<Option<core_::SourceClaim>, String> {
    let Some(repo) = flag(args, "--repo") else {
        return Ok(None);
    };
    let need = |n: &str| flag(args, n).ok_or(format!("{n} is required with --repo"));
    Ok(Some(core_::SourceClaim {
        repository: repo.clone(),
        commit: hex_n::<20>(need("--commit")?, "--commit")?,
        source_digest: hex_n::<32>(need("--source-digest")?, "--source-digest")?,
        build: core_::BuildEnvironment {
            image_digest: hex_n::<32>(need("--image-digest")?, "--image-digest")?,
            toolchain: need("--toolchain")?.clone(),
            network_disabled: has(args, "--offline"),
        },
    }))
}

/// The ImageID of a built guest: what the loader records in a header built
/// from it, with the protocol's default kernel.
fn image_id_of(path: &str) -> Result<[u8; 32], String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read {path}: {e}"))?;
    let digest = risc0_binfmt::compute_image_id(&bytes)
        .map_err(|e| format!("{path} is not a RISC0 program binary: {e}"))?;
    let mut out = [0u8; 32];
    out.copy_from_slice(digest.as_bytes());
    Ok(out)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first().map(String::as_str) else {
        return usage();
    };
    let r = match cmd {
        "keygen" => keygen(&args),
        "image-id" => image_id_cmd(&args),
        "entry-address" => entry_address_cmd(&args),
        "global-address" => client(&args).map(|c| {
            println!("{}", base58(&sdk::global_address(c.registry())));
            ExitCode::SUCCESS
        }),
        "program-header" => program_header_cmd(&args),
        "balance" => chain_client(&args).and_then(|c| {
            let id = program_arg(&args)?;
            println!("{}", c.balance(&id).map_err(|e| e.to_string())?);
            Ok(ExitCode::SUCCESS)
        }),
        "account-id" => {
            let k = key_from(&args, "--key");
            k.map(|k| {
                println!("{}", base58(&k.account_id()));
                ExitCode::SUCCESS
            })
        }
        "lookup" => lookup_cmd(&args),
        "list" => search_cmd(&args, ""),
        "search" => search_cmd(&args, args.get(1).map_or("", String::as_str)),
        "init" => init_cmd(&args),
        "register" => register_cmd(&args),
        "update" => update_cmd(&args),
        "claim-source" => claim_source_cmd(&args),
        "record-rebuild" => record_rebuild_cmd(&args),
        "sync-header" => sync_header_cmd(&args),
        "freeze" => freeze_cmd(&args),
        "verify" => verify_cmd(&args),
        "local-verified" => Ok(local_verified_cmd()),
        "idl" => Ok(idl_cmd(&args)),
        _ => return usage(),
    };
    r.unwrap_or_else(fail)
}

fn keygen(args: &[String]) -> Result<ExitCode, String> {
    let k = Keypair::generate();
    match flag(args, "--out") {
        Some(path) => {
            std::fs::write(path, format!("{}\n", k.secret_hex()))
                .map_err(|e| format!("cannot write {path}: {e}"))?;
            println!("{}", base58(&k.account_id()));
            println!("  secret written to {path}");
        }
        None => {
            println!("{}", base58(&k.account_id()));
            println!("  secret {}", k.secret_hex());
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn image_id_cmd(args: &[String]) -> Result<ExitCode, String> {
    let path = args.get(1).ok_or("image-id <BIN>")?;
    println!("{}", hex(&image_id_of(path)?));
    // How many segment accounts `wallet program-loader deploy` needs: the
    // loader stores the user ELF (not the kernel) in 96 KiB chunks.
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    if let Ok(bin) = risc0_binfmt::ProgramBinary::decode(&bytes) {
        const SEGMENT: usize = 96 * 1024;
        eprintln!(
            "  user ELF {} bytes: {} loader segment(s)",
            bin.user_elf.len(),
            bin.user_elf.len().div_ceil(SEGMENT)
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn entry_address_cmd(args: &[String]) -> Result<ExitCode, String> {
    let c = client(args)?;
    let key = key_arg(args)?;
    println!("{}", base58(&sdk::key_address(c.registry(), &key)));
    println!(
        "  {}",
        if key.publisher.is_some() {
            "third-party entry: one per publisher, so these never collide"
        } else {
            "deployer entry: exactly one, derivable from the program id alone"
        }
    );
    Ok(ExitCode::SUCCESS)
}

fn program_header_cmd(args: &[String]) -> Result<ExitCode, String> {
    let c = chain_client(args)?;
    let program = program_arg(args)?;
    match c.program_header(&program).map_err(|e| e.to_string())? {
        Some(h) => {
            println!("image id  {}", hex(&h.image_id));
            println!(
                "header    {}",
                if h.immutable {
                    "immutable: can never be repointed, so a match cannot go stale"
                } else {
                    "mutable: the deployer may repoint it; re-check after any update"
                }
            );
            Ok(ExitCode::SUCCESS)
        }
        None => {
            println!("no program header at {}", base58(&program));
            Ok(ExitCode::FAILURE)
        }
    }
}

fn state_line(e: &core_::Entry) -> String {
    use core_::VerificationState as S;
    match e.verification.state {
        S::Verified if e.verification.is_self_attested(&e.author) => "verified (by its own author)".into(),
        S::Verified => "verified".into(),
        S::Mismatched => "MISMATCHED".into(),
        S::Unreproducible => "unreproducible".into(),
        S::Claimed => "claimed".into(),
        S::Unclaimed => "unclaimed".into(),
        S::Stale => "stale (header repointed since the verdict)".into(),
    }
}

fn print_entry(e: &core_::Entry) {
    println!("{} {}", e.name, e.version);
    println!("  program      {}", base58(&e.program_id));
    println!(
        "  publisher    {}",
        match e.publisher {
            core_::Publisher::Canonical => "deployer (signed by the program's header account)",
            core_::Publisher::ThirdParty => "third party",
        }
    );
    println!("  author       {}", base58(&e.author));
    println!(
        "  image id     {}{}",
        hex(&e.image_id),
        if e.header_immutable { "  (header immutable)" } else { "  (header mutable)" }
    );
    if !e.description.is_empty() {
        println!("  description  {}", e.description);
    }
    if !e.tags.is_empty() {
        println!("  tags         {}", e.tags.join(", "));
    }
    match &e.idl {
        Some(id) => println!("  idl          {}", encode_content_id(id)),
        None => println!("  idl          none"),
    }
    println!("  verification {}", state_line(e));
    if let Some(a) = e.verification.attester {
        println!("  attester     {}", base58(&a));
    }
    if let Some(cl) = &e.verification.claim {
        println!("  source       {} @ {}", cl.repository, hex(&cl.commit));
        println!(
            "  build        {} (network {})",
            cl.build.toolchain,
            if cl.build.network_disabled { "disabled" } else { "ENABLED" }
        );
    }
    println!(
        "  registered   block {} ({} ms)",
        e.registered_at.block, e.registered_at.millis
    );
    if let Some(s) = e.verification.last_checked {
        println!("  last checked block {} ({} ms)", s.block, s.millis);
    }
    if e.frozen {
        println!("  frozen       yes: metadata can never change again");
    }
}

/// Multibase base58btc, the form a Logos Storage node accepts.
fn encode_content_id(id: &core_::ContentId) -> String {
    const ALPHABET: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    let mut digits: Vec<u8> = Vec::new();
    for &byte in id {
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
    let mut s = String::from("z");
    s.extend(std::iter::repeat_n('1', id.iter().take_while(|b| **b == 0).count()));
    s.extend(digits.iter().rev().map(|d| ALPHABET[*d as usize] as char));
    s
}

fn lookup_cmd(args: &[String]) -> Result<ExitCode, String> {
    let c = client(args)?;
    let key = key_arg(args)?;
    let addr = sdk::key_address(c.registry(), &key);
    println!("derived  {}", base58(&addr));
    match c.entry_at(&addr).map_err(|e| e.to_string())? {
        Some(e) => {
            print_entry(&e);
            if let Some(m) = local::mark_for(&hex(&key.program)) {
                println!(
                    "  locally verified by you (unix {}): your own rebuild, not a registry claim",
                    m.marked_at
                );
            }
        }
        // Not an error: a derived address always exists as an address.
        None => println!("nothing is registered at that address"),
    }
    Ok(ExitCode::SUCCESS)
}

fn search_cmd(args: &[String], needle: &str) -> Result<ExitCode, String> {
    let c = client(args)?;
    let needle = if needle.starts_with("--") { "" } else { needle }.to_lowercase();
    let entries = c.walk().map_err(|e| e.to_string())?;
    let hits: Vec<_> = entries
        .iter()
        .filter(|e| {
            needle.is_empty()
                || e.name.to_lowercase().contains(&needle)
                || e.version.to_lowercase().contains(&needle)
                || e.description.to_lowercase().contains(&needle)
                || e.tags.iter().any(|t| t.to_lowercase().contains(&needle))
                || base58(&e.author).to_lowercase().contains(&needle)
        })
        .collect();
    println!("{} of {} entries", hits.len(), entries.len());
    for e in hits {
        println!(
            "  {:<28} {:<9} {:<10} {:<44} {}",
            e.name.chars().take(28).collect::<String>(),
            e.version.chars().take(9).collect::<String>(),
            match e.publisher {
                core_::Publisher::Canonical => "deployer",
                core_::Publisher::ThirdParty => "3rd-party",
            },
            base58(&e.program_id),
            state_line(e)
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// Sign, submit, wait for inclusion. Returns the block.
fn submit(c: &Client, tx: &BuiltTransaction, keys: &[&Keypair], payer: &Keypair) -> Result<u64, String> {
    println!(
        "submitting {} ({} inputs, {} signer(s) + fee payer {})",
        tx.instruction,
        tx.accounts.len(),
        tx.signer_ids().len(),
        base58(&payer.account_id())
    );
    let gas = std::env::var("PROVENANCE_GAS_LIMIT")
        .ok()
        .and_then(|g| g.parse().ok())
        .unwrap_or(DEFAULT_GAS);
    let before = c.balance(&payer.account_id()).ok();
    let s = c.submit_with_gas(tx, keys, Some(payer), gas).map_err(|e| e.to_string())?;
    let after = c.balance(&payer.account_id()).ok();
    let fee = match (before, after) {
        (Some(b), Some(a)) if b >= a => format!("  fee {}", b - a),
        _ => String::new(),
    };
    println!("  tx {}  included in block {}{fee}", s.hash, s.block);
    Ok(s.block)
}

fn reverted(what: &str) -> Result<ExitCode, String> {
    println!("REVERTED: {what}");
    println!("  the transaction was included and charged, and the chain kept nothing of it.");
    Ok(ExitCode::FAILURE)
}

fn init_cmd(args: &[String]) -> Result<ExitCode, String> {
    let c = client(args)?;
    let payer = key_from(args, "--payer")?;
    if let Some(g) = c.global().map_err(|e| e.to_string())? {
        println!("already initialized: {} entries", g.total_entries);
        return Ok(ExitCode::SUCCESS);
    }
    submit(&c, &sdk::build_initialize(c.registry()), &[], &payer)?;
    match c.global().map_err(|e| e.to_string())? {
        Some(g) => {
            println!("initialized: global {}  entries {}", base58(&sdk::global_address(c.registry())), g.total_entries);
            Ok(ExitCode::SUCCESS)
        }
        None => reverted("initialize"),
    }
}

fn register_cmd(args: &[String]) -> Result<ExitCode, String> {
    let c = client(args)?;
    let program = program_arg(args)?;
    let key = key_from(args, "--key")?;
    let payer = key_from(args, "--payer")?;
    let author = key.account_id();
    let deployer = !has(args, "--third-party");
    let name = flag(args, "--name").ok_or("--name is required")?;
    let version = flag(args, "--version").ok_or("--version is required")?;
    let description = flag(args, "--description").cloned().unwrap_or_default();
    let tags: Vec<String> = flags(args, "--tag").into_iter().cloned().collect();
    let idl = flag(args, "--idl").cloned().unwrap_or_default();
    let source = claim_from(args)?;

    if deployer && author != program {
        println!(
            "refused: a deployer entry must be signed by the program's own header account.\n  \
             --key controls {}, the program is {}.",
            base58(&author),
            base58(&program)
        );
        if !has(args, "--submit-anyway") {
            println!("  use --third-party to publish your own entry about it.");
            return Ok(ExitCode::FAILURE);
        }
        println!("  --submit-anyway: sending it so the chain's refusal is on record.");
    }

    let header = c
        .program_header(&program)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("{} holds no program header: nothing is deployed there", base58(&program)))?;
    let seq_index = c.global().map_err(|e| e.to_string())?.ok_or("the registry is not initialized")?.total_entries;
    let at = c.clock().map_err(|e| e.to_string())?;
    let req = sdk::RegisterRequest {
        program: &program,
        author: &author,
        deployer,
        header: header.view(),
        seq_index,
        at,
        name,
        version,
        description: &description,
        tags: &tags,
        idl: &idl,
        source,
    };
    let tx = sdk::build_register(c.registry(), &req).map_err(|e| e.to_string())?;
    let entry = tx.entry_address().expect("register touches an entry");
    println!(
        "{} entry for {} at {}",
        if deployer { "deployer" } else { "third-party" },
        base58(&program),
        base58(&entry)
    );
    println!("  header image {} ({})", hex(&header.image_id), if header.immutable { "immutable" } else { "mutable" });
    println!("  directory slot {seq_index}, stamped block {} ({} ms)", at.block, at.millis);
    submit(&c, &tx, &[&key], &payer)?;
    match c.entry_at(&entry).map_err(|e| e.to_string())? {
        Some(e) if e.author == author && e.program_id == program => {
            println!("registered:");
            print_entry(&e);
            Ok(ExitCode::SUCCESS)
        }
        Some(_) => reverted("an entry already existed at that address; it was not overwritten"),
        None => reverted("no entry was written"),
    }
}

fn update_cmd(args: &[String]) -> Result<ExitCode, String> {
    let c = client(args)?;
    let k = key_arg(args)?;
    let key = key_from(args, "--key")?;
    let payer = key_from(args, "--payer")?;
    let version = flag(args, "--version").ok_or("--version is required")?;
    let description = flag(args, "--description").cloned().unwrap_or_default();
    let idl = flag(args, "--idl").cloned().unwrap_or_default();
    let before = c.entry_by_key(&k).map_err(|e| e.to_string())?.ok_or("nothing is registered there")?;
    if before.author != key.account_id() {
        println!("note: the recorded author is {}; the chain will refuse this signer", base58(&before.author));
    }
    let tx = sdk::build_update(c.registry(), &k, &key.account_id(), version, &description, &idl)
        .map_err(|e| e.to_string())?;
    submit(&c, &tx, &[&key], &payer)?;
    let after = c.entry_by_key(&k).map_err(|e| e.to_string())?.ok_or("the entry vanished")?;
    if after == before {
        return reverted("the entry is unchanged");
    }
    println!("updated:");
    print_entry(&after);
    Ok(ExitCode::SUCCESS)
}

fn claim_source_cmd(args: &[String]) -> Result<ExitCode, String> {
    let c = client(args)?;
    let k = key_arg(args)?;
    let key = key_from(args, "--key")?;
    let payer = key_from(args, "--payer")?;
    let claim = claim_from(args)?.ok_or("--repo and the build flags are required")?;
    let tx = sdk::build_claim_source(c.registry(), &k, &key.account_id(), claim.clone())
        .map_err(|e| e.to_string())?;
    submit(&c, &tx, &[&key], &payer)?;
    let after = c.entry_by_key(&k).map_err(|e| e.to_string())?.ok_or("nothing is registered there")?;
    if after.verification.claim.as_ref() != Some(&claim) {
        return reverted("the claim was not recorded");
    }
    println!("claimed:");
    print_entry(&after);
    Ok(ExitCode::SUCCESS)
}

fn record_rebuild_cmd(args: &[String]) -> Result<ExitCode, String> {
    let c = client(args)?;
    let k = key_arg(args)?;
    let key = key_from(args, "--key")?;
    let payer = key_from(args, "--payer")?;
    let rebuilt = match (flag(args, "--file"), flag(args, "--image")) {
        (Some(f), _) => image_id_of(f)?,
        (None, Some(h)) => hex_n::<32>(h, "--image")?,
        (None, None) => return Err("--file <BIN> or --image <HEX> is required".into()),
    };
    let header = c
        .program_header(&k.program)
        .map_err(|e| e.to_string())?
        .ok_or("no program header at that account")?;
    let before = c.entry_by_key(&k).map_err(|e| e.to_string())?.ok_or("nothing is registered there")?;
    println!("rebuilt image  {}", hex(&rebuilt));
    println!("header image   {}{}", hex(&header.image_id), if header.matches(&rebuilt) { "  (match)" } else { "  (DIFFERENT)" });
    let at = c.clock().map_err(|e| e.to_string())?;
    let tx = sdk::build_record_rebuild(c.registry(), &k, header.view(), rebuilt, at, &key.account_id());
    submit(&c, &tx, &[&key], &payer)?;
    let after = c.entry_by_key(&k).map_err(|e| e.to_string())?.ok_or("the entry vanished")?;
    if after.verification == before.verification && after.image_id == before.image_id {
        return reverted("no verdict was recorded");
    }
    println!("recorded:");
    print_entry(&after);
    Ok(ExitCode::SUCCESS)
}

fn sync_header_cmd(args: &[String]) -> Result<ExitCode, String> {
    let c = client(args)?;
    let k = key_arg(args)?;
    let payer = key_from(args, "--payer")?;
    let header = c
        .program_header(&k.program)
        .map_err(|e| e.to_string())?
        .ok_or("no program header at that account")?;
    let before = c.entry_by_key(&k).map_err(|e| e.to_string())?.ok_or("nothing is registered there")?;
    if before.image_id == header.image_id && before.header_immutable == header.immutable {
        println!("already in sync: {}", hex(&header.image_id));
        return Ok(ExitCode::SUCCESS);
    }
    submit(&c, &sdk::build_sync_header(c.registry(), &k, header.view()), &[], &payer)?;
    let after = c.entry_by_key(&k).map_err(|e| e.to_string())?.ok_or("the entry vanished")?;
    if after.image_id != header.image_id {
        return reverted("the entry still records the old header");
    }
    println!("synced:");
    print_entry(&after);
    Ok(ExitCode::SUCCESS)
}

fn freeze_cmd(args: &[String]) -> Result<ExitCode, String> {
    let c = client(args)?;
    let k = key_arg(args)?;
    let key = key_from(args, "--key")?;
    let payer = key_from(args, "--payer")?;
    submit(&c, &sdk::build_freeze(c.registry(), &k, &key.account_id()), &[&key], &payer)?;
    let after = c.entry_by_key(&k).map_err(|e| e.to_string())?.ok_or("nothing is registered there")?;
    if !after.frozen {
        return reverted("the entry is not frozen");
    }
    println!("frozen: {} {} can never change again", after.name, after.version);
    Ok(ExitCode::SUCCESS)
}

/// Local verification: rebuild the recorded source yourself, then ask whether
/// the ImageID you got is the one the program's header records on chain.
fn verify_cmd(args: &[String]) -> Result<ExitCode, String> {
    let path = args.get(1).ok_or("verify <BIN> --program <PROGRAM>")?;
    let program = flag(args, "--program")
        .and_then(|s| parse_account(s))
        .ok_or("--program <PROGRAM> is required")?;
    let c = chain_client(args)?;
    let rebuilt = image_id_of(path)?;
    let header = c
        .program_header(&program)
        .map_err(|e| e.to_string())?
        .ok_or("no program header at that account")?;
    println!("rebuilt  {}", hex(&rebuilt));
    println!("on chain {}  ({})", hex(&header.image_id), if header.immutable { "immutable header" } else { "mutable header" });
    if !header.matches(&rebuilt) {
        println!("MISMATCH: your rebuild is not the program running at {}", base58(&program));
        return Ok(ExitCode::FAILURE);
    }
    println!("match: your rebuild is the program running at {}", base58(&program));
    println!("  necessary, not sufficient: a build that fetched the answer over the network");
    println!("  would match too. Rebuild with the network disabled (scripts/build-sealed.sh).");
    if has(args, "--mark") {
        use sha2::Digest as _;
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let mark = local::Mark {
            program_id: hex(&program),
            digest: hex(&rebuilt),
            file_sha256: hex(&sha2::Sha256::digest(&bytes)),
            marked_at: local::now(),
        };
        let at = local::add(&mark).map_err(|e| format!("the match stands, but the mark was not written: {e}"))?;
        println!("  marked locally verified, for you only, in {}", at.display());
    }
    Ok(ExitCode::SUCCESS)
}

fn local_verified_cmd() -> ExitCode {
    let marks = local::load();
    println!("{}", local::store_path().display());
    if marks.is_empty() {
        println!("  nothing marked yet: `verify <BIN> --program <PROGRAM> --mark` after a rebuild");
    }
    for m in marks {
        println!("  {}  image {}  unix {}", m.program_id, m.digest, m.marked_at);
    }
    ExitCode::SUCCESS
}

/// Check a content identifier, and resolve it from a directory when asked.
fn idl_cmd(args: &[String]) -> ExitCode {
    let Some(raw) = args.get(1) else {
        eprintln!("idl <CONTENT_ID> [--from <DIR>]");
        return ExitCode::FAILURE;
    };
    let Some(id) = core_::decode_content_id(raw.trim()) else {
        eprintln!(
            "not a multibase base58btc content identifier: a Logos Storage identifier starts with `z` and decodes to {} bytes",
            core_::CONTENT_ID_LEN
        );
        return ExitCode::FAILURE;
    };
    println!("content id   {}", raw.trim());
    println!("  bytes      {}", hex(&id));
    let Some(from) = flag(args, "--from") else {
        println!("  well formed; nothing fetched (pass --from <DIR> to resolve offline)");
        return ExitCode::SUCCESS;
    };
    match std::fs::read(std::path::Path::new(from).join(raw.trim())) {
        Ok(body) => match serde_json::from_slice::<serde_json::Value>(&body) {
            Ok(_) => println!("  resolved, {} bytes, parses as JSON", body.len()),
            Err(_) => println!("  resolved, {} bytes, but it does NOT parse: treat the IDL as unavailable", body.len()),
        },
        // Unresolvable is a normal outcome: the on-chain metadata still stands.
        Err(e) => println!("  IDL unavailable ({e}); the entry's on-chain metadata is unaffected"),
    }
    ExitCode::SUCCESS
}
