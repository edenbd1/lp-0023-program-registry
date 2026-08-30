// SPDX-License-Identifier: MIT OR Apache-2.0
//! The explorer bakes its own source commit into the binary via
//! `option_env!("EXPLORER_SOURCE_COMMIT")`, so a visitor can see, on `/about`,
//! exactly which commit the running site was built from. cargo does not track
//! arbitrary environment variables for rebuilds on its own, so without this a
//! changed commit would leave a stale value in an incrementally-cached binary.
//! Declaring the dependency here makes the self-verification honest: change the
//! commit, and the binary — and therefore its digest — is rebuilt.

fn main() {
    println!("cargo:rerun-if-env-changed=EXPLORER_SOURCE_COMMIT");
}
