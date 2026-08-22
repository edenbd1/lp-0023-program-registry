// Runs the guest under the RISC0 executor (no proving) and prints the cycle
// table. Executor only: these are the counts that decide whether an operation
// fits the LEZ public-execution budget, and they are deterministic.

use lez_registry_methods::{LEZ_REGISTRY_GUEST_ELF, LEZ_REGISTRY_GUEST_ID};
use risc0_zkvm::{default_executor, ExecutorEnv};

type Row = (String, u64, u64, u64, usize);

const LEZ_PUBLIC_EXECUTION_CAP: u64 = 32_000_000;

fn main() {
    let env = ExecutorEnv::builder().build().expect("env builds");
    let info = default_executor()
        .execute(env, LEZ_REGISTRY_GUEST_ELF)
        .expect("guest executes");

    let (rows, baseline, whole_register, whole_update, whole_claim, whole_takeover): (
        Vec<Row>,
        u64,
        u64,
        u64,
        u64,
        u64,
    ) = info.journal.decode().expect("journal decodes");

    println!("guest image id : {LEZ_REGISTRY_GUEST_ID:?}");
    println!("total cycles   : {}", info.cycles());
    println!("measurement baseline subtracted from every row: {baseline} cycles\n");
    println!("| op | cases | median | min | max |");
    println!("|---|---:|---:|---:|---:|");
    for (name, med, lo, hi, n) in &rows {
        println!("| `{name}` | {n} | {med} | {lo} | {hi} |");
    }
    println!("\n| instruction body | cycles | share of the cap |");
    println!("|---|---:|---:|");
    for (name, n) in [
        ("register", whole_register),
        ("update", whole_update),
        ("claim_source", whole_claim),
        ("claim_canonical", whole_takeover),
    ] {
        println!(
            "| `{name}` | {n} | {:.4}% |",
            n as f64 * 100.0 / LEZ_PUBLIC_EXECUTION_CAP as f64
        );
    }
    println!("\nLEZ public-execution cap: {LEZ_PUBLIC_EXECUTION_CAP}");
}
