# Publishing the module to a catalog

The Basecamp module is published through a fork of
[`logos-co/logos-modules-release-base`](https://github.com/logos-co/logos-modules-release-base),
whose workflows call
[`logos-co/logos-modules-release-action`](https://github.com/logos-co/logos-modules-release-action)
(`release.yml@v1`) to build the module's `lgx-portable` for each variant, verify
it, cut a `<module>-v<version>` release and rebuild the catalog's `index.json`.

| module | path in this repository | type |
|---|---|---|
| `lez_program_registry` (Provenance) | `app` | ui (built with `logos-module-builder`, `mkLogosModule`) |

The fork is prepared locally in `../lp-0023-catalog`: the base
repository's history, plus the commits that configure it. It has not been
pushed and the fork does not exist yet.

## What the fork changes

- `logos-repo.json`: the catalog's identity and `indexUrl` pointing at the
  fork's `index` release. It passes the release tool's own check
  (`logos-modules-release-tool` `index.py validate-repo`, the same check the
  fork's `validate-repo.yml` runs on every push). No `includesUrl`: the module
  depends on no other module, so the catalog draws from no other catalog.
- `submodules/lp-0023-program-registry`: this repository, branch `v0.3`, as one
  submodule, pinned at the commit to publish.
- `modules.txt`: the module's path inside it, `submodules/lp-0023-program-registry/app`.
  Upstream `release-all.yml` treats each submodule root as one module; here
  the module is under `app/`, so the discover step reads `modules.txt` when it
  exists and falls back to `.gitmodules` otherwise.
- `.github/workflows/release-lez_program_registry.yml`: generated from the
  base's template with that module path (the action accepts a `module_path`
  below a submodule boundary).
- `_release-module.yml`: `variants: "darwin-arm64,linux-amd64"`, the two
  platforms the module is built and tested on (the CI job "Basecamp module"
  builds `./app#lgx-portable` on `macos-14` and `ubuntu-24.04`). Signing stays
  `none`, matching the empty `trustedSigners`.

## The catalog URL

Add this in Basecamp (Settings, Package repositories) or with `lgpd repo add`:

```
https://raw.githubusercontent.com/edenbd1/lp-0023-provenance-catalog/main/logos-repo.json
```

Its index will be served from
`https://github.com/edenbd1/lp-0023-provenance-catalog/releases/download/index/index.json`.

## Steps, in order

Nothing below has been run. Each step publishes something; run them only
when ready.

1. This repository must be public, and the commit the submodule pins must be
   on GitHub (the release workflow clones the submodule with the catalog's
   token, which cannot read a private repository it does not own):

   ```bash
   cd ../lp-0023-program-registry
   gh repo edit edenbd1/lp-0023-program-registry --visibility public --accept-visibility-change-consequences
   git push origin v0.3
   ```

2. Create the fork under the catalog's name, without cloning it:

   ```bash
   gh repo fork logos-co/logos-modules-release-base --fork-name lp-0023-provenance-catalog --clone=false
   ```

3. Point the prepared repository at the fork and push it. The local `main` is
   the base's history plus the configuration commits, so it fast-forwards the
   fork's `main`:

   ```bash
   cd ../lp-0023-catalog
   git remote set-url origin https://github.com/edenbd1/lp-0023-provenance-catalog.git
   git submodule update --init
   git -C submodules/lp-0023-program-registry log --oneline -1   # the pinned commit
   git push origin main
   ```

   Forks have Actions disabled until they are enabled once:

   ```bash
   gh api -X PUT repos/edenbd1/lp-0023-provenance-catalog/actions/permissions -F enabled=true -f allowed_actions=all
   ```

4. Release the module, then check the index:

   ```bash
   ./scripts/catalog.sh release lez_program_registry --watch
   ./scripts/catalog.sh status
   gh release list -R edenbd1/lp-0023-provenance-catalog
   curl -sL https://github.com/edenbd1/lp-0023-provenance-catalog/releases/download/index/index.json | python3 -m json.tool | head -40
   ```

   `./scripts/catalog.sh release-all --watch` does the same through the
   umbrella workflow, which reads `modules.txt`.

5. Install from it on a clean Basecamp (never the default user directory):

   ```bash
   ~/Applications/LogosBasecamp-0.3.0.app/Contents/MacOS/LogosBasecamp --user-dir /tmp/provenance-catalog-check
   ```

   Settings, Package repositories, add the URL above; then install
   **Provenance**. On Linux, the same with the Basecamp 0.3.0 AppImage and
   `--user-dir`.

## A new version

Bump `version` in `app/metadata.json` (and `app/module.json`, which mirrors
it), push it, then in the catalog move the submodule pointer and release:

```bash
cd ../lp-0023-catalog
git -C submodules/lp-0023-program-registry fetch && git -C submodules/lp-0023-program-registry checkout <commit>
git commit -am "lez_program_registry <version>" && git push
./scripts/catalog.sh release lez_program_registry --watch
```

The first release after the testnet 0.3 deployment should set the panel's
default registry account to the deployed program id, so a fresh install
resolves entries with no setting or environment variable.
