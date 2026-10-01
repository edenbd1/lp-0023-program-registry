# LP-0023 criteria, and where the v0.3 port stands

Against `prizes/LP-0023.md` on `logos-co/lambda-prize` (open, $2,000, first
submission to meet every criterion). Status is **met**, **partial** or
**open**; nothing is marked met that has not been run. "Local" means against a
real LEZ v0.3.0 standalone sequencer (`evidence/e2e-v03-local.txt`); the public
testnet v0.3 deployment is **pending funding** ([deploy-v03.md](deploy-v03.md)).

## Functionality

| criterion | status | evidence |
|---|---|---|
| On-chain registry built with SPEL, with a published IDL; only metadata and pointers on chain | **partial** | `program/` on `lee_core` v0.3.0, `idl/lez_registry.idl.json` in SPEL's format with a drift test. Not built with SPEL, because SPEL has no v0.3 release (README, "Why the IDL is hand-maintained") |
| Register with name, version, author, description, IDL pointer, code pointer, registration timestamp; lookup without a centralised indexer | **met (local)** | `register` takes all of them in one transaction; the timestamp is bounded on chain by validity windows; lookup is one PDA derivation and one read |
| Deployer vs third-party entries; deployer entry signed by the deploying account; clients tell them apart without an off-chain service; unauthorised deployer attempt rejected in the demo | **met (local)** | the deployer entry must be signed by the program's header account (the loader's deploy authority); `publisher` is on chain; the transcript shows a stranger's deployer entry reverted with `E9010` |
| Only the original author may update (version, IDL pointer, description) | **met (local)** | `require_writable_by`; transcript shows a stranger's update reverted with `E9003` |
| IDL stored on Logos Storage | **partial** | the entry stores a Logos Storage content id and `tools/storage-upload.sh` uploads; the local run uses a previously uploaded id, it does not upload |
| Discovery: by program id, full enumeration, search by name and author | **met (local)** | `lookup`, `list` (sequence directory walk), `search`; `indexer/` optional |
| Source verification: pointer, commit, build config; local rebuild marks "locally verified" | **met (local)** | `claim_source` / `register --repo …`; `verify --mark` compares a rebuild's ImageID with the header on chain; transcript rebuilds with the network denied and records `Verified` |
| Testnet 0.3 end to end | **open** | pending funding; the identical sequence runs locally |

## Usability

| criterion | status | evidence |
|---|---|---|
| Module/SDK to register, resolve and search | **met (local)** | `sdk/`: resolve, walk, build, BIP-340 sign, submit; exercised by the CLI and `sdk/tests/e2e.rs` against the local chain |
| CLI covering register, update, lookup, search, verification | **met (local)** | `cli/` |

## Reliability

| criterion | status | evidence |
|---|---|---|
| Graceful degradation on an unresolvable or unparseable IDL | **met** | `lez-registry idl`, the explorer and the Basecamp module show the entry with the IDL marked unavailable |
| No mandatory centralised services | **met** | sequencer only; indexer and explorer optional and self-hostable |

## Performance

| criterion | status | evidence |
|---|---|---|
| Cycle counts for register and update | **met** | [CYCLES.md](../CYCLES.md): register 105,403, update 55,611 cycles (plan plus applies), `tools/cycles-v03` |

## Supportability

| criterion | status | evidence |
|---|---|---|
| Deployed and tested on testnet 0.3 | **open** | pending funding |
| Builds and runs end to end on macOS (Apple Silicon) and Linux (x86_64) | **partial** | run end to end on macOS; the Linux path is the CI e2e job, not yet run on this branch |
| Logos Core module built with `logos-module-builder` (`mkLogosModule`) | **partial** | `app/` builds with CMake against Qt 6.9.2; the `mkLogosModule` path is not yet done |
| IDL for the registry; the registry registered in itself | **partial** | registered in itself by its own header key (local); IDL hand-maintained, not SPEL-generated (see above) |
| E2E tests against a real standalone sequencer in CI, green on the default branch | **partial** | `scripts/e2e-v03-local.sh` is the CI e2e job; not yet run on GitHub for this branch |
| README: setup, deployed program id, CLI and module usage | **partial** | everything except the program id, which waits for the deployment |
| SDK worked example: program id to metadata and IDL | **met** | `sdk/examples/resolve.rs`, README |

## Adoption

All open, and none of it is code: 5 independent third-party Logos UI modules
using the SDK; 50 programs registered on the official zone by at least 20
independent authors; 20 entries with independently rebuildable source claims;
registrations over at least two months with at least 10 a month; 20 Discord and
20 X testimonials. The two-month window starts with the testnet v0.3
deployment.
