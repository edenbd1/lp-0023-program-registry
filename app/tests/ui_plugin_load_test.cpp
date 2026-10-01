// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Does this module actually load the way Basecamp loads a `ui` plugin?
//
// The RFP asks for a mini-app in Basecamp. Shipping a `.lgx` is not that
// claim: Basecamp's PluginLoader has to accept the binary, find the
// interface it compares against, read the metadata, and get a widget back
// through the vtable. When any of those fails it logs one line on stderr, from
// a terminal launch, which a reviewer clicking a tile never sees. The tile is
// simply inert, and nothing says why.
//
// So this drives the same steps, in order, and asserts on each:
//
//   1. The binary loads with every symbol bound, on its own. This module is
//      self-contained on purpose, it names `LogosAPI` only as an opaque
//      pointer type, so it has no undefined host symbols and `RTLD_NOW`
//      succeeds without the host's runtime in the process. A plugin that leans
//      on the host resolves those at load time instead; both designs are
//      legitimate, and the one here is asserted rather than assumed, because
//      "it loaded" means something different in each.
//
//   2. QPluginLoader ACCEPTS it, and REFUSES something that is not a plugin.
//      The refusal is the control: a loader that accepts everything would pass
//      step 2 on a JSON file, and then the acceptance above says nothing. Both
//      directions are asserted for that reason.
//
//   3. The IID it reports is `com.logos.component.IComponent`, which is what
//      the host compares against. A private IID makes `qobject_cast<IComponent*>`
//      return null and Basecamp logs "Plugin does not implement IComponent".
//
//   4. The embedded metadata is this plugin's own: `type` is `ui` (a `core`
//      type is installed into the modules directory, where nothing gives it a
//      window), and `main` names the file that was actually built.
//
//   5. `qobject_cast<IComponent*>` succeeds across the plugin boundary and the
//      vtable is the one the host expects, checked by calling through it:
//      `createWidget(nullptr)` must return a widget and `destroyWidget` must
//      take it back. A null api pointer is the point, the GUI has to come up
//      and say it has no runtime rather than crash the host, because that is
//      what it is handed when anything above it went wrong.
//
// Not asserted: that Basecamp draws it. Nothing headless can assert that.
//
// Run:  ui_plugin_load_test <plugin.dylib|.so> <expected-main-name>
// with QT_QPA_PLATFORM=offscreen where there is no display.

#include <QApplication>
#include <QJsonArray>
#include <QJsonObject>
#include <QPluginLoader>
#include <QString>
#include <QTemporaryFile>
#include <QWidget>

#include <cstdio>
#include <dlfcn.h>

// Basecamp's IComponent, declared exactly as app/src/plugin.h declares it. Not
// included from there on purpose: this harness is the thing that would catch
// that declaration drifting, and a harness that shares the declaration under
// test cannot. At namespace scope, because Q_DECLARE_INTERFACE expands to an
// explicit specialisation of qobject_cast and those are only legal there.
class IComponent
{
public:
    virtual ~IComponent() = default;
    virtual QWidget *createWidget(void *api) = 0;
    virtual void destroyWidget(QWidget *widget) = 0;
};
#define IComponent_IID "com.logos.component.IComponent"
Q_DECLARE_INTERFACE(IComponent, IComponent_IID)

namespace {
int failures = 0;
int asserted = 0;
// Every assertion this harness is supposed to make. A run that makes fewer has
// skipped one, and a skipped assertion is not a passing one.
//
// This is defensive rather than a fix for something observed. Building the
// harness against a different Qt from the plugin does put two QtCore frameworks
// in one process and does stop step 5, but Qt aborts there
// (`QWidget: Must construct a QApplication before a QWidget`, SIGABRT, exit
// 134), so that particular case is already loud. The count covers the case that
// would not be: a step that returns early without aborting. A harness whose
// last assertion can be skipped in silence is worth less than no harness, and
// counting is cheaper than reasoning about which skips are loud.
constexpr int EXPECTED_ASSERTIONS = 10;
void check(bool ok, const QString &what)
{
    ++asserted;
    std::fprintf(stderr, "  %s  %s\n", ok ? "ok  " : "FAIL", qPrintable(what));
    if (!ok) ++failures;
}
} // namespace

int main(int argc, char **argv)
{
    // QApplication, not QCoreApplication: step 5 constructs a QWidget, and a
    // QWidget without a QApplication aborts.
    QApplication app(argc, argv);

    if (argc < 3) {
        std::fprintf(stderr, "usage: %s <plugin.dylib|.so> <expected-main-name>\n", argv[0]);
        return 2;
    }
    const QString pluginPath = QString::fromUtf8(argv[1]);
    const QString expectedMain = QString::fromUtf8(argv[2]);

    // ---- 1. every symbol binds, with no host runtime in the process --------
    {
        void *h = dlopen(pluginPath.toUtf8().constData(), RTLD_NOW | RTLD_LOCAL);
        const QString err = h == nullptr ? QString::fromUtf8(dlerror()) : QString();
        check(h != nullptr,
              h != nullptr
                  ? QStringLiteral("binds every symbol against Qt alone, no Logos runtime in the process")
                  : QStringLiteral("RTLD_NOW failed, so a symbol is unresolved: %1").arg(err));
        if (h != nullptr) {
            check(dlsym(h, "qt_plugin_instance") != nullptr,
                  QStringLiteral("exports qt_plugin_instance, which is what QPluginLoader calls"));
        }
    }

    // ---- 2. the loader accepts this, and refuses what is not a plugin ------
    QPluginLoader loader(pluginPath);
    QObject *instance = loader.instance();
    check(instance != nullptr,
          instance != nullptr
              ? QStringLiteral("QPluginLoader accepts it")
              : QStringLiteral("QPluginLoader refused it: %1").arg(loader.errorString()));
    {
        // The control. Without it, "the loader accepted our plugin" is a
        // sentence about a loader nobody has seen say no.
        QTemporaryFile notAPlugin;
        notAPlugin.open();
        notAPlugin.write("{\"this\":\"is json, not a plugin\"}");
        notAPlugin.flush();
        QPluginLoader bogus(notAPlugin.fileName());
        check(bogus.instance() == nullptr,
              QStringLiteral("and refuses a file that is not a plugin, the acceptance above "
                             "is a decision, not a rubber stamp"));
    }

    // ---- 3 & 4. the IID the host compares against, and the metadata --------
    const QJsonObject meta = loader.metaData();
    check(meta.value(QStringLiteral("IID")).toString() == QStringLiteral(IComponent_IID),
          QStringLiteral("declares IID %1, which is what Basecamp compares against (got \"%2\")")
              .arg(QStringLiteral(IComponent_IID), meta.value(QStringLiteral("IID")).toString()));

    const QJsonObject embedded = meta.value(QStringLiteral("MetaData")).toObject();
    check(embedded.value(QStringLiteral("type")).toString() == QStringLiteral("ui"),
          QStringLiteral("its type is \"ui\", a \"core\" type installs into the modules "
                         "directory, where nothing gives it a window (got \"%1\")")
              .arg(embedded.value(QStringLiteral("type")).toString()));
    check(embedded.value(QStringLiteral("main")).toString() == expectedMain,
          QStringLiteral("its `main` is \"%1\", the file that was built (got \"%2\")")
              .arg(expectedMain, embedded.value(QStringLiteral("main")).toString()));

    // ---- 5. the vtable, called through -------------------------------------
    if (instance != nullptr) {
        auto *component = qobject_cast<IComponent *>(instance);
        check(component != nullptr,
              component != nullptr
                  ? QStringLiteral("qobject_cast<IComponent*> succeeds across the plugin boundary")
                  : QStringLiteral("qobject_cast returned null, the host would log "
                                   "\"Plugin does not implement IComponent\""));
        if (component != nullptr) {
            QWidget *w = component->createWidget(nullptr);
            check(w != nullptr,
                  QStringLiteral("createWidget(nullptr) returns a widget rather than crashing, "
                                 "a null api is what it is handed when the runtime is missing"));
            if (w != nullptr) {
                component->destroyWidget(w);
                check(true, QStringLiteral("destroyWidget takes it back"));
            }
        }
    }

    if (asserted < EXPECTED_ASSERTIONS) {
        std::fprintf(stderr,
                     "\nonly %d of %d assertions ran. The rest were skipped, not passed,\n"
                     "most likely the harness and the plugin are linked against different\n"
                     "Qt builds, which puts two QtCore frameworks in one process and makes\n"
                     "QApplication invisible to the plugin's side. Build this against the\n"
                     "same Qt the plugin uses.\n",
                     asserted, EXPECTED_ASSERTIONS);
        return 1;
    }
    std::fprintf(stderr, failures == 0
                     ? "\n%d assertions, all of them ran: the module loads the way Basecamp "
                       "loads a ui plugin\n"
                     : "\n%d assertion(s) failed\n", failures == 0 ? asserted : failures);
    return failures == 0 ? 0 : 1;
}
