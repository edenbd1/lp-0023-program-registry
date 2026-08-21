# Deployments

Public LEZ testnet, `https://testnet.lez.logos.co`.

## The registry, and the registry's own entry in it

| | |
|---|---|
| ProgramId (RISC0 ImageID) | `fabf1f340a592547c0d9bf84153e96839a833f583a03c60a9f3affbbd2f3ef45` |
| deployment digest | `d5ce586aa075ae0b7f87c6a0fc3db08b9bd500181b259a0eb4e3b765d7827b96` |
| deploy transaction | [`d5ce586a…d7827b96`](https://explorer.testnet.lez.logos.co/transaction/d5ce586aa075ae0b7f87c6a0fc3db08b9bd500181b259a0eb4e3b765d7827b96) · block 17358 |
| `initialize` | [`e3eaedd0…befd45ee`](https://explorer.testnet.lez.logos.co/transaction/e3eaedd021fa0f49a58867c69cd15fe455dc3c7b5d02ffe4d7c9f2c6befd45ee) · block 17359 |
| `register` (itself) | [`5dcae30b…608d37db`](https://explorer.testnet.lez.logos.co/transaction/5dcae30b296b88232a83973d62a329f8a17784094dd221484c129fa4608d37db) · block 17387 |
| global state | `CCgowxA9ojz3DWUvDwkeyiJ4GQFdCQuUs6RcEsJ86e13` |
| canonical entry | `nTnLMBYdEuxNZXzgFEV9XDn6FFc85DP6MBTsHtb3e2P` |

The deploy transaction hash and the deployment digest are the same 64 characters,
and that is not a coincidence to be admired — it was **computed locally before the
deploy was sent** and then matched by the chain. That is the check
`lez-registry deployment-digest` performs, and it is why the tool can tell you the
file you hold is the file that was deployed without asking an explorer to vouch
for it.

The ImageID is a different number, `fabf1f34…`. Both are printed above precisely
because a registry that blurred them would resolve nothing.

## Read the state rather than the receipt

```bash
export LEE_WALLET_HOME_DIR=~/.lez-wallet
spel --idl idl/lez_registry.idl.json \
  inspect CCgowxA9ojz3DWUvDwkeyiJ4GQFdCQuUs6RcEsJ86e13 --type GlobalAccount
spel --idl idl/lez_registry.idl.json \
  inspect nTnLMBYdEuxNZXzgFEV9XDn6FFc85DP6MBTsHtb3e2P --type EntryAccount
```

Global reads `total_entries: 1, total_verified: 0` — one program registered, none
yet rebuilt, which is the honest state and not a placeholder. The entry decodes to
the ImageID above, `lez-program-registry`, `0.1.0`, publisher `Canonical`,
verification `Unclaimed`.

`total_verified` stays 0 until someone other than us rebuilds the source offline
and the ImageID matches. Printing anything else would be the exact failure this
registry exists to avoid.

## An earlier program is still on chain

`f6a765c2a316da6344de3cccfb39868732a4102f93ff9f0cc8d55be2d7c9e577`, deploy
[`447263aa…2d9cb604`](https://explorer.testnet.lez.logos.co/transaction/447263aa8e01b2d3b5cbb4c603b61194d5909b0158f3901e88d282202d9cb604)
at block 17342, is the build from before `SourceClaim` dropped its redundant
`artifact` field. It is not a stale link: it is a different program, still
deployed, still callable, with its own global account and its own entries.

That is the version-binding argument made concrete rather than asserted. Changing
one field of one struct produced a different ImageID, hence a different address,
hence an entry that starts `Unclaimed` — with no state machine involved and no way
for the new build to inherit anything the old one had earned.

## Reproduce

```bash
cargo risczero build --manifest-path program/Cargo.toml
spel program-id artifacts/programs/lez_registry.bin        # the ImageID
cargo run --manifest-path cli/Cargo.toml --release -- \
  deployment-digest artifacts/programs/lez_registry.bin    # the deploy tx hash
```

Run from the repository root: the Docker build context is taken from there, and
the guest depends on `lez-registry-core` by path. Needs Docker (the guest builder
image is `linux/amd64` and runs under emulation on Apple silicon),
`cargo risczero` 3.0.5, and `spel`.
