// SPDX-License-Identifier: MIT OR Apache-2.0
//
// The registry, looked up rather than searched.
//
// Type a program id and this panel *derives* where its entry lives — two
// SHA-256 hashes, computed here — then reads that account from a sequencer. The
// derived address is shown before the answer arrives, on purpose: it is the
// evidence that nothing was asked of an index that could omit a program or
// serve a stale answer.
//
// Nothing here signs. A registry browser with signing power would be a custody
// risk for no gain.

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: root
    implicitWidth: 760
    implicitHeight: 620

    readonly property color bg:      "#0e1116"
    readonly property color panel:   "#161b22"
    readonly property color line:    "#2a313b"
    readonly property color fg:      "#e4e8ef"
    readonly property color muted:   "#8d97a6"
    readonly property color accent:  "#7fb0e0"
    readonly property color good:    "#5fb87f"
    readonly property color caution: "#d6a45e"
    readonly property color bad:     "#d1706a"

    property string status: "Loading the registry…"
    property bool   busy: false
    // Where an incoming entry belongs. `busy` cannot answer that: a
    // single lookup sets it too, and appending on it would push a
    // looked-up entry into the listing.
    property bool   walking: false
    property var    entry: null
    property string derived: ""

    // Verified is deliberately not one colour. A build whose only voucher is its
    // own author is not the same claim as one a third party reproduced, and
    // painting both green would hide the difference that matters most.
    function stateColour(state, selfAttested) {
        if (state === "verified") return selfAttested ? caution : good
        if (state === "mismatched") return bad
        if (state === "unreproducible") return bad
        if (state === "claimed") return caution
        return muted
    }

    function stateHeadline(state, selfAttested) {
        if (state === "verified" && selfAttested) return "Verified — by its own author"
        if (state === "verified") return "Verified — independently rebuilt"
        if (state === "mismatched") return "Mismatched — the published source does not rebuild to this"
        if (state === "unreproducible") return "Not reproducible"
        if (state === "claimed") return "Source claimed, not verified"
        return "No source claimed"
    }

    function stateExplanation(state, selfAttested) {
        if (state === "verified" && selfAttested)
            return "The only party vouching for this build is the party that shipped it. "
                 + "That is useful, and it is not an independent check."
        if (state === "verified")
            return "Someone other than the author rebuilt this offline and the bytecode matched."
        if (state === "mismatched")
            return "Somebody published a source repository for this program and it does not "
                 + "produce this bytecode. That is a stronger negative signal than no claim "
                 + "at all — do not read it as merely unverified."
        if (state === "unreproducible")
            return "A rebuild was recorded, but the claimed build had network access — so a "
                 + "match would prove nothing about the source."
        if (state === "claimed")
            return "A source is asserted and nobody has rebuilt it. Typing a URL is not evidence."
        return "Nobody has said where the source is. This is the honest default, not a failure."
    }

    ListModel { id: entries }

    function matches(e, q) {
        if (!q) return true
        q = q.toLowerCase()
        return e.name.toLowerCase().indexOf(q) >= 0
            || e.version.toLowerCase().indexOf(q) >= 0
            || e.description.toLowerCase().indexOf(q) >= 0
            || e.verification.indexOf(q) >= 0
            || e.publisher.indexOf(q) >= 0
            || e.author.toLowerCase().indexOf(q) >= 0
    }

    Connections {
        target: bridge
        function onWalkProgress(done, total) {
            root.status = "Reading " + (done + 1) + " of " + total + "…"
        }
        function onWalkDone(found) {
            root.busy = false
            root.walking = false
            root.status = found + " entries read from " + bridge.endpoint()
        }
        function onEntryFound(name, version, description, author, publisher,
                              verification, selfAttested, frozen, repository,
                              toolchain, offlineBuild, attester, account,
                              registeredBlock, registeredMillis, lastCheckedBlock, idl) {
            var rec = {
                name: name, version: version, description: description,
                author: author, publisher: publisher, verification: verification,
                selfAttested: selfAttested, frozen: frozen, repository: repository,
                toolchain: toolchain, offlineBuild: offlineBuild,
                attester: attester, account: account,
                registeredBlock: registeredBlock, registeredMillis: registeredMillis,
                lastCheckedBlock: lastCheckedBlock, idl: idl
            }
            // A walk fills the list; a single lookup selects. The same signal
            // serves both because the entry is the same object either way.
            // Only a single lookup ends the reading state; a walk ends at
            // walkDone. Clearing `busy` unconditionally below this line stopped
            // the list after one entry — every later one took the else branch
            // and replaced the detail panel instead of being appended, so the
            // header said "6 registered" over a list of one.
            if (root.walking) {
                entries.append(rec)
            } else {
                root.selected = rec
                root.busy = false
                root.status = "Read from " + bridge.endpoint()
            }
            root.entry = {
                name: name, version: version, description: description,
                author: author, publisher: publisher, verification: verification,
                selfAttested: selfAttested, frozen: frozen, repository: repository,
                toolchain: toolchain, offlineBuild: offlineBuild,
                attester: attester, account: account,
                registeredBlock: registeredBlock, registeredMillis: registeredMillis,
                lastCheckedBlock: lastCheckedBlock, idl: idl
            }
        }
        function onEntryMissing(account) {
            root.entry = null
            root.busy = false
            root.status = "Nothing is registered at " + account
                        + " — the address exists, it was simply never written to."
        }
        function onCountersUpdated(total, verified) {
            counters.text = total + " registered · " + verified + " verified"
        }
        function onFailed(what) {
            root.busy = false
            root.walking = false
            root.status = "Failed: " + what
        }
    }

    property var selected: null

    Component.onCompleted: { root.busy = true; root.walking = true; bridge.listEntries() }

    Rectangle {
        anchors.fill: parent
        color: bg

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 18
            spacing: 14

            RowLayout {
                Layout.fillWidth: true
                Text {
                    text: "LEZ Program Registry"
                    color: fg; font.pixelSize: 19; font.bold: true
                }
                Item { Layout.fillWidth: true }
                Text {
                    id: counters
                    text: "—"
                    color: muted; font.pixelSize: 13
                }
            }

            Text {
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                color: muted
                font.pixelSize: 12
                text: "Registry " + bridge.registryId().slice(0, 16) + "… on " + bridge.endpoint()
                    + ". Addresses are derived here, never looked up."
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                TextField {
                    id: input
                    Layout.fillWidth: true
                    placeholderText: "filter the list, or paste a 64-character program id to resolve"
                    color: fg
                    font.family: "monospace"
                    font.pixelSize: 12
                    background: Rectangle {
                        color: panel; border.color: line; radius: 4
                    }
                    onTextChanged: root.derived = bridge.entryAddressFor(text)
                    onAccepted: go.clicked()
                }

                Button {
                    id: go
                    text: root.busy ? "Reading…" : "Look up"
                    // Enabled only for a full program id: anything shorter is a
                    // filter over the list, which needs no button.
                    enabled: !root.busy && input.text.trim().length === 64
                    onClicked: {
                        root.busy = true
                        root.status = "Deriving and reading…"
                        bridge.lookup(input.text.trim())
                    }
                }
            }

            // Shown before any answer arrives: the address is a function of the
            // program id alone, so it is knowable without the network.
            Rectangle {
                Layout.fillWidth: true
                visible: root.derived.length > 0
                color: panel; border.color: line; radius: 4
                implicitHeight: derivedText.implicitHeight + 16
                Text {
                    id: derivedText
                    anchors.fill: parent
                    anchors.margins: 8
                    wrapMode: Text.WrapAnywhere
                    color: accent
                    font.family: "monospace"
                    font.pixelSize: 11
                    text: "derives to  " + root.derived
                }
            }

            RowLayout {
                Layout.fillWidth: true
                Layout.fillHeight: true
                spacing: 12

                // Browsing. Resolving never needs this -- an entry's address is
                // a function of the program id -- so the list exists for the
                // case where you do not know what you are looking for.
                Rectangle {
                    Layout.preferredWidth: 300
                    Layout.fillHeight: true
                    color: panel; border.color: line; radius: 6

                    ListView {
                        id: listView
                        anchors.fill: parent
                        anchors.margins: 6
                        clip: true
                        model: entries
                        spacing: 1

                        delegate: Rectangle {
                            width: listView.width
                            height: visible ? 46 : 0
                            visible: matches(model, input.text)
                            color: root.selected && root.selected.account === model.account
                                   ? "#20262b" : "transparent"
                            radius: 4

                            ColumnLayout {
                                anchors.fill: parent
                                anchors.leftMargin: 8
                                anchors.rightMargin: 8
                                spacing: 1
                                Text {
                                    text: model.name + "  " + model.version
                                    color: fg; font.pixelSize: 12
                                    Layout.fillWidth: true; elide: Text.ElideRight
                                }
                                Text {
                                    // The verdict in the list, not only in the
                                    // detail: a reader scanning names should see
                                    // which claims are backed by what.
                                    text: stateHeadline(model.verification, model.selfAttested)
                                    color: stateColour(model.verification, model.selfAttested)
                                    font.pixelSize: 10
                                    Layout.fillWidth: true; elide: Text.ElideRight
                                }
                            }
                            MouseArea {
                                anchors.fill: parent
                                onClicked: { root.selected = entries.get(index); root.entry = root.selected }
                            }
                        }
                    }
                }


                Rectangle {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    color: panel; border.color: line; radius: 6
                    visible: root.entry !== null

                    ColumnLayout {
                        anchors.fill: parent
                        anchors.margins: 16
                        spacing: 10

                        RowLayout {
                            Layout.fillWidth: true
                            Text {
                                text: root.entry ? root.entry.name : ""
                                color: fg; font.pixelSize: 17; font.bold: true
                            }
                            Text {
                                text: root.entry ? root.entry.version : ""
                                color: muted; font.pixelSize: 13
                            }
                            Item { Layout.fillWidth: true }
                            Text {
                                visible: root.entry && root.entry.frozen
                                text: "frozen"
                                color: caution; font.pixelSize: 12
                            }
                        }

                        Text {
                            Layout.fillWidth: true
                            wrapMode: Text.WordWrap
                            text: root.entry ? root.entry.description : ""
                            color: muted; font.pixelSize: 12
                        }

                        Rectangle {
                            Layout.fillWidth: true
                            implicitHeight: verdict.implicitHeight + 20
                            color: "transparent"
                            border.color: root.entry
                                ? stateColour(root.entry.verification, root.entry.selfAttested)
                                : line
                            border.width: 1
                            radius: 4

                            ColumnLayout {
                                id: verdict
                                anchors.fill: parent
                                anchors.margins: 10
                                spacing: 4
                                Text {
                                    text: root.entry
                                        ? stateHeadline(root.entry.verification, root.entry.selfAttested)
                                        : ""
                                    color: root.entry
                                        ? stateColour(root.entry.verification, root.entry.selfAttested)
                                        : muted
                                    font.pixelSize: 14; font.bold: true
                                }
                                Text {
                                    Layout.fillWidth: true
                                    wrapMode: Text.WordWrap
                                    text: root.entry
                                        ? stateExplanation(root.entry.verification, root.entry.selfAttested)
                                        : ""
                                    color: muted; font.pixelSize: 11
                                }
                            }
                        }

                        GridLayout {
                            Layout.fillWidth: true
                            columns: 2
                            columnSpacing: 14
                            rowSpacing: 5

                            Text { text: "publisher";  color: muted; font.pixelSize: 11 }
                            Text { text: root.entry ? root.entry.publisher : ""
                                   color: fg; font.pixelSize: 11 }

                            Text { text: "author";     color: muted; font.pixelSize: 11 }
                            Text { text: root.entry ? root.entry.author : ""
                                   color: fg; font.pixelSize: 11; font.family: "monospace" }

                            Text { text: "attested by"; color: muted; font.pixelSize: 11
                                   visible: root.entry && root.entry.attester.length > 0 }
                            Text { text: root.entry ? root.entry.attester : ""
                                   color: fg; font.pixelSize: 11; font.family: "monospace"
                                   visible: root.entry && root.entry.attester.length > 0 }

                            Text { text: "repository"; color: muted; font.pixelSize: 11
                                   visible: root.entry && root.entry.repository.length > 0 }
                            Text { Layout.fillWidth: true; wrapMode: Text.WrapAnywhere
                                   text: root.entry ? root.entry.repository : ""
                                   color: accent; font.pixelSize: 11
                                   visible: root.entry && root.entry.repository.length > 0 }

                            Text { text: "toolchain";  color: muted; font.pixelSize: 11
                                   visible: root.entry && root.entry.toolchain.length > 0 }
                            Text { text: root.entry ? root.entry.toolchain : ""
                                   color: fg; font.pixelSize: 11
                                   visible: root.entry && root.entry.toolchain.length > 0 }

                            Text { text: "build network"; color: muted; font.pixelSize: 11
                                   visible: root.entry && root.entry.repository.length > 0 }
                            Text {
                                visible: root.entry && root.entry.repository.length > 0
                                text: root.entry
                                    ? (root.entry.offlineBuild
                                        ? "disabled — a match can mean something"
                                        : "enabled — a match proves nothing")
                                    : ""
                                color: root.entry && root.entry.offlineBuild ? good : bad
                                font.pixelSize: 11
                            }
                        }

                        Item { Layout.fillHeight: true }
                    }
                }
            }

            Text {
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                visible: root.entry !== null
                color: muted
                font.pixelSize: 11
                text: "Names, descriptions and tags are asserted by whoever registered the "
                    + "entry. The registry does not vouch for them. And verified means only "
                    + "that the deployed bytecode was produced from the stated source — it "
                    + "says nothing about whether that source is safe, audited or benign."
            }

            Text {
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                text: root.status
                color: muted; font.pixelSize: 11
            }
        }
    }
}
