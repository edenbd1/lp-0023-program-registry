// SPDX-License-Identifier: MIT OR Apache-2.0
#include "plugin.h"
#include "registry_bridge.h"

#include <QQmlContext>
#include <QQmlEngine>
#include <QQuickWidget>
#include <QUrl>

RegistryPlugin::RegistryPlugin(QObject* parent) : QObject(parent) {}

RegistryPlugin::~RegistryPlugin() = default;

QWidget* RegistryPlugin::createWidget(LogosAPI* /*api*/) {
    // The bridge derives entry addresses locally and reads them from a
    // sequencer. It holds no keys and signs nothing: a registry browser that
    // could sign would be a custody risk for no gain.
    m_bridge = new RegistryBridge(this);

    // Qt's resource system is process-global: two modules that both
    // register /qml/Main.qml resolve to whichever registered first, so a
    // second module rendered the first one's UI over its own
    // data. The prefix is this module's name, which cannot collide.
    auto* view = new QQuickWidget();
    view->engine()->rootContext()->setContextProperty(
        QStringLiteral("bridge"), m_bridge);
    view->setResizeMode(QQuickWidget::SizeRootObjectToView);
    // Without a focus policy the widget never takes keyboard focus inside
    // Basecamp's dock: a click reaches the QML, but keys go to the host, so a
    // text field cannot be typed into. Measured in Basecamp 0.3.0.
    view->setFocusPolicy(Qt::StrongFocus);
    view->setSource(QUrl(QStringLiteral("qrc:/lez_program_registry/Main.qml")));
    return view;
}

void RegistryPlugin::destroyWidget(QWidget* widget) {
    if (widget) {
        widget->deleteLater();
    }
}
