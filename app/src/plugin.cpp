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

    auto* view = new QQuickWidget();
    view->engine()->rootContext()->setContextProperty(
        QStringLiteral("bridge"), m_bridge);
    view->setResizeMode(QQuickWidget::SizeRootObjectToView);
    view->setSource(QUrl(QStringLiteral("qrc:/qml/Main.qml")));
    return view;
}

void RegistryPlugin::destroyWidget(QWidget* widget) {
    if (widget) {
        widget->deleteLater();
    }
}
