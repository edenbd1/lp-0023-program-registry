# Deploying Provenance on public testnet v0.3

The exact commands to put the registry on the Logos LEZ zone of testnet v0.3
(`https://testnet.lez.logos.co`, LEZ v0.3.0) and to re-drive the evidence the
local end-to-end run produced. Nothing in this repository runs these for you:
they spend LGO and create the registry's permanent program account.

Every step was first run against a local v0.3.0 standalone sequencer; the
transcript is [`evidence/e2e-v03-local.txt`](../evidence/e2e-v03-local.txt) and
`scripts/e2e-v03-local.sh` is the same sequence end to end.

## 0. What you need

```bash
# LEZ v0.3.0, for the wallet (program-loader deploy).
git clone https://github.com/logos-blockchain/logos-execution-zone _external/lez
git -C _external/lez checkout db66590ab821a4e142c211017a3866d007f6fa77   # tag v0.3.0
(cd _external/lez && cargo build --release -p wallet)
WALLET=$PWD/_external/lez/target/release/wallet
# macOS only: the wallet links the CommandLineTools Python framework.
export DYLD_FALLBACK_FRAMEWORK_PATH=/Library/Developer/CommandLineTools/Library/Frameworks

# This repository's CLI.
cargo build --release --manifest-path cli/Cargo.toml
REG_CLI=$PWD/cli/target/release/lez-registry

export LEZ_RPC=https://testnet.lez.logos.co
export RISC0_DEV_MODE=0
```

The guest to deploy is the **sealed** build, so that anyone can rebuild it to
the same ImageID with the network off:

```bash
./scripts/build-sealed.sh                 # prints "sealed ImageID: <hex>"
ARTIFACT=program/target/riscv32im-risc0-zkvm-elf/docker/lez_registry.bin
cp "$ARTIFACT" artifacts/programs/lez_registry.bin
$REG_CLI image-id artifacts/programs/lez_registry.bin   # same hex; also prints the segment count
```

The committed `artifacts/programs/lez_registry.bin` is that build: the ELF
from the `r0.1.91.1` builder with the network disabled, wrapped with the
v1compat kernel exactly as `cargo risczero build` does after its Docker step
(on this machine that last step hung after the ELF was exported, so the wrap
was done separately). ImageID
`dd12f4a8e3c7218d85cdf6c0625014b352e65be3ff447b911766476671cd6266`, sha256
`65cf1994920c399764af4238958f221ce2093c895d56120ee297d0922cf986dd`.

## 1. Wallet and keys

```bash
export LEE_WALLET_HOME_DIR=$HOME/.provenance-wallet
mkdir -p "$LEE_WALLET_HOME_DIR" "$HOME/.provenance"
cat > "$LEE_WALLET_HOME_DIR/wallet_config.json" <<EOF
{"sequencers":[{"sequencer_addr":"$LEZ_RPC"}],"seq_poll_timeout":"30s",
 "seq_tx_poll_max_blocks":15,"seq_poll_max_retries":10,
 "seq_block_poll_max_amount":100,"calibration_limit":100}
EOF
$WALLET check-health                      # first run asks for a password and prints a recovery phrase

# The registry's header key IS its deploy authority: whoever holds it can
# repoint the program and sign the registry's own deployer entry. Back it up.
$REG_CLI keygen --out "$HOME/.provenance/registry.key"
$REG_CLI keygen --out "$HOME/.provenance/payer.key"
$REG_CLI keygen --out "$HOME/.provenance/attester.key"   # an independent rebuilder
REG=$($REG_CLI account-id --key "$HOME/.provenance/registry.key")
PAYER=$($REG_CLI account-id --key "$HOME/.provenance/payer.key")
$WALLET account import public --private-key "$(cat "$HOME/.provenance/registry.key")"
$WALLET account import public --private-key "$(cat "$HOME/.provenance/payer.key")"
echo "registry program account: $REG"
echo "fee payer:                $PAYER"
```

## 2. Fund the payer with LGO

There is no LEZ faucet. LGO reaches a LEZ account from Bedrock:

1. Get testnet LGO on Bedrock from the faucet,
   <https://testnet.blockchain.logos.co/web/faucet/>, into a Bedrock wallet key
   you control (your Bedrock node's wallet).
2. Bridge it into the zone with a **ChannelDeposit** on the LEZ channel
   (`0x0101…01`, 32 bytes of `0x01`), whose `metadata` is
   `borsh(DepositMetadata { recipient_id: <PAYER as 32 bytes> })`, submitted to
   your Bedrock node's `POST /channel/deposit` with the faucet note as input.
   The sequencer credits the deposit to `$PAYER` once Bedrock finalises it.
3. Check it arrived:

```bash
$REG_CLI balance "$PAYER"
```

How much: the local run's fees are in the transcript (`fee …` after each
transaction). Deploying is the expensive part, one transaction per 96 KiB
segment plus the header; budget the deploy at what the local run's payer spent
on it, plus about a million per registry transaction at the base fee, and keep
headroom because the base fee moves with load.

## 3. Deploy the registry

```bash
N=$($REG_CLI image-id artifacts/programs/lez_registry.bin 2>&1 >/dev/null \
      | sed -n 's/.*: \([0-9]*\) loader segment.*/\1/p')
SEGS=(); for _ in $(seq 1 "$N"); do SEGS+=("Public/$($REG_CLI keygen | sed -n 1p)"); done
$WALLET program-loader deploy --elf artifacts/programs/lez_registry.bin \
  --header "Public/$REG" --segments "${SEGS[@]}" --payer "Public/$PAYER"

$REG_CLI program-header "$REG"            # must print the sealed ImageID
export PROVENANCE_REGISTRY=$REG
```

The header is created **mutable**, so a fix can still be shipped. Freezing it
later (`$WALLET program-loader update-header --header Public/$REG
--first-segment <first segment> --immutable --payer Public/$PAYER`) makes every
verdict about it permanent; `sync-header` then records the frozen flag.

## 4. Initialise and register the registry in itself

```bash
$REG_CLI init --payer "$HOME/.provenance/payer.key"

./scripts/verify-source.sh                # refuses if the tree differs from HEAD; prints the claim fields
COMMIT=$(git rev-parse HEAD)
SOURCE_DIGEST=$(git ls-files -z src program tests Cargo.toml Cargo.lock \
  | sort -z | xargs -0 shasum -a 256 | shasum -a 256 | cut -d' ' -f1)
BUILDER_DIGEST=<"builder digest" line from verify-source.sh>

$REG_CLI register "$REG" --name provenance --version 0.3.0 \
  --description "The LEZ program registry, registered in itself" \
  --tag registry --tag verified-builds \
  --idl <z… content id of idl/lez_registry.idl.json on Logos Storage> \
  --repo https://github.com/edenbd1/lp-0023-program-registry --commit "$COMMIT" \
  --source-digest "$SOURCE_DIGEST" --image-digest "$BUILDER_DIGEST" \
  --toolchain "risc0-guest-builder r0.1.91.1" --offline \
  --key "$HOME/.provenance/registry.key" --payer "$HOME/.provenance/payer.key"
```

Upload the IDL first with `tools/storage-upload.sh idl/lez_registry.idl.json`
(prints the `z…` content id). The registration timestamp is taken from the
clock account and bounded on chain by the transaction's validity window.

## 5. Re-drive the evidence

```bash
# A rebuild from a clean clone, network off, compared with the header on chain.
./scripts/build-sealed.sh
$REG_CLI verify program/target/riscv32im-risc0-zkvm-elf/docker/lez_registry.bin --program "$REG" --mark

# The independent attestation: anyone may record it; the attester is named.
$REG_CLI record-rebuild "$REG" --file program/target/riscv32im-risc0-zkvm-elf/docker/lez_registry.bin \
  --key "$HOME/.provenance/attester.key" --payer "$HOME/.provenance/payer.key"

# The unauthorised deployer entry, refused on chain (any program you did not deploy).
$REG_CLI register <SOMEONE_ELSES_PROGRAM> --name x --version 1 \
  --key "$HOME/.provenance/attester.key" --payer "$HOME/.provenance/payer.key" --submit-anyway

# Read back, from the chain and from the SDK.
$REG_CLI lookup "$REG"
$REG_CLI list
LEZ_RPC=$LEZ_RPC PROVENANCE_REGISTRY=$REG \
  cargo test --release --manifest-path sdk/Cargo.toml --test e2e -- --ignored
cargo run --release --manifest-path sdk/Cargo.toml --example resolve -- "$REG" "$REG"
```

Then record the deployment where clients read it:

- `DEPLOYMENTS.md`: the registry account, the ImageID, the deploy and register
  transaction hashes and blocks;
- `README.md`: the program id;
- `.github/workflows/ci.yml`: set `PROVENANCE_REGISTRY` for the read-only job
  against the public zone;
- the explorer: `--registry $REG` (or `PROVENANCE_REGISTRY`) in its Dockerfile;
- the Basecamp module: `PROVENANCE_REGISTRY` in its launch environment.
