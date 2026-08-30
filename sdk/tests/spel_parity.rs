// SPDX-License-Identifier: MIT OR Apache-2.0
//! The in-process builder against `spel`, byte for byte.
//!
//! `spel` is the tool that populated the live registry, so its serialization is
//! the one the chain has already accepted. This test builds the same register
//! and update transactions two ways — through [`lez_registry_sdk::build_register`]
//! and by shelling out to `spel --dry-run=json` — and asserts the instruction
//! data is identical to the byte. If the encoder ever drifts from `spel`, and
//! therefore from the chain, this is where it shows.
//!
//! Skipped when `spel` is not on `PATH` (it is not on CI), so a missing tool
//! reads as "not run" rather than as a failure. When `spel` is present the
//! assertion is exact — a version that disagrees fails loudly rather than
//! skipping.
//!
//! Run it with the rest of the ignored set: `cargo test -- --ignored`.

use lez_registry_core as core_;
use lez_registry_sdk::{build_register, build_update, Publisher, RegisterRequest, UpdateRequest};
use std::process::Command;

const IDL: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../idl/lez_registry.idl.json");
const REGISTRY_HEX: &str = "f59751e932e856f7565f34e79ae726791794dc68e55f2413bc0375008cc07bf8";
const CLOCK_B58: &str = "4BdcjoXkq786TMWcBGGHqcxeLYMZmn17rL4eM9ZyRWNU";
const AUTHOR_B58: &str = "DumJ4LCBnHE9jUu2yxPfqdL14g3v756Gzby6LuT9hE51";

fn hex32(s: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).expect("valid hex");
    }
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Run `spel --dry-run=json` for `instruction` and return its `instruction_data`
/// hex string, or `None` if `spel` is not installed.
fn spel_instruction_data(instruction: &str, extra_args: &[&str]) -> Option<String> {
    let mut cmd = Command::new("spel");
    cmd.args([
        "--dry-run=json",
        "--idl",
        IDL,
        "--program",
        REGISTRY_HEX,
        "--",
    ]);
    cmd.arg(instruction);
    cmd.args(extra_args);
    cmd.args(["--author", AUTHOR_B58]);

    let out = match cmd.output() {
        Ok(o) => o,
        // Not installed — this whole test is a no-op, which is the CI case.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => panic!("failed to run spel: {e}"),
    };
    assert!(
        out.status.success(),
        "spel exited non-zero:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("spel --dry-run=json emits JSON");
    Some(
        v["instruction_data"]
            .as_str()
            .expect("instruction_data is a hex string")
            .to_string(),
    )
}

#[test]
#[ignore = "shells out to the spel binary"]
fn register_bytes_are_identical_to_spel() {
    let registry = hex32(REGISTRY_HEX);
    let program = hex32("1111111111111111111111111111111111111111111111111111111111111111");
    let author = base58_to_32(AUTHOR_B58);
    let clock = base58_to_32(CLOCK_B58);
    let seq_index = 5u64;

    let tx = build_register(
        &registry,
        &RegisterRequest {
            program: &program,
            author: &author,
            publisher: Publisher::Canonical,
            name: "cross check",
            version: "1.2.3",
            description: "compare with spel",
            clock: &clock,
            seq_index,
        },
    )
    .expect("valid metadata");

    // Feed spel the seeds the builder derived, so both encode the same values.
    let entry_seed = core_::entry_seed(&program);
    let seq_seed = core_::seq_seed(seq_index);
    let Some(from_spel) = spel_instruction_data(
        "register",
        &[
            "--entry-seed",
            &hex(&entry_seed),
            "--seq-seed",
            &hex(&seq_seed),
            "--program-id",
            &hex(&program),
            "--canonical",
            "1",
            "--name",
            "cross check",
            "--version",
            "1.2.3",
            "--description",
            "compare with spel",
            "--clock",
            CLOCK_B58,
        ],
    ) else {
        eprintln!("spel not on PATH — skipping the register parity check");
        return;
    };

    assert_eq!(
        hex(&tx.instruction_data_bytes()),
        from_spel,
        "the in-process register encoding drifted from spel's"
    );
}

#[test]
#[ignore = "shells out to the spel binary"]
fn update_bytes_are_identical_to_spel() {
    let registry = hex32(REGISTRY_HEX);
    let program = hex32("2222222222222222222222222222222222222222222222222222222222222222");
    let author = base58_to_32(AUTHOR_B58);
    // A real Logos Storage identifier, so the idl string is a non-trivial case.
    let idl = "zDvZRwzmA4EqenwX34C9oqpDpp4jP5SM8k7y5kE7DccFe7xVDHNG";

    let tx = build_update(
        &registry,
        &UpdateRequest {
            program: &program,
            author: &author,
            publisher: Publisher::Canonical,
            version: "9.9.9",
            description: "an update body",
            idl,
        },
    )
    .expect("valid metadata");

    let entry_seed = core_::entry_seed(&program);
    let Some(from_spel) = spel_instruction_data(
        "update",
        &[
            "--entry-seed",
            &hex(&entry_seed),
            "--version",
            "9.9.9",
            "--description",
            "an update body",
            "--idl",
            idl,
        ],
    ) else {
        eprintln!("spel not on PATH — skipping the update parity check");
        return;
    };

    assert_eq!(
        hex(&tx.instruction_data_bytes()),
        from_spel,
        "the in-process update encoding drifted from spel's"
    );
}

fn base58_to_32(s: &str) -> [u8; 32] {
    lez_registry_sdk::from_base58(s).expect("valid base58 account id")
}
