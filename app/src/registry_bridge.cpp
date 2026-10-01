// SPDX-License-Identifier: MIT OR Apache-2.0
#include "registry_bridge.h"

#include <QCryptographicHash>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QNetworkProxy>
#include <QNetworkReply>
#include <QNetworkRequest>
#include <QUrl>

namespace {

// No registry is compiled in: the testnet v0.3 deployment is pending funding,
// and a built-in id from a superseded chain would derive addresses that hold
// nothing, which reads as "not registered", a silent wrong answer. The panel
// takes it from PROVENANCE_REGISTRY or from setRegistry(), and says so when
// neither is set.

const char* kDefaultRpc = "https://testnet.lez.logos.co";

QByteArray sha256(const QByteArray& in) {
    return QCryptographicHash::hash(in, QCryptographicHash::Sha256);
}

QByteArray fromHex32(const QString& hex) {
    const QString s = hex.trimmed().toLower();
    if (s.size() != 64) return {};
    const QByteArray raw = QByteArray::fromHex(s.toLatin1());
    return raw.size() == 32 ? raw : QByteArray();
}

// Base58 to 32 bytes, as the wallet prints account ids (`Public/` optional).
// Empty on a bad character or a length that is not 32 bytes.
QByteArray fromBase58(QString s) {
    static const QByteArray kAlphabet(
        "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz");
    s = s.trimmed();
    if (s.startsWith(QStringLiteral("Public/"))) s = s.mid(7);
    QList<quint8> bytes;  // little-endian accumulator
    for (const QChar c : s) {
        const int v = kAlphabet.indexOf(static_cast<char>(c.toLatin1()));
        if (v < 0 || c.unicode() > 127) return {};
        int carry = v;
        for (auto& b : bytes) {
            carry += b * 58;
            b = static_cast<quint8>(carry & 0xFF);
            carry >>= 8;
        }
        while (carry > 0) {
            bytes.append(static_cast<quint8>(carry & 0xFF));
            carry >>= 8;
        }
    }
    for (const QChar c : s) {
        if (c != QLatin1Char('1')) break;
        bytes.append(0);
    }
    if (bytes.size() != 32) return {};
    QByteArray out(32, '\0');
    for (int i = 0; i < 32; ++i) out[i] = static_cast<char>(bytes[31 - i]);
    return out;
}

// An account id given as base58 or as 64 hex characters.
QByteArray fromAccount(const QString& s) {
    const QByteArray hex = fromHex32(s);
    return hex.isEmpty() ? fromBase58(s) : hex;
}

// Base58, matching the chain's account rendering. Leading zero bytes are
// significant and must survive as '1's: dropping them yields a shorter string
// that decodes to a different account.
QString base58(const QByteArray& in) {
    static const char* kAlphabet =
        "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    QList<quint8> digits;
    for (unsigned char byte : in) {
        int carry = byte;
        for (auto& d : digits) {
            carry += d << 8;
            d = static_cast<quint8>(carry % 58);
            carry /= 58;
        }
        while (carry > 0) {
            digits.append(static_cast<quint8>(carry % 58));
            carry /= 58;
        }
    }
    QString out;
    for (unsigned char byte : in) {
        if (byte != 0) break;
        out.append(QLatin1Char('1'));
    }
    for (auto it = digits.crbegin(); it != digits.crend(); ++it) {
        out.append(QLatin1Char(kAlphabet[*it]));
    }
    return out;
}

// LEZ's program-derived address. Copied from the runtime rather than invented:
// lee/state_machine/core, AccountId::for_public_pda.
QByteArray pda(const QByteArray& programId, const QByteArray& seed) {
    QByteArray buf;
    buf.append("/LEE/v0.2/AccountId/PDA/", 24);
    buf.append(8, '\0');
    buf.append(programId);
    buf.append(seed);
    return sha256(buf);
}

QByteArray entrySeed(const QByteArray& programId) {
    QByteArray buf("lez-registry:entry:v1");
    buf.append(programId);
    return sha256(buf);
}

// Multibase base58btc, the `z…` form a Logos Storage node hands you. Separate
// from `base58` because that one takes a fixed 32 bytes and a content
// identifier is 38.
QString base58Cid(const QByteArray& in) {
    static const char* kAlphabet =
        "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    QList<quint8> digits;
    for (unsigned char byte : in) {
        int carry = byte;
        for (auto& d : digits) {
            carry += d << 8;
            d = static_cast<quint8>(carry % 58);
            carry /= 58;
        }
        while (carry > 0) {
            digits.append(static_cast<quint8>(carry % 58));
            carry /= 58;
        }
    }
    QString out(QLatin1Char('z'));
    for (auto it = digits.crbegin(); it != digits.crend(); ++it) {
        out.append(QLatin1Char(kAlphabet[*it]));
    }
    return out;
}

QByteArray seqSeed(quint64 n) {
    QByteArray buf("lez-registry:seq:v1");
    for (int i = 0; i < 8; ++i) buf.append(static_cast<char>((n >> (8 * i)) & 0xFF));
    return sha256(buf);
}

QByteArray globalSeed() {
    // Right-padded, not hashed: the program declares this account as
    // pda = [literal("...")] and SPEL turns a literal into 32 bytes by padding.
    // Hashing it here would produce a well-formed address the chain has nothing
    // at — a mistake this project has already made once, in Rust.
    QByteArray seed("lez-registry:global:v1");
    seed.append(32 - seed.size(), '\0');
    return seed;
}

// Borsh reader. Every read is bounds-checked and sets `ok` false rather than
// returning a plausible zero, because an account one byte short would otherwise
// render as a real, wrong answer.
struct Reader {
    const QByteArray& b;
    int  pos = 0;
    bool ok  = true;

    void skip(int n) {
        if (pos + n > b.size()) { ok = false; return; }
        pos += n;
    }
    quint8 u8() {
        if (pos + 1 > b.size()) { ok = false; return 0; }
        return static_cast<quint8>(b[pos++]);
    }
    quint32 u32() {
        if (pos + 4 > b.size()) { ok = false; return 0; }
        quint32 v = 0;
        for (int i = 3; i >= 0; --i) v = (v << 8) | static_cast<quint8>(b[pos + i]);
        pos += 4;
        return v;
    }
    quint64 u64() {
        if (pos + 8 > b.size()) { ok = false; return 0; }
        quint64 v = 0;
        for (int i = 7; i >= 0; --i) v = (v << 8) | static_cast<quint8>(b[pos + i]);
        pos += 8;
        return v;
    }
    QByteArray bytes(int n) {
        if (pos + n > b.size()) { ok = false; return {}; }
        QByteArray out = b.mid(pos, n);
        pos += n;
        return out;
    }
    QString str() {
        const quint32 n = u32();
        if (!ok || pos + static_cast<int>(n) > b.size()) { ok = false; return {}; }
        QString out = QString::fromUtf8(b.mid(pos, static_cast<int>(n)));
        pos += static_cast<int>(n);
        return out;
    }
};

const char* stateWord(quint8 tag) {
    switch (tag) {
        case 0:  return "unclaimed";
        case 1:  return "claimed";
        case 2:  return "verified";
        case 3:  return "unreproducible";
        case 4:  return "mismatched";
        case 5:  return "stale";
        default: return "unknown";
    }
}

}  // namespace

RegistryBridge::RegistryBridge(QObject* parent)
    : QObject(parent), m_rpc(QString::fromLatin1(kDefaultRpc)) {
    const QByteArray envReg = qgetenv("PROVENANCE_REGISTRY");
    if (!envReg.isEmpty()) m_registry = fromAccount(QString::fromLatin1(envReg));
    const QByteArray envRpc = qgetenv("LEZ_RPC");
    if (!envRpc.isEmpty()) m_rpc = QString::fromLatin1(envRpc);
    // Qt's macOS system-proxy lookup builds a QRegularExpression, PCRE2 tries to
    // JIT-compile it, and pthread_jit_write_protect_np traps: Basecamp runs under
    // the hardened runtime without com.apple.security.cs.allow-jit, so the first
    // HTTP request took the whole host process down with SIGTRAP — no dialog, no
    // log line. A module cannot add an entitlement to somebody else's signed
    // binary, so it declines the lookup instead. Direct connection only, which is
    // what talking to a sequencer over its public URL wants anyway.
    QNetworkProxyFactory::setUseSystemConfiguration(false);
    m_net.setProxy(QNetworkProxy::NoProxy);
}

QString RegistryBridge::registryId() const {
    return m_registry.isEmpty() ? QString() : base58(m_registry);
}

bool RegistryBridge::setRegistry(const QString& account) {
    const QByteArray id = fromAccount(account);
    if (id.isEmpty()) return false;
    m_registry = id;
    return true;
}

bool RegistryBridge::isProgramId(const QString& text) const {
    return !fromAccount(text).isEmpty();
}

bool RegistryBridge::needRegistry() {
    if (!m_registry.isEmpty()) return true;
    emit failed(QStringLiteral(
        "no registry set: set PROVENANCE_REGISTRY to the registry program account"));
    return false;
}

void RegistryBridge::setEndpoint(const QString& url) {
    if (!url.trimmed().isEmpty()) m_rpc = url.trimmed();
}

QString RegistryBridge::entryAddressFor(const QString& programId) const {
    const QByteArray pid = fromAccount(programId);
    if (pid.isEmpty() || m_registry.isEmpty()) return {};
    return base58(pda(m_registry, entrySeed(pid)));
}

void RegistryBridge::post(const QString& accountB58,
                          const std::function<void(const QByteArray&, bool)>& then) {
    QJsonObject body{{"jsonrpc", "2.0"},
                     {"id", 1},
                     {"method", "getAccount"},
                     {"params", QJsonArray{accountB58}}};
    QNetworkRequest req{QUrl(m_rpc)};
    req.setHeader(QNetworkRequest::ContentTypeHeader, "application/json");
    QNetworkReply* reply = m_net.post(req, QJsonDocument(body).toJson());
    connect(reply, &QNetworkReply::finished, this, [this, reply, then]() {
        reply->deleteLater();
        if (reply->error() != QNetworkReply::NoError) {
            emit failed(reply->errorString());
            return;
        }
        const QJsonObject root =
            QJsonDocument::fromJson(reply->readAll()).object();
        const QJsonValue result = root.value(QStringLiteral("result"));
        if (!result.isObject()) {
            then({}, false);
            return;
        }
        // LEZ v0.3: an account holds one shard per program, keyed by the
        // program's account id. The registry's data is its own shard.
        const QJsonArray data = result.toObject()
                                    .value(QStringLiteral("data")).toObject()
                                    .value(QStringLiteral("shards")).toObject()
                                    .value(base58(m_registry)).toArray();
        QByteArray raw;
        raw.reserve(data.size());
        for (const auto& v : data) {
            raw.append(static_cast<char>(v.toInt() & 0xFF));
        }
        then(raw, !raw.isEmpty());
    });
}

void RegistryBridge::refreshCounters() {
    if (!needRegistry()) return;
    const QString addr = base58(pda(m_registry, globalSeed()));
    post(addr, [this](const QByteArray& raw, bool present) {
        if (!present) {
            emit failed(QStringLiteral("registry global state is empty"));
            return;
        }
        Reader r{raw};
        const quint64 total = r.u64();
        if (!r.ok) {
            emit failed(QStringLiteral("global state is shorter than 8 bytes"));
            return;
        }
        // On v0.3 the global account keeps only the directory counter; the
        // verified count is the walk's, reported when a walk finishes.
        emit countersUpdated(QString::number(total), QStringLiteral("—"));
    });
}

void RegistryBridge::emitEntry(const QByteArray& payload, const QString& addr) {
    Reader r{payload};
    // program_id: the program's header account on LEZ v0.3, what calls dispatch
    // to and the seed every entry address is derived from.
    const QString programId = base58(r.bytes(32));
    // image_id and header_immutable: the header as the registry checked it on
    // chain. A mutable header can be repointed after a verdict.
    const QString imageId = QString::fromLatin1(r.bytes(32).toHex());
    const bool headerImmutable = r.u8() == 1;
    // registered_at: Stamp { block: u64, millis: u64 }, read from the
    // sequencer's clock at registration rather than supplied by the
    // registrant.
    const quint64 regBlock  = r.u64();
    const quint64 regMillis = r.u64();
    const QString name    = r.str();
    const QString version = r.str();
    const QString author  = base58(r.bytes(32));
    const quint8  pubTag  = r.u8();
    const QString desc    = r.str();

    const quint32 tagCount = r.u32();
    for (quint32 i = 0; i < tagCount && r.ok; ++i) (void)r.str();

    // idl: Option<ContentId>, and a ContentId is 38 bytes, not 32. A real Logos
    // Storage identifier is a CIDv1 marker, a varint codec and a multihash that
    // carries its own code and length before the digest. Skipping 32 here read
    // six bytes of the identifier as the verification state and shifted every
    // field after it -- invisible until an entry actually had an IDL, which the
    // registry's own entry now does.
    QString idl;
    if (r.u8() == 1) {
        idl = base58Cid(r.bytes(38));
    }

    const quint8 stateTag = r.u8();
    QString repository, toolchain, commit;
    bool offline = false;
    if (r.u8() == 1) {  // claim: Option<SourceClaim>
        repository = r.str();
        // commit: [u8; 20], a git object id. Surfaced (as hex) so a reader can
        // pin the exact revision the claim was built from, not only the repo.
        commit = QString::fromLatin1(r.bytes(20).toHex());
        r.skip(32);      // source_digest
        r.skip(32);      // build.image_digest
        toolchain = r.str();
        offline   = r.u8() == 1;
    }
    // last_checked: Option<Stamp>. A verdict with no date cannot be judged
    // for staleness, so the panel is given one.
    quint64 checkedBlock = 0;
    if (r.u8() == 1) {
        checkedBlock = r.u64();
        (void)r.u64();  // millis, not displayed
    }
    QString attester;
    if (r.u8() == 1) attester = base58(r.bytes(32));
    const bool frozen = r.u8() == 1;

    if (!r.ok) {
        emit failed(QStringLiteral("entry did not decode"));
        return;
    }

    const bool verified = stateTag == 2;
    const bool selfAttested = verified && !attester.isEmpty() && attester == author;

    emit entryFound(name, version, desc, author,
            pubTag == 0 ? QStringLiteral("deployer")
                    : QStringLiteral("third party"),
            QString::fromLatin1(stateWord(stateTag)), selfAttested,
                    frozen, repository, toolchain, offline, attester, addr,
                    QString::number(regBlock), QString::number(regMillis),
                    QString::number(checkedBlock), idl, commit, programId,
                    imageId, headerImmutable);
    if (verified) ++m_walkVerified;
}

void RegistryBridge::lookup(const QString& programId) {
    if (!needRegistry()) return;
    const QByteArray pid = fromAccount(programId);
    if (pid.isEmpty()) {
        emit failed(QStringLiteral("a program id is an account id: base58, or 64 hex characters"));
        return;
    }
    const QString addr = base58(pda(m_registry, entrySeed(pid)));

    post(addr, [this, addr](const QByteArray& raw, bool present) {
        if (!present) {
            // A derived address always exists as an address; what separates a
            // registered program from an unregistered one is whether anything
            // was ever written there.
            emit entryMissing(addr);
            return;
        }
        // v0.3 stores the Entry as plain borsh in the registry's shard.
        emitEntry(raw, addr);
    });
}

// A chain of signals rather than a loop: two reads per entry, and a loop would
// need a nested event loop, which is what freezes the host.
void RegistryBridge::walkFrom(quint64 n, quint64 total, int found) {
    if (n >= total) {
        emit countersUpdated(QString::number(total), QString::number(m_walkVerified));
        emit walkDone(found);
        return;
    }
    emit walkProgress(static_cast<int>(n), static_cast<int>(total));

    const QByteArray registry = m_registry;
    post(base58(pda(registry, seqSeed(n))),
         [this, n, total, found, registry](const QByteArray& raw, bool present) {
             if (!present) {
                 walkFrom(n + 1, total, found);
                 return;
             }
             Reader slot{raw};
             const QByteArray programId = slot.bytes(32);
             const QByteArray seed = slot.bytes(32);
             (void)programId;
             if (!slot.ok) {
                 walkFrom(n + 1, total, found);
                 return;
             }
             // The slot's own seed, not one derived from the program id: a
             // third-party entry does not live at the canonical address, and
             // deriving would silently drop it from the listing.
             const QString addr = base58(pda(registry, seed));
             post(addr, [this, n, total, found, addr](const QByteArray& raw2, bool present2) {
                 if (present2) emitEntry(raw2, addr);
                 // One unreadable slot must not end the walk.
                 walkFrom(n + 1, total, present2 ? found + 1 : found);
             });
         });
}

void RegistryBridge::listEntries() {
    if (!needRegistry()) return;
    post(base58(pda(m_registry, globalSeed())), [this](const QByteArray& raw, bool present) {
        if (!present) {
            emit failed(QStringLiteral("the registry is not initialized"));
            return;
        }
        Reader r{raw};
        const quint64 total = r.u64();
        if (!r.ok) {
            emit failed(QStringLiteral("global state is shorter than 8 bytes"));
            return;
        }
        emit countersUpdated(QString::number(total), QStringLiteral("…"));
        m_walkVerified = 0;
        walkFrom(0, total, 0);
    });
}
