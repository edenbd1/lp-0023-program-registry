// SPDX-License-Identifier: MIT OR Apache-2.0
//! Execution gas of every registry instruction on LEZ v0.3.
//!
//! A v0.3 public call is one `plan` guest run, then one `apply` guest run per
//! effect it planned, and the transaction is charged the sum of their RISC0
//! cycles as execution gas. This runs the real guest binary through exactly
//! that sequence, under the executor the sequencer uses without proving, with
//! instructions and shard selectors built by the SDK and shard contents carried
//! from one instruction to the next, and prints the per-call and total cycles.
//!
//!   cargo run --release -- <lez_registry.bin>

use lee_core::{
    account::{AccountId, ProgramShardSelector},
    program::{
        AccountMeta, ApplyInput, CallKind, GuestOutput, PlanInput, ProgramHeader,
        PROGRAM_LOADER_ACCOUNT_ID,
    },
    to_borsh_frame, to_frame,
};
use lez_registry_core::{self as core_, instruction::EntryKey};
use lez_registry_sdk::{self as sdk, BuiltTransaction, HeaderView};
use risc0_zkvm::{default_executor, ExecutorEnv};
use std::collections::HashMap;

type Shards = HashMap<([u8; 32], [u8; 32]), Vec<u8>>;

fn run(elf: &[u8], kind: CallKind, payload: &[u8]) -> (GuestOutput, u64) {
    let mut b = ExecutorEnv::builder();
    b.write_slice(&to_borsh_frame(&kind));
    b.write_slice(&to_frame(payload));
    let env = b.build().expect("env builds");
    let info = default_executor().execute(env, elf).expect("the guest runs");
    let cycles = info.cycles();
    let journal = info.journal.bytes;
    let len = u32::from_le_bytes(journal[..4].try_into().unwrap()) as usize;
    let out: GuestOutput = borsh::from_slice(&journal[4..4 + len]).expect("a guest output");
    (out, cycles)
}

/// Run one transaction: plan, then every apply. Returns (plan, applies, total).
fn execute(elf: &[u8], registry: &[u8; 32], tx: &BuiltTransaction, shards: &mut Shards) -> (u64, Vec<u64>, u64) {
    let signers = tx.signer_ids();
    let input = PlanInput {
        self_account_id: AccountId::new(*registry),
        caller_account_id: None,
        accounts: tx
            .accounts
            .iter()
            .map(|a| {
                AccountMeta::new(
                    AccountId::new(a.selector.account),
                    signers.contains(&a.selector.account),
                    AccountId::new(a.selector.program),
                )
            })
            .collect(),
        instruction_data: tx.instruction_data.clone(),
    };
    let (out, plan_cycles) = run(elf, CallKind::Plan, &borsh::to_vec(&input).unwrap());
    let GuestOutput::Plan(plan) = out else { panic!("plan returned an apply") };
    let mut applies = Vec::new();
    for effect in plan.effects {
        let key = (*effect.selector.account_id.value(), *effect.selector.program_account_id.value());
        let pre = shards.get(&key).cloned().unwrap_or_default();
        let ai = ApplyInput {
            self_account_id: AccountId::new(*registry),
            selector: ProgramShardSelector::new(effect.selector.account_id, effect.selector.program_account_id),
            pre_data: pre.try_into().expect("fits"),
            effect_data: effect.data,
        };
        let (out, c) = run(elf, CallKind::Apply, &borsh::to_vec(&ai).unwrap());
        let GuestOutput::Apply(a) = out else { panic!("apply returned a plan") };
        if let Some(post) = a.post_data {
            shards.insert(key, post.into_inner());
        }
        applies.push(c);
    }
    let total = plan_cycles + applies.iter().sum::<u64>();
    (plan_cycles, applies, total)
}

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "../../artifacts/programs/lez_registry.bin".into());
    let elf = std::fs::read(&path).expect("read the guest binary");
    let registry = [0x42u8; 32];
    let program = [0x11u8; 32];
    let attester = [0x33u8; 32];
    let image = [0x77u8; 32];

    // The program's header, as the loader would hold it.
    let mut words = [0u32; 8];
    for (w, c) in words.iter_mut().zip(image.chunks_exact(4)) {
        *w = u32::from_le_bytes(c.try_into().unwrap());
    }
    let mut shards: Shards = HashMap::new();
    shards.insert(
        (program, *PROGRAM_LOADER_ACCOUNT_ID.value()),
        ProgramHeader { image_id: words, program_first_segment: AccountId::new([5; 32]), immutable: false }.to_bytes(),
    );
    let header = HeaderView { image_id: image, immutable: false };
    let at = core_::Stamp { block: 100, millis: 1_790_000_000_000 };
    let key = EntryKey { program, publisher: None };
    let claim = core_::SourceClaim {
        repository: "https://github.com/edenbd1/lp-0023-program-registry".into(),
        commit: [0xab; 20],
        source_digest: [0xcd; 32],
        build: core_::BuildEnvironment {
            image_digest: [0xef; 32],
            toolchain: "rustc 1.97.0-dev (rzup, risc0-build 3.0.5)".into(),
            network_disabled: true,
        },
    };
    let tags = vec!["registry".to_string(), "verified-builds".to_string()];
    let description = "The LEZ program registry, registered in itself";

    let txs: Vec<(&str, BuiltTransaction)> = vec![
        ("initialize", sdk::build_initialize(&registry)),
        (
            "register (deployer, with source claim)",
            sdk::build_register(
                &registry,
                &sdk::RegisterRequest {
                    program: &program,
                    author: &program,
                    deployer: true,
                    header,
                    seq_index: 0,
                    at,
                    name: "provenance",
                    version: "0.3.0",
                    description,
                    tags: &tags,
                    idl: "zDvZRwzm9Ni32iyvoy868YVrBHFz8vo3oZirrCKhFMMn9aXuinfQ",
                    source: Some(claim.clone()),
                },
            )
            .unwrap(),
        ),
        (
            "update (version, description, IDL)",
            sdk::build_update(&registry, &key, &program, "0.3.1", description, "zDvZRwzm9Ni32iyvoy868YVrBHFz8vo3oZirrCKhFMMn9aXuinfQ").unwrap(),
        ),
        ("claim_source", sdk::build_claim_source(&registry, &key, &program, claim).unwrap()),
        ("record_rebuild (match, offline: Verified)", sdk::build_record_rebuild(&registry, &key, header, image, at, &attester)),
        ("sync_header (unchanged)", sdk::build_sync_header(&registry, &key, header)),
        ("freeze", sdk::build_freeze(&registry, &key, &program)),
    ];

    println!("guest  {path}");
    println!("| instruction | plan | applies | total cycles (execution gas) |");
    println!("|---|---:|---|---:|");
    for (name, tx) in &txs {
        let (p, a, t) = execute(&elf, &registry, tx, &mut shards);
        let a: Vec<String> = a.iter().map(u64::to_string).collect();
        println!("| `{name}` | {p} | {} | {t} |", a.join(" + "));
    }
    let entry = shards.get(&(sdk::entry_address(&registry, &program), registry)).expect("an entry was written");
    let e: core_::Entry = borsh::from_slice(entry).unwrap();
    println!("\nfinal entry: {} {} {:?} frozen={}, {} bytes on chain", e.name, e.version, e.verification.state, e.frozen, entry.len());
}
