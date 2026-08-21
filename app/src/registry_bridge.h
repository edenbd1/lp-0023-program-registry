// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Bridge exposed to QML as `bridge`.
//
// The point of this panel is that it never asks an index. Given a program id it
// **derives** the entry's account address locally — two SHA-256 hashes — and
// reads that account from a LEZ sequencer. There is no server in the path that
// could omit a program, serve a stale answer, or go down separately from the
// chain.
//
// It holds no keys and signs nothing. A registry browser with signing power
// would be a custody risk for no gain.
//
// Asynchronous by construction. Basecamp 0.2.2 does not bundle QtConcurrent, and
// a nested event loop inside createWidget would freeze the host, so requests go
// through QNetworkAccessManager and are answered by signal.

#pragma once

#include <QNetworkAccessManager>
#include <QObject>
#include <QString>

class RegistryBridge : public QObject {
    Q_OBJECT
public:
    explicit RegistryBridge(QObject* parent = nullptr);

    /// Derive this program's entry address and read it. Answered by
    /// `entryFound`, `entryMissing` or `failed` — never by leaving the panel
    /// showing the previous program's answer.
    Q_INVOKABLE void lookup(const QString& programIdHex);

    /// Read the registry's counters.
    Q_INVOKABLE void refreshCounters();

    /// Walk every entry. Answered by `entryFound` per entry, then `walkDone`.
    ///
    /// Listing exists because resolving does not need it: an entry's address is
    /// a function of the program id, so a client that knows what it is looking
    /// for never walks. Browsing is the case where you do not know, and it is
    /// linear and openly so.
    Q_INVOKABLE void listEntries();

    /// Show the address without reading it, so the derivation is visible even
    /// when the network is not.
    Q_INVOKABLE QString entryAddressFor(const QString& programIdHex) const;

    Q_INVOKABLE void    setEndpoint(const QString& url);
    Q_INVOKABLE QString endpoint() const { return m_rpc; }
    Q_INVOKABLE QString registryId() const;

signals:
    /// `verification` is one of unclaimed / claimed / verified / unreproducible.
    /// `selfAttested` is true when the only party vouching for the build is the
    /// entry's own author — deliberately a separate flag rather than a fifth
    /// state, because it is a property of who attested, not of what was proven.
    void entryFound(const QString& name, const QString& version,
                    const QString& description, const QString& author,
                    const QString& publisher, const QString& verification,
                    bool selfAttested, bool frozen, const QString& repository,
                    const QString& toolchain, bool offlineBuild,
                    const QString& attester, const QString& account,
                    const QString& registeredBlock, const QString& registeredMillis,
                    const QString& lastCheckedBlock, const QString& idl);
    void entryMissing(const QString& account);
    void countersUpdated(const QString& total, const QString& verified);
    void walkProgress(int done, int total);
    void walkDone(int found);
    void failed(const QString& what);

private:
    void post(const QString& accountB58,
              const std::function<void(const QByteArray&, bool)>& then);
    void walkFrom(quint64 n, quint64 total, int found);
    void emitEntry(const QByteArray& payload, const QString& account);

    QNetworkAccessManager m_net;
    QString m_rpc;
};
