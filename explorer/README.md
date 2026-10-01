# LEZ Registry Explorer

A public, self-hostable block explorer for the LEZ program registry — Etherscan
for the Logos Execution Zone, minus the part where you have to trust the site.

It is a **pure view**. It holds no database. Every value on every page is read
live from the chain through the same `lez-registry-sdk` the CLI uses, and every
address it prints is *computed* from a program id rather than looked up in a
table only this site has. Turn it off and nothing about the registry changes:
`lez-registry lookup <id>` and the SDK still resolve every program without it.

> The registry works fully without this site. The explorer is an accelerator for
> browsing, never a dependency for resolving. This is the same relationship the
> `indexer` next door has to the chain, and it is deliberate.

## Run it — one command

With Docker (from the repo root, so the path dependencies resolve):

```bash
docker build -f explorer/Dockerfile -t lez-registry-explorer .
docker run --rm -p 8080:8080 lez-registry-explorer
# then open http://localhost:8080
```

Or straight from source:

```bash
cargo run --release --manifest-path explorer/Cargo.toml
# then open http://127.0.0.1:8080
```

That is the whole install. No database to provision, no migration, no secret, no
account. It comes up pointed at the documented public testnet deployment and the
public sequencer, warms one walk of the registry, and serves.

**Run your own instance → identical bytes, identical data.** The *data* is
identical because it was never the server's — it belongs to the chain; point two
instances at the same registry and endpoint and they render the same rows. The
*bytes* of the site are identical because the build is reproducible: the same
commit built on the same pinned base image produces a byte-identical binary, and
the explorer shows you its own sha256 so you can check (see
[Self-verification](#self-verification--the-explorer-verifies-itself)).

### The container, in detail

The image is a **sealed** build, in the same spirit as
[`../scripts/build-sealed.sh`](../scripts/build-sealed.sh): a fetch phase with the
network vendors every crate and checks it against `explorer/Cargo.lock`, then a
build phase with `RUN --network=none` compiles the binary offline, so a build
script cannot reach out and the binary is a function of the pinned sources alone.
The runtime image holds only the binary — the CSS and JS are compiled in via
`include_str!` — runs as `nobody`, and defaults to `--host 0.0.0.0` so the
published port is reachable. Append any flag from the table below to `docker run`
to override a default; stamp the commit so `/about` can name it:

```bash
docker build -f explorer/Dockerfile \
  --build-arg SOURCE_COMMIT=$(git rev-parse HEAD) \
  -t lez-registry-explorer .
```

### Options

```
lez-registry-explorer [REGISTRY_HEX] [options]

  --registry <HEX>          registry program id      (default: the testnet deployment below)
  --endpoint <URL>          sequencer JSON-RPC        (default: https://testnet.lez.logos.co)
  --host <ADDR>             bind address             (default: 127.0.0.1; the container passes 0.0.0.0)
  --port <N>                listen port              (default: 8080)
  --storage-gateway <URL>   Logos Storage gateway to resolve IDLs (optional; see below)
  --cache-ttl <SECONDS>     in-memory walk cache TTL (default: 10)
  --no-cache                read the chain on every request
```

Loopback is the default; a container opts into `--host 0.0.0.0` so its published
port is reachable, which is exactly what the `Dockerfile` does.

The registry is required (`--registry <ACCOUNT>` or `PROVENANCE_REGISTRY`): the
testnet v0.3 deployment is pending funding, so there is no default to fall back
to (see [`../DEPLOYMENTS.md`](../DEPLOYMENTS.md)). The endpoint defaults to
`https://testnet.lez.logos.co`. With Docker:
`docker run --rm -p 8080:8080 -e PROVENANCE_REGISTRY=<ACCOUNT> lez-registry-explorer`.

## What it shows

- **`/` — the registry.** Every registered program in one scannable table:
  name, version, program id, author, a canonical / third-party tag, and a
  verification badge. A stats strip reads the on-chain counter. Beneath it, a
  legend spells out all five states — because on this page a badge is never bare.
- **`/program/<hex>` — an entry.** Full on-chain metadata; the source claim
  (repository, commit, source digest, toolchain, and whether the build was
  offline); the resolved IDL or its content id; and a **derivation trace** that
  shows, in your face, how the account it just read was computed from the program
  id — ending in the exact `getAccount` / `lez-registry lookup` you could run
  instead. A third-party entry is `/program/<hex>?publisher=<hex>`.
- **`/search?q=` — search.** By name, author, program-id prefix, version, tag or
  state. The same predicate the SDK and the `indexer` apply.
- **`/about` — how this stays decentralised.** The four principles stated plainly
  (read-only/no DB; open-source one-command self-host; the same open-source
  indexer; the registry works without the site), the live on-chain stats, the five
  states with the "verified ≠ safe" caveat, and — the killer bit —
  [the explorer verifying itself](#self-verification--the-explorer-verifies-itself).
- **`/api/programs.json`** — a machine-readable mirror of the list, so the view
  is scriptable and its numbers checkable. Each record carries the derivation
  needed to re-check it without this API.

Every `/program/<hex>` permalink also carries **Open Graph / Twitter-card** meta
(title = program name + verification state, a plain-English description, and an
absolute canonical URL built from the request's `Host`), so a shared link renders
a clean summary card on X or Discord. The card pulls in no external image and no
external service — it is a `summary` card built from strings already read from the
chain — so a shared link does not compromise the "pure view" claim.

## How it reads the chain

No custom protocol and no privileged access — the same reads a wallet would do.

1. **Enumerate.** `Client::walk()` reads the singleton global account
   (`getAccount` on the derived global address) for `total_entries`, then walks
   the registry's own on-chain **sequence directory** slot by slot, deriving each
   entry account and decoding it. LEZ has no `getProgramAccounts` and no
   accounts-by-owner — there is nothing to enumerate *from* — so this directory,
   maintained by the registry program itself, is the only discovery path, and
   every row it yields is re-derivable.
2. **Resolve.** `Client::entry(program_id)` (or `third_party_entry`) derives the
   entry address as `pda(registry, sha256("lez-registry:entry:v1" ‖ program_id))`
   and does a single `getAccount`. One hash, one read, no index.
3. **Decode.** borsh, via `lez-registry-core` — the *same crate the on-chain
   program links*, so an address or a field this site shows is the one the
   program insists on. The explorer reimplements no derivation of its own.
4. **Resolve IDLs (optional).** An entry stores only a 38-byte Logos Storage
   content id on chain. With `--storage-gateway <URL>` set, the explorer does one
   read-only `GET <gateway>/<cid>`, checks the bytes parse as JSON, and renders
   them; without it, the content id is shown with the command to fetch it. Either
   way a missing or unparseable IDL degrades gracefully and never touches the
   on-chain metadata.

### If you want the indexer instead

Search here runs the identical filter over the identical walk the repo's
open-source [`indexer`](../indexer) exposes as a service. Both are self-hosted
and hold no database; run the `indexer` when you want the search to be a separate
process. Neither is ever required to *resolve* a program — that is always a
derivation plus one read.

## Self-verification — the explorer verifies itself

This registry's whole argument is that you should be able to **rebuild what is
running and confirm it**, rather than trust a badge. The explorer holds *itself*
to that. At start-up it reads its own executable (`current_exe()`), hashes it, and
shows that sha256 — alongside its source repo and the commit it was built from —
on `/about` and in the footer of every page. That number is the digest of the
binary serving you.

Because the container build is sealed and reproducible, that digest is the same
for everyone who builds the same commit on the same pinned base image. So you can
rebuild the explorer and confirm the running site, byte-for-byte:

```bash
# 1. Rebuild the explorer at the commit /about shows, sealed and reproducible
git clone https://github.com/edenbd1/lp-0023-program-registry
cd lp-0023-program-registry && git checkout <commit-from-/about>
docker build -f explorer/Dockerfile \
  --build-arg SOURCE_COMMIT=$(git rev-parse HEAD) \
  -t lez-registry-explorer .

# 2. Print the binary's sha256 and compare with the running site's /about
docker run --rm --entrypoint sha256sum lez-registry-explorer \
  /usr/local/bin/lez-registry-explorer
```

Three independent numbers agree by construction: the build stage prints the
digest, `sha256sum` over the packaged binary reports it, and the running process
hashes itself and shows it. A `cargo run` binary is honest too, but its digest is
machine-specific (paths, toolchain) and `/about` says so plainly — reproducibility
across rebuilders is a property of the **pinned image**, which is why the digest
worth comparing is the container's.

Measured, not asserted: two independent from-scratch builds of this repo (the
second `docker build --no-cache`) produced the byte-identical binary
`9701bbbb3bbe3d281d2c1e843014907056a3e1975b549116f3e3fba2dda05344` for commit
`5cd798c` on `rust:1.90.0-slim-bookworm`. One honest caveat: that digest is for
the platform Docker built (here `linux/arm64`); a rebuild for a different target
architecture reproduces *its own* stable digest, not this one. Pin
`--platform` (and, for the strictest guarantee, the base image by `@sha256:`
digest) when you publish a number for others to match.

**Could the explorer be registered as an entry in the registry it displays?** The
registry keys on RISC0 ImageIDs — on-chain programs — and this explorer is a
native binary, so honestly it is *not* an on-chain entry. What it does instead is
practise the same source-verification discipline it exposes: a named source, a
pinned toolchain, a **sealed offline build**, and a **published digest anyone can
reproduce**. The registry's own program does the on-chain version of exactly this
— it is slot 0, the registry registered in itself, `Verified` and self-attested —
which `/about` links to as the worked example.

## Why this cannot re-centralise the registry

The registry's whole design refuses mandatory external services. A block explorer
is the classic place that promise quietly dies — the "official" explorer becomes
the thing everyone actually trusts. This one is built so it cannot:

- **No database.** The only state is an in-memory walk cache with a short TTL,
  rebuilt from the chain and discarded on restart. Nothing resolves *from* it; it
  is a latency shim, not a source of truth. Run with `--no-cache` to remove even
  that.
- **No private index, no hosted secret.** Discovery is the on-chain sequence
  directory; search is open-source and re-runnable.
- **No web fonts, no analytics, no outbound calls** beyond the sequencer and, if
  you opt in, a Storage gateway. A "trust nothing" view must not phone a CDN, so
  the visual identity is carried by system fonts and a monospace type system.
- **Self-hostable in one command**, and identical for everyone who runs it.
- **It shows its work.** The derivation trace on every entry, and the `getAccount`
  command beside it, exist so a reader can bypass the site entirely and get the
  same answer from the chain.
- **It verifies itself.** The explorer publishes its own source, commit and binary
  sha256, so you can rebuild it reproducibly and confirm the running site — the
  same discipline it holds programs to, turned on the site itself.

## Design notes

- **Verified is the only green, and it is earned.** Each of the five states owns
  a distinct hue and a plain sentence; a self-attested `Verified` renders apart
  from an independently rebuilt one, because those are different claims. "Verified
  does not mean safe" appears on every entry that shows a state.
- **Two themes.** Dark by default, light on toggle, following the OS preference
  and remembering an explicit choice locally.

## Known gap

**Live IDL resolution is wired but untested against real chain data.** None of the
entries currently on the testnet deployment carries an IDL pointer — all six
decode with `idl = None` — so the `--storage-gateway` fetch-and-parse path has no
live entry to exercise end-to-end. The content-id **encoder** that path depends on
is pinned by a unit test against the two real Logos Storage identifiers documented
in `lez-registry-core`, and the graceful-degradation branches render correctly;
what is unproven is a real gateway round-trip, because there is no public Logos
Storage gateway documented for this endpoint and no on-chain CID to fetch.

## Licence

MIT OR Apache-2.0.
