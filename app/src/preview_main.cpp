// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Standalone preview harness for local QML iteration. It is NOT part of the
// shipped module — the module is a Basecamp plugin (see plugin.cpp). This
// mirrors exactly what Basecamp does at load: it constructs the RegistryBridge,
// exposes it to QML as `bridge`, and loads Main.qml, so the browse/detail UI can
// be run and screenshotted against the live registry without a full Basecamp
// install. Guarded behind -DBUILD_PREVIEW=ON so it never affects the plugin
// build or CI.
//
//   cmake -B build -DBUILD_PREVIEW=ON -DCMAKE_PREFIX_PATH=<Qt>
//   cmake --build build
//   PREVIEW_QML=qml/Main.qml ./build/registry_preview
//
// Offscreen screenshot (no display needed):
//   QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software \
//   PREVIEW_QML=qml/Main.qml PREVIEW_SHOT=/tmp/gui.png ./build/registry_preview

#include "registry_bridge.h"

#include <QGuiApplication>
#include <QQmlContext>
#include <QQuickView>
#include <QTimer>
#include <QUrl>

int main(int argc, char** argv) {
    QGuiApplication app(argc, argv);

    // The same object the plugin hands QML: derives entry addresses locally and
    // reads them from a sequencer, holds no keys, signs nothing.
    RegistryBridge bridge;

    QQuickView view;
    view.rootContext()->setContextProperty(QStringLiteral("bridge"), &bridge);
    view.setResizeMode(QQuickView::SizeRootObjectToView);
    view.resize(940, 700);

    const QString qml =
        qEnvironmentVariable("PREVIEW_QML", QStringLiteral("qml/Main.qml"));
    view.setSource(QUrl::fromLocalFile(qml));
    if (view.status() == QQuickView::Error) {
        return 2;
    }

    // With PREVIEW_SHOT set, wait PREVIEW_MS (default 6s, long enough for the
    // on-chain walk to populate) then grab the rendered scene and exit.
    const QString shot = qEnvironmentVariable("PREVIEW_SHOT");
    if (!shot.isEmpty()) {
        const int ms =
            qEnvironmentVariable("PREVIEW_MS", QStringLiteral("6000")).toInt();
        QTimer::singleShot(ms, &app, [&view, shot]() {
            view.grabWindow().save(shot);
            QGuiApplication::quit();
        });
    }

    view.show();
    return app.exec();
}
