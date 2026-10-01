# Provenance: the LEZ program registry

**Provenance** tells you what a program on the Logos Execution Zone is, who
published it, and whether its bytecode was built from the source it names.

A program on LEZ v0.3 is an account: a header its deployer signed for, pointing
at bytecode whose RISC0 ImageID the loader recorded. Nothing in that account
says what the program does, who wrote it, what its interface looks like, or
whether the source you are reading is the source that is running. Provenance is
an on-chain registry that answers those questions, and tooling that answers the
last one honestly.

> **Status: ported to LEZ v0.3.0.** The program, SDK, CLI, indexer, explorer and
> Basecamp module target v0.3.0 (`db66590ab`). The whole lifecycle runs against
> a real v0.3.0 standalone sequencer: [`evidence/e2e-v03-local.txt`](evidence/e2e-v03-local.txt).
> **Testnet v0.3 deployment: pending funding.** The program id goes here and in
> [`DEPLOYMENTS.md`](DEPLOYMENTS.md) once it is deployed; the commands are in
> [`docs/deploy-v03.md`](docs/deploy-v03.md).

## Run it

```bash
./scripts/demo.sh
```

From a clean clone on macOS (Apple Silicon) or Linux (x86_64). It fetches LEZ
v0.3.0 into `_external/lez` (or uses `LEZ_SRC`), builds the standalone
sequencer and the wallet once, then runs `scripts/e2e-v03-local.sh`: deploy the
registry through the v0.3 wallet, register it in itself, rebuild it from source
with the network denied and verify it, refuse an unauthorised deployer entry on
chain, publish a third-party entry, update, record a mismatch, make a verdict
stale by repointing a header, freeze, then list, search and resolve. Needs Rust,
the RISC0 toolchain (`rzup`), `python3` and `curl`.

## How it works on v0.3

**Entries are keyed by the program account.** A program's deployer entry lives
at `pda(registry, sha256("lez-registry:entry:v1" ‖ program))`; a third party's
entry about it at the same with `":third-party:" ‖ publisher` added. Resolving a
program is one hash and one account read, with no index in the path.

**The deployer entry is provable.** On v0.3 the loader refuses to create a
program header unless the header account signs, so the header key is the
deploy authority. `register` without `--third-party` requires the author to be
that same account, signing. Anyone else's attempt is refused on chain (error
`E9010`, shown in the transcript), and clients tell the two apart from the
entry's `publisher` field without trusting any service.

**The recorded image and frozen flag are checked on chain.** `register`,
`record_rebuild` and `sync_header` carry the header the client read; the guest
emits a read-only effect on the program's `program_loader` shard that fails
unless the header there is exactly that `(image_id, immutable)`. The entry
records both, so a verdict is always about a specific ImageID, and a verdict
about an immutable header can never go stale.

**Time comes from the chain.** The client reads the clock account and passes
`Stamp { block, millis }`; the guest sets the transaction's block and timestamp
validity windows to `[block, block + 600)` and `[millis, millis + 600000)`,
which the runtime enforces. A registration time is never in the future and at
most ten minutes older than the block that includes it.

**One instruction enum.** `lez_registry_core::instruction::Instruction` (borsh)
is decoded by the guest and built by the SDK, so a transaction the SDK builds
is by construction one the program decodes.

### The seven instructions

| instruction | who | accounts |
|---|---|---|
| `initialize` | anyone, once | global |
| `register` | the program's header account (deployer entry) or any signer (third-party) | entry, global, directory, program@loader, author |
| `update` (version, description, IDL pointer) | the entry's author | entry, author |
| `claim_source` (repository, commit, source digest, build config) | the entry's author | entry, author |
| `record_rebuild` | anyone; the attester is recorded | entry, program@loader, attester |
| `sync_header` | anyone | entry, program@loader |
| `freeze` | the entry's author, one way | entry, author |

`claim_canonical` from the v0.2.4 design is gone: v0.2.4 had no deploy
authority, so the canonical slot had to be won by rebuilding; v0.3 has one.

### Verification states

| state | meaning |
|---|---|
| `Unclaimed` | no source asserted: the honest default |
| `Claimed` | a source is asserted and nobody has rebuilt it |
| `Verified` | rebuilt with the network disabled, and the ImageID matched the header on chain |
| `Unreproducible` | rebuilt, but the claimed build had network access, so the match proves nothing |
| `Mismatched` | rebuilt, and it does not produce the deployed ImageID |
| `Stale` | a verdict was recorded, then the header was repointed at different bytecode |

Verification proves one thing: rebuilding the stated source at the stated
commit with the stated toolchain produces the bytecode that is deployed. It
does not prove the program is safe. A badge always carries its attester, and
`is_self_attested` says when the only voucher is the author.

## CLI

```bash
cargo build --release --manifest-path cli/Cargo.toml
export LEZ_RPC=https://testnet.lez.logos.co        # or a local sequencer
export PROVENANCE_REGISTRY=<registry account>      # pending the v0.3 deployment

lez-registry lookup <PROGRAM>                      # resolve: one hash, one read
lez-registry lookup <PROGRAM> --publisher <ACCOUNT>
lez-registry list
lez-registry search token
lez-registry program-header <PROGRAM>              # ImageID and frozen flag from the loader

lez-registry register <PROGRAM> --name N --version V --description D --idl z… \
  --repo URL --commit HEX40 --source-digest HEX --image-digest HEX --toolchain T --offline \
  --key header.key --payer payer.key               # add --third-party for your own entry about it
lez-registry update <PROGRAM> --version V --idl z… --key author.key --payer payer.key
lez-registry claim-source <PROGRAM> --repo … --key author.key --payer payer.key
lez-registry record-rebuild <PROGRAM> --file rebuilt.bin --key attester.key --payer payer.key
lez-registry sync-header <PROGRAM> --payer payer.key
lez-registry freeze <PROGRAM> --key author.key --payer payer.key

lez-registry verify rebuilt.bin --program <PROGRAM> --mark   # locally verified, for you only
lez-registry image-id rebuilt.bin
```

Keys are files holding a 32-byte secret in hex, the form
`wallet account import public --private-key` takes, so the same account works
in the LEZ wallet and here (`lez-registry keygen --out FILE` makes one). The fee
payer must hold LGO. On v0.3 a failed call is charged and reverted rather than
rejected, so after every write the CLI reads the chain back and prints
`REVERTED` when nothing changed. A refused call is charged its whole gas limit
(a panicked guest reports no cycle count), which is why the CLI's default limit
is 1,000,000, about ten times a `register`; `PROVENANCE_GAS_LIMIT` overrides it.

## SDK: resolve a program id to metadata and IDL

This is the path a wallet or explorer takes ([`sdk/examples/resolve.rs`](sdk/examples/resolve.rs)):

```rust
use lez_registry_sdk::{entry_address, parse_account, Client, VerificationState};

let registry = parse_account("<registry account>").unwrap();
let program = parse_account("<program account>").unwrap();
let client = Client::new("https://testnet.lez.logos.co", registry);

// Where the answer lives: a pure function of the two ids. No index.
let addr = entry_address(&registry, &program);

match client.entry(&program)? {
    None => println!("unknown program"),
    Some(e) => {
        println!("{} {} by {:?}", e.name, e.version, e.author);
        // The IDL lives on Logos Storage; the entry holds its content id. If it
        // cannot be fetched or parsed, show the entry anyway, IDL unavailable.
        let idl = e.idl;
        // Is the verdict about the bytecode that runs now?
        let live = client.program_header(&program)?;
        let current = live.is_some_and(|h| h.image_id == e.image_id);
        let verified = current && e.verification.state == VerificationState::Verified;
    }
}
```

Writing goes through the same crate: `build_register` and friends produce a
`BuiltTransaction` (borsh instruction plus shard selectors), and
`Client::submit` signs it with BIP-340 keys, adds the fee payer as co-signer,
sends it with `sendTransaction`, and waits for inclusion. `cargo run --example
resolve -- <REGISTRY> <PROGRAM>` runs the example.

## The IDL, and why it is hand-maintained

The prize asks for the registry program to be built with SPEL and its IDL to be
generated by SPEL. SPEL has no release for LEZ v0.3: its latest,
`v0.7.0-rc.2` (2026-09-28), still pins LEZ v0.2.4, whose program model
(pre/post account states, ImageID program ids) v0.3 replaced with plan/apply
over per-program shards and header accounts. A SPEL build of this program
cannot run on v0.3, so the v0.3 guest is written against `lee_core` v0.3.0
directly.

The IDL, [`idl/lez_registry.idl.json`](idl/lez_registry.idl.json), is therefore
written by hand **in SPEL's IDL format**, and kept honest by
[`sdk/tests/idl_drift.rs`](sdk/tests/idl_drift.rs), which:

- parses it with SPEL's own IDL types (`spel-framework-core/src/idl.rs`,
  vendored under `vendor/spel/` with SPEL's licences);
- checks the instruction list against the instruction enum, in order;
- borsh-encodes a sample of every instruction and account type with the real
  types and decodes it back using only the IDL, field by field;
- compares every instruction's accounts with what the SDK puts on the wire;
- checks every error code the guest raises is listed, and nothing else is.

Reordering, renaming or retyping a field in either place fails the test. When
SPEL ships v0.3 support, the IDL is regenerated with it and this test becomes a
comparison of two generated files.

## Basecamp module, explorer, indexer

- [`app/`](app/README.md): the Logos Core / Basecamp UI module. Browses,
  filters and resolves entries by deriving addresses locally and reading the
  registry's shard; holds no keys. The sequencer and the registry are settings
  in the panel (or `LEZ_RPC` / `PROVENANCE_REGISTRY`). Packaged for Basecamp
  0.3.0 with logos-module-builder: `nix build ./app#lgx-portable`, then
  `scripts/install-local.sh <user-dir> result/*.lgx`. It is published through
  a module catalog forked from `logos-modules-release-base`, for darwin-arm64
  and linux-amd64, at
  `https://raw.githubusercontent.com/edenbd1/lp-0023-provenance-catalog/main/logos-repo.json`
  (prepared, not yet published; [`docs/publish.md`](docs/publish.md)).
- [`explorer/`](explorer/README.md): a read-only, self-hostable web explorer
  for the registry (`--registry <ACCOUNT> --endpoint <URL>`), no database, no
  CDN, no analytics.
- `indexer/`: optional self-hostable search
  (`lez-registry-indexer <REGISTRY> --endpoint <URL>`). Every result can be
  re-derived and re-read without it.

## Design positions

**What lives on chain?** The entry: metadata, the header's ImageID and frozen
flag, the source claim and the verdict. The IDL JSON and the source live off
chain, on Logos Storage and the named repository; the entry holds their
pointers. An IDL is kilobytes, and an on-chain kilobyte is paid for by everyone
who syncs the zone.

**Where does verified source live?** A claim carries `source_digest`, a hash
over the source tree as built, as well as the repository URL. Any mirror that
reproduces the digest is as good as the original host, so the URL is a hint
rather than a liveness dependency.

**Is there a trusted-signer overlay?** No. The registry records whether an entry
came from the program's deployer or anyone else and who attested each verdict,
and refuses to rank them. Deciding which attesters to trust is the reader's.

More in [`docs/design-positions.md`](docs/design-positions.md).

## Cost

Every instruction is about 1% of the 10M execution-gas ceiling or less:
`register` 106,297 cycles, `update` 56,326, on the sealed guest `dd12f4a8`.
Measured per plan and apply call by the method of LEZ v0.3.0's
`tools/cycle_bench`, in [`CYCLES.md`](CYCLES.md) with `tools/cycles-v03`.

## Layout

- `src/lib.rs`: entry types, address derivation, the verification state
  machine, the instruction enum. `no_std`, no I/O; the guest and every client
  link the same rules.
- `program/`: the v0.3 guest (plan/apply over `lee_core` v0.3.0).
- `sdk/`: resolve, walk, build, sign and submit; `tests/idl_drift.rs`.
- `cli/`: the `lez-registry` command.
- `indexer/`, `explorer/`, `app/`: search, web view, Basecamp module.
- `idl/`: the registry's IDL, in SPEL's format.
- `scripts/e2e-v03-local.sh`: the full lifecycle on a local v0.3.0 sequencer.
- `scripts/build-sealed.sh`: the guest build with the container's network off.
- `scripts/verify-source.sh`, `scripts/verify-onchain.sh`: claim fields, and a
  read-only re-check of a deployment.
- `tools/guest-native`: build the guest without Docker; `tools/cycles-v03`:
  cycle costs.

## Licence

MIT OR Apache-2.0.
