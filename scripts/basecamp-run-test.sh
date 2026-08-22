#!/usr/bin/env bash
# Check .lgx packages for the two failures a load test cannot see. Neither shows
# up in QPluginLoader; both were found by clicking a tile.
#
#   1. The host dies on the click. A module's first QNetworkAccessManager request
#      triggers Qt's macOS system-proxy lookup, which asks PCRE2 to JIT-compile a
#      regex; Basecamp runs hardened without com.apple.security.cs.allow-jit, so
#      the JIT allocation traps and takes the process with it. Silent: no dialog,
#      no log line, the window simply gone.
#
#   2. Two modules render each other. Qt's resource system is process-global, so
#      two packages that both register qrc:/qml/Main.qml resolve to whichever
#      registered first — one tile draws the other's UI over its own live data.
#
# The second is why this takes a list rather than one package: with a single
# module installed there is nothing to collide with, and a per-repository test
# would pass on a repository that is broken for every real user.
#
#   scripts/basecamp-run-test.sh <install-name>:<path.lgx> ...
#
# Exit 0 all good, 1 a check failed, 2 the environment is not usable.

set -uo pipefail

APP=/Applications/LogosBasecamp.app
PLUGINS="$HOME/Library/Application Support/Logos/LogosBasecamp/plugins"
[ $# -ge 2 ] || { echo "usage: $0 <install-name>:<path.lgx> ... (at least two)"; exit 2; }
command -v python3 >/dev/null || { echo "python3 is required"; exit 2; }

work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
fail=0

# --- 1. Resource roots, read out of the shipped binaries -------------------
# Source is not the artifact. Qt stores QString literals as UTF-16, so this
# scans for the encoded form rather than running `strings`, which finds nothing.
echo "-- QML resource root, per package --"
roots=$(python3 - "$@" <<'PY'
import pathlib, re, sys, tarfile, tempfile
pat = re.compile(rb"(?:q\x00r\x00c\x00:\x00)((?:[a-z_0-9/]\x00)+)M\x00a\x00i\x00n\x00\.\x00q\x00m\x00l\x00")
for spec in sys.argv[1:]:
    name, lgx = spec.split(":", 1)
    with tempfile.TemporaryDirectory() as td:
        tarfile.open(lgx).extractall(td)
        dy = next(pathlib.Path(td).rglob("*.dylib"), None) or next(pathlib.Path(td).rglob("*.so"))
        hits = {m.group(1).decode("utf-16-le") for m in pat.finditer(dy.read_bytes())}
    print("%s\t%s" % (name, ",".join(sorted(hits)) or "?"))
PY
) || { echo "could not read the packages"; exit 2; }
printf '%s\n' "$roots" | while IFS=$'\t' read -r n r; do printf '   %-32s %s\n' "$n" "$r"; done
# A package with no qrc QML at all reports "?", and two of those are not a
# collision — they have nothing to collide over. Excluding them stops the
# check inventing a failure for modules that never load QML from a resource.
dupes=$(printf '%s\n' "$roots" | cut -f2 | grep -v '^?$' | sort | uniq -d)
if [ -n "$dupes" ]; then
    echo "   FAIL two packages share a resource root: $dupes"
    echo "        set PREFIX in qt_add_resources and the setSource() URL to the module's own name"
    fail=1
else
    echo "   ok   every package owns a distinct resource root"
fi

# --- 2. The host survives every click --------------------------------------
[ -d "$APP" ] || { echo "-- skipping the click test: Basecamp is not installed --"; exit $fail; }
echo "-- opening each tile in Basecamp --"
pkill -f LogosBasecamp 2>/dev/null; sleep 2
names=()
for spec in "$@"; do
    name=${spec%%:*}; lgx=${spec#*:}
    rm -rf "$work/x"; mkdir -p "$work/x"
    tar -xzf "$lgx" -C "$work/x" || { echo "cannot unpack $lgx"; exit 2; }
    variant=$(ls "$work/x/variants" | head -1)
    rm -rf "$PLUGINS/$name"; mkdir -p "$PLUGINS/$name"
    cp -R "$work/x/variants/$variant/." "$PLUGINS/$name/"
    cp "$work/x/manifest.json" "$PLUGINS/$name/"
    printf '%s' "$variant" > "$PLUGINS/$name/variant"
    names+=("$name")
done
open -a "$APP"; sleep 14
for name in "${names[@]}"; do
    osascript -e "tell application \"System Events\" to tell process \"LogosBasecamp.bin\" \
        to click button \"$name\" of window 1" >/dev/null 2>&1
    sleep 8
    if pgrep -f LogosBasecamp.bin >/dev/null; then
        echo "   ok   $name — the host is still running"
    else
        echo "   FAIL $name — the host died on the click"
        fail=1
        open -a "$APP"; sleep 14
    fi
done
pkill -f LogosBasecamp 2>/dev/null

# What this does NOT check: that the surface on screen is the right one. Qt does
# not expose QQuickWidget content through the accessibility tree, so there is
# nothing to read back, and asserting it from a screenshot needs a human. The
# resource-root check above is the machine-checkable half of that question.
[ $fail -eq 0 ] && echo "All packages own distinct resource roots and survive being opened."
exit $fail
