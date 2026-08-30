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

```bash
cargo run --release --manifest-path explorer/Cargo.toml
# then open http://127.0.0.1:8080
```

That is the whole install. No database to provision, no migration, no secret, no
account. It comes up pointed at the documented public testnet deployment and the
public sequencer, warms one walk of the registry, and serves.

Anyone who runs that command gets **identical data**, because the data was never
the server's — it belongs to the chain. Point two instances at the same registry
and endpoint and they render the same bytes.

### Options

```
lez-registry-explorer [REGISTRY_HEX] [options]

  --registry <HEX>          registry program id      (default: the testnet deployment below)
  --endpoint <URL>          sequencer JSON-RPC        (default: https://testnet.lez.logos.co)
  --port <N>                listen port              (default: 8080)
  --storage-gateway <URL>   Logos Storage gateway to resolve IDLs (optional; see below)
  --cache-ttl <SECONDS>     in-memory walk cache TTL (default: 10)
  --no-cache                read the chain on every request
```

Defaults, from [`../DEPLOYMENTS.md`](../DEPLOYMENTS.md):

| | |
|---|---|
| registry ProgramId | `f59751e932e856f7565f34e79ae726791794dc68e55f2413bc0375008cc07bf8` |
| global state | `75jzaQqqqf87apGZfr3bewGbHDfCnau49TgohDKc5s1z` |
| endpoint | `https://testnet.lez.logos.co` |

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
- **`/api/programs.json`** — a machine-readable mirror of the list, so the view
  is scriptable and its numbers checkable. Each record carries the derivation
  needed to re-check it without this API.

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
