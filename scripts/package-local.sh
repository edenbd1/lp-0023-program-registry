#!/usr/bin/env bash
# Package everything a user needs to run the registry tooling on their own
# machine, with nothing hosted: the CLI, the explorer and the indexer, one
# script to start the explorer on localhost, and a checksum for every file.
#
#   scripts/package-local.sh            # -> dist/lez-registry-<os>-<arch>.tar.gz
#
# The public site is a convenience. This archive is the same software, so a
# user who does not want to trust a hosted page can run the identical view
# locally and compare what it shows.
set -euo pipefail
cd "$(dirname "$0")/.."

os=$(uname -s | tr '[:upper:]' '[:lower:]')
arch=$(uname -m)
name="lez-registry-${os}-${arch}"
out="dist/$name"
rm -rf "$out" "dist/$name.tar.gz"
mkdir -p "$out/bin"

for crate in cli explorer indexer; do
  echo "building $crate" >&2
  cargo build --release --manifest-path "$crate/Cargo.toml" >&2
done
cp cli/target/release/lez-registry          "$out/bin/"
cp explorer/target/release/lez-registry-explorer "$out/bin/"
cp indexer/target/release/lez-registry-indexer   "$out/bin/"
cp idl/lez_registry.idl.json "$out/"
cp LICENSE-MIT LICENSE-APACHE "$out/" 2>/dev/null || true

cat > "$out/run.sh" <<'RUN'
#!/usr/bin/env bash
# Start the explorer on this machine and open it. Every value it shows is read
# from the chain; nothing is fetched from any site of ours.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
port="${PORT:-8080}"
echo "explorer: http://127.0.0.1:$port   (Ctrl-C to stop)"
( sleep 2; command -v open >/dev/null && open "http://127.0.0.1:$port" \
  || command -v xdg-open >/dev/null && xdg-open "http://127.0.0.1:$port" ) >/dev/null 2>&1 &
exec "$here/bin/lez-registry-explorer" --port "$port" "$@"
RUN
chmod +x "$out/run.sh"

commit=$(git rev-parse HEAD)
cat > "$out/README.md" <<README
# LEZ program registry — run it yourself

Built from \`$commit\` of https://github.com/edenbd1/lp-0023-program-registry.

    ./run.sh                              # the explorer, on http://127.0.0.1:8080
    ./bin/lez-registry lookup <PROGRAM_ID> # resolve a program with no site at all
    ./bin/lez-registry --help

Check the files are the ones published: \`shasum -a 256 -c SHA256SUMS\`.
The explorer's own page shows the sha256 of the binary it runs; compare it with
\`bin/lez-registry-explorer\` below. For a digest every rebuilder reproduces,
build the pinned image instead: \`docker build -f explorer/Dockerfile .\`.
README

( cd "$out" && find . -type f ! -name SHA256SUMS | sort | xargs shasum -a 256 > SHA256SUMS )
tar -C dist -czf "dist/$name.tar.gz" "$name"
echo "dist/$name.tar.gz"
( cd dist && shasum -a 256 "$name.tar.gz" )
