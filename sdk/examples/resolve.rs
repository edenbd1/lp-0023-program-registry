// SPDX-License-Identifier: MIT OR Apache-2.0
//! The integration path most consumers take, end to end.
//!
//! A wallet about to render a transaction, or an explorer decoding one, has one
//! question: **what is this program, and is the source it names the source it
//! was built from?** This is that, in about forty lines of real code.
//!
//! ```text
//! LEZ_RPC=http://127.0.0.1:3040 \
//! cargo run --example resolve -p lez-registry-sdk -- <REGISTRY> <PROGRAM>
//! ```
//!
//! Both are account ids in base58, as the LEZ wallet prints them.

use lez_registry_sdk::{
    base58, content_id_string, entry_address, hex, parse_account, Client, VerificationState,
};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(registry), Some(program)) = (
        args.first().and_then(|s| parse_account(s)),
        args.get(1).and_then(|s| parse_account(s)),
    ) else {
        eprintln!("usage: resolve <REGISTRY> <PROGRAM>");
        std::process::exit(2);
    };
    let endpoint = std::env::var("LEZ_RPC").unwrap_or_else(|_| "https://testnet.lez.logos.co".into());

    // 1. Where the answer lives: a pure function of the two account ids. No
    //    request, no index, nothing that can be down or out of date.
    let addr = entry_address(&registry, &program);
    println!("entry account  {}", base58(&addr));

    // 2. Read it, and the program's header, from the chain.
    let client = Client::new(endpoint, registry);
    let entry = match client.entry(&program) {
        Ok(Some(e)) => e,
        // Not an error: render it as an unknown program, which is a truthful
        // thing to tell a user about to sign.
        Ok(None) => {
            println!("not registered: render this as an unknown program");
            return;
        }
        Err(e) => {
            eprintln!("could not reach the registry: {e}");
            std::process::exit(1);
        }
    };
    println!("name           {} {}", entry.name, entry.version);
    println!("description    {}", entry.description);
    match entry.idl {
        // The IDL itself lives on Logos Storage; the entry holds its identifier.
        // An unresolvable IDL is shown as unavailable; the metadata above stands.
        Some(idl) => println!("idl            {}", content_id_string(&idl)),
        None => println!("idl            unavailable"),
    }

    // 3. Is the verdict still about the bytecode that runs? On v0.3 a mutable
    //    header can be repointed, so compare the entry's recorded ImageID with
    //    the header as it is now.
    let live = client.program_header(&program).ok().flatten();
    let current = live.is_some_and(|h| h.image_id == entry.image_id);

    // 4. The part that decides what a UI is allowed to say.
    let verdict = match entry.verification.state {
        _ if !current => "the program was redeployed since this entry looked: treat as unverified",
        VerificationState::Verified if entry.verification.is_self_attested(&entry.author) => {
            "verified, but only by the party that shipped it"
        }
        VerificationState::Verified => "verified by an independent rebuild",
        VerificationState::Mismatched => "MISMATCHED: a source is published and does not rebuild",
        VerificationState::Unreproducible => "a rebuild was tried with the network up, so it proves nothing",
        VerificationState::Claimed => "a source is claimed, nobody rebuilt it",
        VerificationState::Unclaimed => "no source claimed",
        VerificationState::Stale => "verified once, but against bytecode that no longer runs",
    };
    println!("verification   {verdict}");
    if entry.header_immutable {
        println!("header         immutable: this verdict can never go stale");
    }
    if let Some(who) = entry.verification.attester {
        println!("attested by    {}", base58(&who));
    }
    if let Some(claim) = &entry.verification.claim {
        println!("source         {} @ {}", claim.repository, hex(&claim.commit));
    }
    // Verified means the deployed bytecode was produced from the stated source;
    // it says nothing about whether that source is safe, audited or benign.
    println!("\nVerified is not safe. It means the bytecode matches the source, nothing more.");
}
