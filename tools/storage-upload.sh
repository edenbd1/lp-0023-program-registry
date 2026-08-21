#!/usr/bin/env bash
# Build and run the Logos Storage uploader.
#
# STORAGE_SRC must point at a built logos-storage-nim checkout: the library is
# not published as a binary anywhere, so it has to be built from source once.
set -euo pipefail
cd "$(dirname "$0")/.."

STORAGE_SRC="${STORAGE_SRC:-$PWD/_external/logos-storage-nim}"
[ -f "$STORAGE_SRC/build/libstorage.dylib" ] || [ -f "$STORAGE_SRC/build/libstorage.so" ] || {
  echo "no built libstorage under $STORAGE_SRC/build — set STORAGE_SRC" >&2
  exit 1
}
[ $# -ge 1 ] || { echo "usage: tools/storage-upload.sh <file>" >&2; exit 2; }

BIN="${TMPDIR:-/tmp}/lp0023_storage_upload"
cc -o "$BIN" tools/storage-upload.c \
   -I"$STORAGE_SRC/library" -L"$STORAGE_SRC/build" \
   -lstorage -Wl,-rpath,"$STORAGE_SRC/build"

# The node writes a repo into the working directory, so give it one of its own
# rather than scattering a datastore through the checkout.
RUN="${TMPDIR:-/tmp}/lp0023-storage-run"
mkdir -p "$RUN"
FILE=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
# The node writes its own logs to stdout, so the CID is emitted with a marker
# and everything else is dropped. `--raw` keeps the full transcript instead.
if [ "${2:-}" = "--raw" ]; then
  ( cd "$RUN" && "$BIN" "$FILE" )
else
  ( cd "$RUN" && "$BIN" "$FILE" ) | sed -n 's/^CID //p'
fi
