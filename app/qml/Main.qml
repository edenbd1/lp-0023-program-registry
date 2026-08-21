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

    property string status: "Enter a program id."
    property bool   busy: false
    property var    entry: null
    property string derived: ""

    // Verified is deliberately not one colour. A build whose only voucher is its
    // own author is not the same claim as one a third party reproduced, and
    // painting both green would hide the difference that matters most.
    function stateColour(state, selfAttested) {
        if (state === "verified") return selfAttested ? caution : good
        if (state === "unreproducible") return bad
        if (state === "claimed") return caution
        return muted
    }

    function stateHeadline(state, selfAttested) {
        if (state === "verified" && selfAttested) return "Verified — by its own author"
        if (state === "verified") return "Verified — independently rebuilt"
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
        if (state === "unreproducible")
            return "A rebuild was recorded, but the claimed build had network access — so a "
                 + "match would prove nothing about the source."
        if (state === "claimed")
            return "A source is asserted and nobody has rebuilt it. Typing a URL is not evidence."
        return "Nobody has said where the source is. This is the honest default, not a failure."
    }

    Connections {
        target: bridge
        function onEntryFound(name, version, description, author, publisher,
                              verification, selfAttested, frozen, repository,
                              toolchain, offlineBuild, attester, account) {
            root.entry = {
                name: name, version: version, description: description,
                author: author, publisher: publisher, verification: verification,
                selfAttested: selfAttested, frozen: frozen, repository: repository,
                toolchain: toolchain, offlineBuild: offlineBuild,
                attester: attester, account: account
            }
            root.busy = false
            root.status = "Read from " + bridge.endpoint()
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
            root.status = "Failed: " + what
        }
    }

    Component.onCompleted: bridge.refreshCounters()

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
                    placeholderText: "program id — 64 hex characters"
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

            Text {
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                text: root.status
                color: muted; font.pixelSize: 11
            }
        }
    }
}
