# Deployments

## Public testnet v0.3 (`https://testnet.lez.logos.co`, LEZ v0.3.0)

**Testnet v0.3 deployment: pending funding.** The registry account, its
ImageID, and the deploy and registration transactions go here once the fee
payer holds LGO. The exact commands are in
[`docs/deploy-v03.md`](docs/deploy-v03.md).

Earlier deployments on the v0.2.4 zone are superseded: that chain is being
replaced by testnet v0.3, its program ids (RISC0 ImageIDs) are not v0.3
program accounts, and registrations there do not carry over. None of their
addresses are used anywhere in this branch.

## Local v0.3.0 standalone sequencer (end to end)

The full lifecycle has been run against a real LEZ v0.3.0 sequencer
(`db66590ab`, standalone, `RISC0_DEV_MODE=0`), every step read back from the
chain. Transcript: [`evidence/e2e-v03-local.txt`](evidence/e2e-v03-local.txt).
Re-run it with `scripts/e2e-v03-local.sh`; account ids differ on every run
because the script generates fresh keys.

What that run shows, in order:

| step | outcome |
|---|---|
| deploy the guest through `wallet program-loader deploy` | header holds the build's ImageID, mutable |
| `initialize`, then the registry registers itself | deployer entry, signed by the registry's own header account |
| rebuild from source, fresh target dir, network denied | same ImageID; marked locally verified; attested by a third key: `Verified` |
| a stranger submits a **deployer** entry for a program they did not deploy | included, charged, reverted: `E9010`, nothing written |
| the program's own header key registers it | deployer entry |
| a rebuild against a claim whose build had the network up | `Unreproducible`, attester named |
| the stranger registers their own entry about it | third-party entry, at its own address |
| the stranger tries to update the deployer's entry | reverted: `E9003` |
| the author updates version, description and IDL pointer | applied |
| a third-party source claim that does not rebuild | `Mismatched`, recorded |
| a mutable program is repointed by its deployer, then `sync_header` | `Stale`, new ImageID recorded |
| `freeze`, then an update | reverted: `E9004` |
| `list`, `search`, `lookup`, SDK e2e tests, resolve example, indexer | all from the chain |

Cycle costs per instruction: [`CYCLES.md`](CYCLES.md).
