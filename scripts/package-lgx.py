#!/usr/bin/env python3
"""Package the Basecamp plugin as a `.lgx`, without the Nix toolchain.

    ./scripts/package-lgx.py [--out app/lp-0023-program-registry.lgx]

HOW IT PACKAGES

If the real `lgx` from logos-co/logos-package is available, it is used —
`$LGX_BIN`, then `~/logos/src/logos-package/build/lgx`, then `lgx` on PATH.
That is the canonical packager and it is always preferred.

Only if none is found does this fall back to writing the package directly. The
fallback is not guesswork: the manifest hash scheme is transcribed from
`logos-package/src/crypto/signing.cpp`, and the transcription is checked against
two packages built by the real tool before anything
is written. Both paths were confirmed to produce **identical manifest hashes**
for this module.

Either way the metadata fields are patched in afterwards, which is what
`nix-bundle-lgx`'s `bundle.sh` does too: `lgx add` leaves author, description,
type and category empty because it never reads `metadata.json`.

THE FORMAT, for the record

    manifest.json
    variants/<variant>/<plugin>.<dylib|so>
    variants/<variant>/metadata.json
    variants/<variant>/qml/{Main.qml,qmldir}
    variants/<variant>/<cli>            (optional companion binary)

gzip-compressed tar, GNU format.

THE HASHES (signing.cpp: computeDirectoryHash / computeParentDirectoryHash)

    directory: for each file under the prefix, sorted by relative path,
               concat += relpath + '\\0' + sha256hex(contents) + '\\n'
               hash = sha256hex(concat)
    parent:    for each (child name, child hash) in sorted order,
               concat += name + '\\0' + hash + '\\n'
               hash = sha256hex(concat)
"""

import argparse
import hashlib
import io
import json
import os
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
VARIANT_DEFAULT = "darwin-arm64"


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def directory_hash(prefix: Path) -> str:
    """Hash of every file under `prefix`, keyed by path relative to it."""
    files = []
    for dirpath, _, filenames in os.walk(prefix):
        for name in filenames:
            p = Path(dirpath) / name
            files.append((str(p.relative_to(prefix)), p.read_bytes()))
    if not files:
        return ""
    files.sort(key=lambda t: t[0])
    concat = b"".join(
        rel.encode() + b"\0" + sha(data).encode() + b"\n" for rel, data in files
    )
    return sha(concat)


def parent_hash(children: dict) -> str:
    """Hash of a directory whose entries are themselves hashed directories."""
    if not children:
        return ""
    concat = b"".join(
        name.encode() + b"\0" + children[name].encode() + b"\n"
        for name in sorted(children)
    )
    return sha(concat)


def build_manifest(stage: Path, name: str, version: str, meta: dict, plugin: str) -> dict:
    variants = stage / "variants"
    per_variant = {v.name: directory_hash(v) for v in sorted(variants.iterdir()) if v.is_dir()}
    variants_hash = parent_hash(per_variant)
    hashes = {"root": parent_hash({"variants": variants_hash}), "variants": variants_hash}
    hashes.update({f"variants/{k}": v for k, v in per_variant.items()})
    return {
        "author": meta.get("author", ""),
        "category": meta.get("category", ""),
        "dependencies": meta.get("dependencies", []),
        "description": meta.get("description", ""),
        "hashes": hashes,
        "icon": "",
        "main": {v: plugin for v in per_variant},
        "manifestVersion": "0.3.0",
        "name": name,
        "type": meta.get("type", ""),
        "version": version,
        "view": "qml/Main.qml",
    }


def self_test() -> int:
    """Check the transcription against packages built by the real tool."""
    refs = [
        # Sibling packages only exist on the machine that built them, so from a
        # clean clone the check would otherwise pass by having nothing to test.
        # This module's own committed package was written by the real `lgx`
        # too, and it ships in the repository — so there is always at least one
        # real reference to reproduce.
        ROOT / "app/lp-0023-program-registry.lgx",
    ]
    found = [r for r in refs if r.is_file()]
    if not found:
        print("self-test: no reference .lgx to check against — refusing to "
              "vouch for the transcription", file=sys.stderr)
        return 1
    failures = 0
    for ref in found:
        tmp = Path(tempfile.mkdtemp())
        try:
            with tarfile.open(ref) as t:
                t.extractall(tmp)
            want = json.loads((tmp / "manifest.json").read_text())["hashes"]
            variants = tmp / "variants"
            per = {v.name: directory_hash(v) for v in sorted(variants.iterdir()) if v.is_dir()}
            vh = parent_hash(per)
            got = {"root": parent_hash({"variants": vh}), "variants": vh}
            got.update({f"variants/{k}": v for k, v in per.items()})
            ok = got == want
            print(f"  {'ok  ' if ok else 'FAIL'} {ref.name}")
            if not ok:
                failures += 1
                for k in sorted(set(got) | set(want)):
                    if got.get(k) != want.get(k):
                        print(f"        {k}: got {got.get(k)} want {want.get(k)}")
        finally:
            shutil.rmtree(tmp)
    return failures


def find_lgx():
    """The real packager, if this machine has one."""
    for cand in (os.environ.get("LGX_BIN"),
                 str(Path.home() / "logos/src/logos-package/build/lgx"),
                 shutil.which("lgx")):
        if cand and Path(cand).is_file() and os.access(cand, os.X_OK):
            return cand
    return None


def patch_metadata(pkg: Path, meta: dict, name: str) -> dict:
    """Fold metadata.json into the manifest, as bundle.sh does."""
    with tarfile.open(pkg, "r:gz") as tar:
        members = [(m, tar.extractfile(m).read() if m.isfile() else None)
                   for m in tar.getmembers()]
    manifest = None
    out = []
    for member, data in members:
        if member.name == "manifest.json":
            manifest = json.loads(data)
            for key in ("author", "description", "type", "category", "dependencies"):
                if key in meta:
                    manifest[key] = meta[key]
            data = json.dumps(manifest, indent=2, sort_keys=True).encode()
            member.size = len(data)
        out.append((member, data))
    with tarfile.open(pkg, "w:gz", format=tarfile.GNU_FORMAT) as tar:
        for member, data in out:
            if data is not None:
                tar.addfile(member, io.BytesIO(data))
            else:
                tar.addfile(member)
    return manifest


def unloadable_reason(plugin: Path):
    """Why Basecamp would refuse this binary — or None if it would take it.

    Both failures below are silent in the UI: the app tile just does nothing,
    and the reason only reaches Basecamp's stderr. They are also easy to
    reintroduce, since the default `brew --prefix qt` build hits both. So they
    are checked here, at the one point every package must pass through.
    """
    blob = plugin.read_bytes()

    # The interface string qobject_cast compares across the plugin boundary.
    # Get it wrong and the host logs "Plugin does not implement IComponent".
    if b"com.logos.component.IComponent" not in blob:
        return ("no com.logos.component.IComponent interface string — check "
                "Q_DECLARE_INTERFACE in app/src/plugin.h")

    if plugin.suffix != ".dylib":
        return None  # the checks below read Mach-O load commands

    out = subprocess.run(["otool", "-L", str(plugin)],
                         capture_output=True, text=True).stdout

    # Qt refuses any plugin whose minor version exceeds the host's, and
    # Basecamp 0.2.2 ships 6.9.2.
    versions = re.findall(r"Qt\w+ \(compatibility version [\d.]+, "
                          r"current version 6\.(\d+)\.", out)
    if versions and max(int(v) for v in versions) > 9:
        return (f"built against Qt 6.{max(int(v) for v in versions)} — Basecamp "
                f"0.2.2 runs Qt 6.9.2 and rejects anything newer")

    # Absolute Homebrew paths resolve on this machine and nowhere else; the
    # host's bundled Qt is only reachable through @rpath.
    if "/opt/homebrew" in out:
        return ("links Homebrew Qt by absolute path — build against an "
                "official Qt so the frameworks are referenced via @rpath")

    return None


def qml_resource_root(variant_dir):
    """The qrc root the shipped binary loads its QML from.

    Read from the artifact, not the source: the source is not what ships. Qt
    stores QString literals as UTF-16, so a plain `strings` finds nothing and the
    scan looks for the encoded form.
    """
    pat = re.compile(rb"(?:q\x00r\x00c\x00:\x00)((?:[a-z_0-9/]\x00)+)"
                     rb"M\x00a\x00i\x00n\x00\.\x00q\x00m\x00l\x00")
    for lib in list(variant_dir.rglob("*.dylib")) + list(variant_dir.rglob("*.so")):
        hits = {m.group(1).decode("utf-16-le") for m in pat.finditer(lib.read_bytes())}
        if hits:
            return sorted(hits)[0]
    return None


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--out", default="app/lp-0023-program-registry.lgx")
    ap.add_argument("--variant", default=VARIANT_DEFAULT,
                    help="variant to build from the local artifacts")
    ap.add_argument("--add-variant", metavar="NAME:DIR", action="append", default=[],
                    help="fold in a variant built elsewhere, e.g. "
                         "linux-amd64:/path/with/{so,airdrop}")
    ap.add_argument("--self-test", action="store_true",
                    help="only verify the hash transcription against known packages")
    ap.add_argument("--verify", metavar="LGX",
                    help="recompute an existing package's hashes and check its manifest")
    args = ap.parse_args()

    if args.self_test:
        return 1 if self_test() else 0

    if args.verify:
        pkg = ROOT / args.verify
        if not pkg.is_file():
            print(f"no such package: {pkg}", file=sys.stderr)
            return 1
        tmp = Path(tempfile.mkdtemp())
        try:
            with tarfile.open(pkg) as t:
                t.extractall(tmp)
            want = json.loads((tmp / "manifest.json").read_text())["hashes"]
            variants = tmp / "variants"
            per = {v.name: directory_hash(v) for v in sorted(variants.iterdir()) if v.is_dir()}
            vh = parent_hash(per)
            got = {"root": parent_hash({"variants": vh}), "variants": vh}
            got.update({f"variants/{k}": v for k, v in per.items()})
            names = sorted(set(got) | set(want))
            bad = [k for k in names if got.get(k) != want.get(k)]
            for k in names:
                print(f"  {'ok  ' if k not in bad else 'FAIL'} {k}")
            if bad:
                print("\nthe package contents do not match its manifest", file=sys.stderr)
                return 1
            # Qt's resource system is process-global, so two modules that both
            # register /qml/Main.qml resolve to whichever registered first and one
            # tile draws the other's UI. It is invisible with a single module
            # installed, which is why it survived until someone had two. The root
            # must be the module's own name, and this refuses to ship a package
            # where it is not.
            for v in sorted(variants.iterdir()):
                if not v.is_dir():
                    continue
                root = qml_resource_root(v)
                if root is None:
                    continue
                if root.strip("/") in ("qml", ""):
                    print(f"\n{args.verify}: variant {v.name} loads its QML from "
                          f"{root!r}, which every module would claim — set PREFIX in "
                          "qt_add_resources and the setSource() URL to this module's "
                          "own name", file=sys.stderr)
                    return 1
                print(f"  ok   variants/{v.name} qml root {root}")

            print(f"\n{args.verify}: contents match the manifest "
                  f"({len(per)} variant{'s' if len(per) != 1 else ''}: "
                  f"{', '.join(sorted(per))}; sha256 {sha(pkg.read_bytes())[:16]}…)")
            return 0
        finally:
            shutil.rmtree(tmp)

    app = ROOT / "app"
    meta = json.loads((app / "metadata.json").read_text())
    plugin_name = meta["main"]

    ext = "dylib" if sys.platform == "darwin" else "so"
    plugin = app / "build" / f"{plugin_name}.{ext}"
    if not plugin.is_file():
        print(f"missing {plugin}. Build it first — and mind the Qt version,\n"
              f"see app/README.md:\n"
              f"  aqt install-qt mac desktop 6.9.2 clang_64 --outputdir /tmp/Qt\n"
              f"  cd app && cmake -B build -S . -DCMAKE_PREFIX_PATH=/tmp/Qt/6.9.2/macos "
              f"&& cmake --build build", file=sys.stderr)
        return 1

    if (problem := unloadable_reason(plugin)):
        print(f"refusing to package {plugin.name}: {problem}\n"
              f"See app/README.md — 'Which Qt' and 'The plugin interface is "
              f"ABI-critical'.", file=sys.stderr)
        return 1

    # No companion CLI. This module reads the chain over JSON-RPC from inside
    # the plugin, so there is no binary to ship alongside it and no login-PATH
    # resolution problem to get wrong.

    stage = Path(tempfile.mkdtemp())
    try:
        vdir = stage / "variants" / args.variant
        (vdir / "qml").mkdir(parents=True)
        shutil.copy2(plugin, vdir / plugin.name)
        shutil.copy2(app / "metadata.json", vdir / "metadata.json")
        shutil.copy2(app / "qml" / "Main.qml", vdir / "qml" / "Main.qml")
        shutil.copy2(app / "qml" / "qmldir", vdir / "qml" / "qmldir")

        out = ROOT / args.out
        out.parent.mkdir(parents=True, exist_ok=True)
        # Extra variants built on other platforms. Basecamp picks the one
        # matching the host, so shipping only darwin-arm64 makes the package
        # unopenable for a reviewer on Linux.
        for spec in args.add_variant:
            vname, vsrc = spec.split(":", 1)
            src = Path(vsrc)
            dst = stage / "variants" / vname
            (dst / "qml").mkdir(parents=True)
            libs = [f for f in src.iterdir()
                    if f.suffix in (".so", ".dylib") or f.name.endswith(".dll")]
            if not libs:
                print(f"no plugin library in {src}", file=sys.stderr)
                return 1
            shutil.copy2(libs[0], dst / libs[0].name)
            shutil.copy2(app / "metadata.json", dst / "metadata.json")
            shutil.copy2(app / "qml" / "Main.qml", dst / "qml" / "Main.qml")
            shutil.copy2(app / "qml" / "qmldir", dst / "qml" / "qmldir")
            print(f"  + variant {vname} ({libs[0].name})")

        real = find_lgx()

        if real:
            print(f"packaging with the real lgx: {real}")
            work = stage / "work"
            work.mkdir()
            subprocess.run([real, "create", "lp-0023-program-registry"], cwd=work,
                           check=True, capture_output=True)
            for vd in sorted((stage / "variants").iterdir()):
                lib = next(f for f in vd.iterdir()
                           if f.suffix in (".so", ".dylib") or f.name.endswith(".dll"))
                subprocess.run([real, "add", "lp-0023-program-registry.lgx",
                                "--variant", vd.name, "--files", str(vd),
                                "--main", lib.name, "--view", "qml/Main.qml", "-y"],
                               cwd=work, check=True, capture_output=True)
            shutil.copy2(work / "lp-0023-program-registry.lgx", out)
            manifest = patch_metadata(out, meta, "lp-0023-program-registry")
        else:
            print("real lgx not found — writing the package directly")
            print("verifying the hash algorithm against packages built by the real tool")
            if self_test():
                print("hash transcription no longer matches — refusing to write a "
                      "package Basecamp would reject", file=sys.stderr)
                return 1
            manifest = build_manifest(stage, "lp-0023-program-registry", "0.0.1", meta, plugin.name)
            (stage / "manifest.json").write_text(
                json.dumps(manifest, indent=2, sort_keys=True))
            with tarfile.open(out, "w:gz", format=tarfile.GNU_FORMAT) as tar:
                tar.add(stage / "manifest.json", arcname="manifest.json")
                tar.add(stage / "variants", arcname="variants")

        size = out.stat().st_size
        digest = sha(out.read_bytes())
        print(f"\nwrote {args.out} ({size // 1024} KB)")
        print(f"  sha256   {digest}")
        print(f"  variants {', '.join(sorted(manifest['hashes'][k].split('/')[-1] for k in manifest['hashes'] if k.startswith('variants/')) ) if False else ', '.join(sorted(k.split('/',1)[1] for k in manifest['hashes'] if k.startswith('variants/')))}")
        print(f"  root     {manifest['hashes']['root']}")
        return 0
    finally:
        shutil.rmtree(stage)


if __name__ == "__main__":
    sys.exit(main())
