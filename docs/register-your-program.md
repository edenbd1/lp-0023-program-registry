# Register your LEZ program

For builders who have deployed a program on LEZ and want it to show up — with a
name, an author and a resolvable IDL — in wallets, explorers and developer tools
that read this registry. It takes a few minutes and costs one transaction.

> **Testnet 0.3.** The commands below are for the current public zone (LEZ
> v0.2.4). On 0.3 a program is identified by its header account, and the entry
> *you* register — signed by that account — is the one clients show as the
> deployer's. The steps stay the same; the identifiers change. See
> [lez-0.3-migration.md](lez-0.3-migration.md).

## What you need

- your program's id (`spel program-id <your.bin>`)
- its IDL JSON (SPEL writes it for you)
- a wallet that can sign on the zone (`LEE_WALLET_HOME_DIR`)

## 1. Put your IDL on Logos Storage

The registry keeps a pointer, not the IDL itself, so the entry stays cheap and
the IDL stays fetchable by anyone:

```bash
tools/storage-upload.sh path/to/your.idl.json    # prints a content id, z…
```

## 2. Register

```bash
cargo build --release --manifest-path cli/Cargo.toml
cli/target/release/lez-registry entry-address <PROGRAM_ID_HEX>   # where it will live
```

then register through `spel` with the registry's IDL, as
[`scripts/populate-demo.sh`](../scripts/populate-demo.sh) does — name, version,
description, the IDL content id, and the sequencer's clock account, from which
the registration time is read (you cannot choose it).

## 3. Optional, and worth it: prove your source

An entry can record the repository, commit and build configuration that
reproduce your bytecode. Anyone can then rebuild it and check the match; a user
whose rebuild matches can mark your program *locally verified* for themselves:

```bash
cli/target/release/lez-registry verify <rebuilt.bin> --expect <DEPLOYMENT_DIGEST> --mark --program <PROGRAM_ID_HEX>
```

A matching hash is necessary, not sufficient — `lez-registry verify --explain`
says why, and what the registry records to close the gap.

## 4. Check it

```bash
cli/target/release/lez-registry lookup <PROGRAM_ID_HEX>
```

or open it in the explorer. Only you — the registering author — can update the
entry afterwards.
