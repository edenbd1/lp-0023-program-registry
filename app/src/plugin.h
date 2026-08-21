// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Top-level Qt plugin object for the LP-0023 program registry panel. Owns the
// QQuickWidget hosting the QML scene and exposes the RegistryBridge to it.

#pragma once

#include <QObject>
#include <QString>
#include <QWidget>

// LogosAPI is forward-declared rather than included so this header builds
// standalone in the IDE-only preview path.
class LogosAPI;
class RegistryBridge;

// Basecamp's IComponent interface, declared here so the manual build path does
// not need the SDK header on the include path.
//
// This declaration is ABI-critical and is not a guess: it mirrors, slot for
// slot, the secondary vtable Basecamp's own `main_ui` plugin emits for
// IComponent (the destructor, then createWidget and destroyWidget). An extra
// virtual here — `name()`, say — shifts every later slot, so the host would call
// the wrong function through a correctly cast pointer. Verified against
// LogosBasecamp 0.2.2.
class IComponent {
public:
    virtual ~IComponent() = default;
    virtual QWidget* createWidget(LogosAPI* api) = 0;
    virtual void destroyWidget(QWidget* widget) = 0;
};

// The interface string is what `qobject_cast<IComponent*>` compares across the
// plugin boundary, so it has to be Basecamp's rather than one of our own
// invention: a private IID makes the cast return null and the host logs
// "Plugin does not implement IComponent".
#define IComponent_IID "com.logos.component.IComponent"

Q_DECLARE_INTERFACE(IComponent, IComponent_IID)

class RegistryPlugin : public QObject, public IComponent {
    Q_OBJECT
    Q_PLUGIN_METADATA(IID IComponent_IID FILE "metadata.json")
    Q_INTERFACES(IComponent)

public:
    explicit RegistryPlugin(QObject* parent = nullptr);
    ~RegistryPlugin() override;

    // Not part of IComponent — Basecamp reads the module name from
    // metadata.json. Kept as a plain accessor for the preview app.
    QString  name() const { return QStringLiteral("lez_program_registry"); }
    QWidget* createWidget(LogosAPI* api) override;
    void     destroyWidget(QWidget* widget) override;

private:
    RegistryBridge* m_bridge = nullptr;
};
