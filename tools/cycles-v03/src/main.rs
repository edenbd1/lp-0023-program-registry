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
//! The method is LEZ v0.3.0's `tools/cycle_bench` (same input writers, same
//! executor, one row per plan and per apply, warmup then timed runs); its
//! fixtures are built-in programs, so the registry's cases live here.
//!
//!   cargo run --release -- <lez_registry.bin>

use lee::program::Program;
use lee_core::{
    account::{AccountId, ProgramShardSelector},
    from_frame,
    program::{
        AccountMeta, ApplyInput, GuestOutput, PlanInput, ProgramHeader, PROGRAM_LOADER_ACCOUNT_ID,
    },
};
use lez_registry_core::{self as core_, instruction::EntryKey};
use lez_registry_sdk::{self as sdk, BuiltTransaction, HeaderView};
use risc0_zkvm::{default_executor, ExecutorEnv, ExecutorEnvBuilder};
use std::{collections::HashMap, time::Instant};

type Shards = HashMap<([u8; 32], [u8; 32]), Vec<u8>>;

/// Executor runs per call after one warmup run, as cycle_bench's default
/// `--exec-iters 5`.
const EXEC_ITERS: usize = 5;

/// One guest call: cycles, segments, best and mean executor wall time.
struct Row {
    cycles: u64,
    segments: usize,
    best_ms: f64,
    mean_ms: f64,
}

/// One guest call measured as LEZ v0.3.0's `tools/cycle_bench` measures it:
/// inputs written by `lee::program::Program::write_{plan,apply}_inputs` (the
/// writers the sequencer itself uses), `default_executor().execute` with no
/// proving, one warmup run discarded and `EXEC_ITERS` timed runs kept. The cycle
/// count is the executor's user cycles, identical on every run.
fn run(elf: &[u8], write: impl Fn(&mut ExecutorEnvBuilder)) -> (GuestOutput, Row) {
    let mut samples = Vec::with_capacity(EXEC_ITERS);
    let mut last = None;
    for iter in 0..=EXEC_ITERS {
        let mut b = ExecutorEnv::builder();
        write(&mut b);
        let env = b.build().expect("env builds");
        let started = Instant::now();
        let info = default_executor().execute(env, elf).expect("the guest runs");
        let ms = started.elapsed().as_secs_f64() * 1_000.0;
        if iter > 0 {
            samples.push(ms);
        }
        last = Some(info);
    }
    let info = last.expect("at least one run");
    let payload = from_frame(&info.journal.bytes).expect("a framed journal");
    let out: GuestOutput = borsh::from_slice(payload).expect("a guest output");
    let best_ms = samples.iter().copied().fold(f64::INFINITY, f64::min);
    let mean_ms = samples.iter().sum::<f64>() / samples.len() as f64;
    (out, Row { cycles: info.cycles(), segments: info.segments.len(), best_ms, mean_ms })
}

fn plan(elf: &[u8], input: &PlanInput) -> (GuestOutput, Row) {
    run(elf, |b| Program::write_plan_inputs(input, b).expect("plan input"))
}

fn apply(elf: &[u8], input: &ApplyInput) -> (GuestOutput, Row) {
    run(elf, |b| Program::write_apply_inputs(input, b).expect("apply input"))
}

/// Run one transaction: plan, then every apply, carrying shard contents.
fn execute(elf: &[u8], registry: &[u8; 32], tx: &BuiltTransaction, shards: &mut Shards) -> (Row, Vec<Row>) {
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
    let (out, plan_row) = plan(elf, &input);
    let GuestOutput::Plan(planned) = out else { panic!("plan returned an apply") };
    let mut applies = Vec::new();
    for effect in planned.effects {
        let key = (*effect.selector.account_id.value(), *effect.selector.program_account_id.value());
        let pre = shards.get(&key).cloned().unwrap_or_default();
        let ai = ApplyInput {
            self_account_id: AccountId::new(*registry),
            selector: ProgramShardSelector::new(effect.selector.account_id, effect.selector.program_account_id),
            pre_data: pre.try_into().expect("fits"),
            effect_data: effect.data,
        };
        let (out, row) = apply(elf, &ai);
        let GuestOutput::Apply(a) = out else { panic!("apply returned a plan") };
        if let Some(post) = a.post_data {
            shards.insert(key, post.into_inner());
        }
        applies.push(row);
    }
    (plan_row, applies)
}

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "../../artifacts/programs/lez_registry.bin".into());
    let elf = std::fs::read(&path).expect("read the guest binary");
    let image_id = Program::new(elf.clone().into()).expect("a RISC0 program binary").id();
    let image_hex: String = image_id.iter().flat_map(|w| w.to_le_bytes()).map(|b| format!("{b:02x}")).collect();
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

    println!("guest    {path}");
    println!("ImageID  {image_hex}");
    println!("method   LEZ v0.3.0 tools/cycle_bench (executor, no proving; best / mean of {EXEC_ITERS} runs after a warmup)\n");
    println!("| instruction | phase | user cycles | segments | exec ms (best / mean) |");
    println!("|---|---|---:|---:|---:|");
    let mut totals = Vec::new();
    for (name, tx) in &txs {
        let (p, a) = execute(&elf, &registry, tx, &mut shards);
        for (phase, r) in std::iter::once(("plan", &p)).chain(a.iter().map(|r| ("apply", r))) {
            println!("| `{name}` | {phase} | {} | {} | {:.2} / {:.2} |", r.cycles, r.segments, r.best_ms, r.mean_ms);
        }
        let applies: Vec<String> = a.iter().map(|r| r.cycles.to_string()).collect();
        let total = p.cycles + a.iter().map(|r| r.cycles).sum::<u64>();
        totals.push(format!("| `{name}` | {} | {} | {total} |", p.cycles, applies.join(" + ")));
    }
    println!("\n| instruction | plan | applies | total cycles (execution gas) |");
    println!("|---|---:|---|---:|");
    for t in &totals {
        println!("{t}");
    }
    // What a refused call costs: an update signed by someone who is not the
    // author. The apply panics; the executor reports the cycles it ran.
    let stranger = [0x99u8; 32];
    let refused = sdk::build_update(&registry, &key, &stranger, "9.9.9", "", "").unwrap();
    let signers = refused.signer_ids();
    let input = PlanInput {
        self_account_id: AccountId::new(registry),
        caller_account_id: None,
        accounts: refused
            .accounts
            .iter()
            .map(|a| AccountMeta::new(AccountId::new(a.selector.account), signers.contains(&a.selector.account), AccountId::new(a.selector.program)))
            .collect(),
        instruction_data: refused.instruction_data.clone(),
    };
    let (out, plan_row) = plan(&elf, &input);
    let plan_c = plan_row.cycles;
    let GuestOutput::Plan(planned) = out else { panic!() };
    let effect = planned.effects.into_iter().next().unwrap();
    let pre = shards.get(&(sdk::entry_address(&registry, &program), registry)).cloned().unwrap();
    let ai = ApplyInput {
        self_account_id: AccountId::new(registry),
        selector: ProgramShardSelector::new(effect.selector.account_id, effect.selector.program_account_id),
        pre_data: pre.try_into().unwrap(),
        effect_data: effect.data,
    };
    let mut b = ExecutorEnv::builder();
    Program::write_apply_inputs(&ai, &mut b).expect("apply input");
    b.session_limit(Some(10_000_000));
    match default_executor().execute(b.build().unwrap(), &elf) {
        Ok(info) => println!("\nrefused update: plan {plan_c}, apply exited {:?} after {} cycles", info.exit_code, info.cycles()),
        Err(e) => println!("\nrefused update (frozen entry): plan {plan_c} cycles, then the apply panics: {}. The executor reports no cycle count for a panicked session, and the sequencer charges such a call its whole gas limit.", e.root_cause()),
    }

    let entry = shards.get(&(sdk::entry_address(&registry, &program), registry)).expect("an entry was written");
    let e: core_::Entry = borsh::from_slice(entry).unwrap();
    println!("\nfinal entry: {} {} {:?} frozen={}, {} bytes on chain", e.name, e.version, e.verification.state, e.frozen, entry.len());
}
