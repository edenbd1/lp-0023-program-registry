# Register your LEZ program

For builders who have deployed a program on LEZ v0.3 and want it to show up,
with a name, an author and a resolvable IDL, in wallets, explorers and
developer tools that read this registry. One transaction.

## What you need

- your program's account id: the header account you passed to
  `wallet program-loader deploy --header`, and **its key**. On v0.3 that key is
  the deploy authority, and signing with it is what makes your entry the
  deployer's entry rather than a third party's.
- a fee payer holding LGO (`lez-registry balance <ACCOUNT>`)
- its IDL JSON
- the registry's account (`PROVENANCE_REGISTRY`) and a sequencer (`LEZ_RPC`)

Keys are files with the 64-hex-character secret, the form
`wallet account import public --private-key` takes. The wallet does not export
public-account keys, so create the header key with `lez-registry keygen` and
import it into the wallet before deploying (as [deploy-v03.md](deploy-v03.md)
does).

## 1. Put your IDL on Logos Storage

```bash
tools/storage-upload.sh path/to/your.idl.json    # prints a content id, z…
```

## 2. Register

```bash
cargo build --release --manifest-path cli/Cargo.toml
lez-registry program-header <PROGRAM>            # the ImageID the loader recorded
lez-registry register <PROGRAM> --name my-program --version 1.0.0 \
  --description "What it does" --tag token --idl z… \
  --key header.key --payer payer.key
```

The registry checks the header on chain, records its ImageID and whether it is
immutable, and stamps the registration with chain time. Without the header key,
register with `--third-party`: your entry sits next to the deployer's, keyed by
you, and is shown as a third party's.

## 3. Optional, and worth it: prove your source

```bash
./scripts/verify-source.sh                       # in your repository: the claim fields
lez-registry claim-source <PROGRAM> --repo <URL> --commit <HEX40> \
  --source-digest <HEX> --image-digest <HEX> --toolchain <T> --offline \
  --key header.key --payer payer.key
```

Pass `--offline` only for a build that ran with the network disabled; a rebuild
of a claim without it is recorded as `Unreproducible`, never `Verified`.
Anyone can then rebuild and check:

```bash
lez-registry verify rebuilt.bin --program <PROGRAM> --mark      # locally verified, for you
lez-registry record-rebuild <PROGRAM> --file rebuilt.bin --key me.key --payer payer.key
```

If you later repoint a mutable header, `lez-registry sync-header <PROGRAM>`
records the new ImageID and any verdict about the old one becomes `Stale`.

## 4. Check it

```bash
lez-registry lookup <PROGRAM>
```

Only you, the registering author, can update the entry afterwards.
