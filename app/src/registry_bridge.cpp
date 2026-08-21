// SPDX-License-Identifier: MIT OR Apache-2.0
#include "registry_bridge.h"

#include <QCryptographicHash>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QNetworkReply>
#include <QNetworkRequest>
#include <QUrl>

namespace {

// The registry deployed at block 17474, sealed build. Compiled in rather than
// configured because a wrong registry id derives addresses that hold nothing,
// which reads as "not registered" — a silent wrong answer, the worst kind.
const char* kRegistryIdHex =
    "e1484f1447edd7669d1443154841160f4ddfe4c5d93d410527e5cf99ab9aacba";

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
        default: return "unknown";
    }
}

}  // namespace

RegistryBridge::RegistryBridge(QObject* parent)
    : QObject(parent), m_rpc(QString::fromLatin1(kDefaultRpc)) {}

QString RegistryBridge::registryId() const {
    return QString::fromLatin1(kRegistryIdHex);
}

void RegistryBridge::setEndpoint(const QString& url) {
    if (!url.trimmed().isEmpty()) m_rpc = url.trimmed();
}

QString RegistryBridge::entryAddressFor(const QString& programIdHex) const {
    const QByteArray pid = fromHex32(programIdHex);
    if (pid.isEmpty()) return {};
    const QByteArray registry = fromHex32(QString::fromLatin1(kRegistryIdHex));
    return base58(pda(registry, entrySeed(pid)));
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
        const QJsonArray data =
            result.toObject().value(QStringLiteral("data")).toArray();
        QByteArray raw;
        raw.reserve(data.size());
        for (const auto& v : data) {
            raw.append(static_cast<char>(v.toInt() & 0xFF));
        }
        then(raw, !raw.isEmpty());
    });
}

void RegistryBridge::refreshCounters() {
    const QByteArray registry = fromHex32(QString::fromLatin1(kRegistryIdHex));
    const QString addr = base58(pda(registry, globalSeed()));
    post(addr, [this](const QByteArray& raw, bool present) {
        if (!present) {
            emit failed(QStringLiteral("registry global state is empty"));
            return;
        }
        Reader r{raw};
        const quint64 total    = r.u64();
        const quint64 verified = r.u64();
        if (!r.ok) {
            emit failed(QStringLiteral("global state is shorter than 16 bytes"));
            return;
        }
        emit countersUpdated(QString::number(total), QString::number(verified));
    });
}

void RegistryBridge::lookup(const QString& programIdHex) {
    const QByteArray pid = fromHex32(programIdHex);
    if (pid.isEmpty()) {
        emit failed(QStringLiteral("a program id is 64 hex characters"));
        return;
    }
    const QByteArray registry = fromHex32(QString::fromLatin1(kRegistryIdHex));
    const QString addr = base58(pda(registry, entrySeed(pid)));

    post(addr, [this, addr](const QByteArray& raw, bool present) {
        if (!present) {
            // Not an error. A derived address always exists as an address; what
            // separates a registered program from an unregistered one is whether
            // anything was ever written there.
            emit entryMissing(addr);
            return;
        }
        Reader outer{raw};
        const quint32 payloadLen = outer.u32();
        const QByteArray payload = outer.bytes(static_cast<int>(payloadLen));
        if (!outer.ok) {
            emit failed(QStringLiteral("entry account is truncated"));
            return;
        }

        Reader r{payload};
        r.skip(32);  // program_id, already known
        const QString name    = r.str();
        const QString version = r.str();
        const QString author  = base58(r.bytes(32));
        const quint8  pubTag  = r.u8();
        const QString desc    = r.str();

        const quint32 tagCount = r.u32();
        for (quint32 i = 0; i < tagCount && r.ok; ++i) (void)r.str();

        if (r.u8() == 1) r.skip(32);  // idl: Option<[u8; 32]>

        const quint8 stateTag = r.u8();
        QString repository, toolchain;
        bool offline = false;
        if (r.u8() == 1) {  // claim: Option<SourceClaim>
            repository = r.str();
            r.skip(20);      // commit
            r.skip(32);      // source_digest
            r.skip(32);      // build.image_digest
            toolchain = r.str();
            offline   = r.u8() == 1;
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
                        pubTag == 0 ? QStringLiteral("canonical")
                                    : QStringLiteral("third party"),
                        QString::fromLatin1(stateWord(stateTag)), selfAttested,
                        frozen, repository, toolchain, offline, attester, addr);
    });
}
