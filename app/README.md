# Basecamp module

A registry browser for LogosBasecamp. Type a program id; the panel **derives**
where that program's entry lives and reads it from a sequencer.

The derived address is shown before the answer arrives, and that is the whole
argument on screen: nothing was asked of an index that could omit a program,
serve a stale answer, or go down separately from the chain. Two SHA-256 hashes,
computed in the panel.

It holds no keys and signs nothing.

## What it shows that a tick would not

`Verified` is not one colour here. A build whose only voucher is its own author
renders differently from one a third party reproduced, because those are
different claims and painting both green hides the difference that matters most.
`Unreproducible` says why: the claimed build had network access, so a match
would prove nothing about the source.

## Which registry, which chain

LEZ v0.3: the panel reads the registry program's shard of each account
(`getAccount` → `data.shards[<registry>]`), and a program id is its header
account, typed as base58 or as 64 hex characters. The registry is taken from
`PROVENANCE_REGISTRY` and the sequencer from `LEZ_RPC` (default
`https://testnet.lez.logos.co`); nothing is compiled in, because the testnet
v0.3 deployment is pending funding and a built-in id from an older chain would
resolve every program to "not registered".

## Build

```bash
cmake -S . -B build -DCMAKE_PREFIX_PATH=/tmp/Qt/6.9.2/macos -DCMAKE_BUILD_TYPE=Release
cmake --build build -j4
python3 ../scripts/package-lgx.py
```

Qt **6.9.2** specifically. Basecamp 0.2.2 bundles that version and the bundled
version is a *ceiling*: a plugin built against Homebrew's 6.11 is rejected at
load. `QtConcurrent` is deliberately not linked — Basecamp does not bundle it,
and linking it makes the plugin unloadable.

Official builds reference frameworks as `@rpath/…`, which resolves against
Basecamp's own Qt. Homebrew builds hardcode `/opt/homebrew/opt/qtbase/lib/…` and
work nowhere but the machine that built them.

## Load-verified, not just compiled

A plugin that compiles and does not load is the failure that matters, and it is
invisible without checking. Both the build output and the binary **extracted from
the packaged `.lgx`** were loaded through `QPluginLoader` and cast to
`IComponent`, exactly as Basecamp does:

```
IID           : com.logos.component.IComponent
className     : RegistryPlugin
instance      : loaded
IComponent    : cast succeeded
```

`IComponent`'s vtable is exactly three entries — destructor, `createWidget`,
`destroyWidget`. An extra virtual shifts every later slot and the host calls the
wrong function through a correctly cast pointer, so `src/plugin.h` mirrors it
slot for slot rather than approximating it.

The packaged manifest carries `type: "ui"`. `lgx add` leaves that field empty and
an empty type makes the module invisible in Basecamp, so `package-lgx.py` patches
the metadata in afterwards.

## Two things that only show up with the host actually running

**A module that loads can still kill Basecamp.** `QPluginLoader::load()`
returning true proves the binary is ABI-compatible and exports the interface. It
proves nothing about using it. Clicking the tile killed the host with `SIGTRAP` —
no dialog, no log line, the window simply gone:

```
libsystem_pthread.dylib  pthread_jit_write_protect_np
libpcre2-16.0.dylib      sljit_malloc_exec / pcre2_jit_compile_16
QtCore                   QRegularExpressionPrivate::compilePattern()
QtNetwork                macQueryInternal(QNetworkProxyQuery const&)
QtNetwork                QNetworkProxyFactory::systemProxyForQuery(...)
```

The first `QNetworkAccessManager` request triggers Qt's macOS system-proxy
lookup, which builds a `QRegularExpression`, which asks PCRE2 to JIT-compile it.
Basecamp runs under the hardened runtime *without*
`com.apple.security.cs.allow-jit`, so the JIT allocation traps. A module cannot
add an entitlement to somebody else's signed binary, so it declines the lookup
instead — two lines in the bridge constructor, before any request:

```cpp
QNetworkProxyFactory::setUseSystemConfiguration(false);
m_net.setProxy(QNetworkProxy::NoProxy);
```

**And two modules that both register `qrc:/qml/Main.qml` render each other.** Qt's
resource system is process-global, so whichever registers first wins for both:
with two of these installed, one tile showed the other module's UI over its own
live data. The resource prefix is now this module's own name, which cannot
collide. Verified with five modules installed at once, each opened in turn.

Neither failure appears in a load test. Both appear on the first click.
