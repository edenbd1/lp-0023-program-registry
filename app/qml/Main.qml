// SPDX-License-Identifier: MIT OR Apache-2.0
//
// The registry, browsed and looked up — never searched through an index.
//
// Two screens. A *list* of every registered program, scannable and sortable,
// filterable by name or author. A *detail* page you navigate into from a row,
// carrying the full entry: metadata, the IDL identifier, the source claim down
// to its commit, and the verification verdict spelled out rather than reduced to
// a tick.
//
// The list is built by *walking* the on-chain sequence directory. A single
// lookup takes a different path on purpose: paste a 64-character program id and
// the panel *derives* where its entry lives — two SHA-256 hashes, computed here —
// and shows that address before the answer arrives. It is the evidence that
// nothing was asked of an index that could omit a program or serve a stale one.
//
// Nothing here signs. A registry browser with signing power would be a custody
// risk for no gain, so registering lives elsewhere, where an operator sees what
// they are putting their name to.

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: root
    implicitWidth: 880
    implicitHeight: 660

    // ---- palette ---------------------------------------------------------
    readonly property color bg:      "#0e1116"
    readonly property color panel:   "#161b22"
    readonly property color panel2:  "#1b212a"
    readonly property color line:    "#2a313b"
    readonly property color fg:      "#e4e8ef"
    readonly property color muted:   "#8d97a6"
    readonly property color accent:  "#7fb0e0"
    readonly property color good:    "#5fb87f"
    readonly property color caution: "#d6a45e"
    readonly property color bad:     "#d1706a"

    // ---- live state ------------------------------------------------------
    property string status:  "Reading the registry…"
    property bool   busy:     false   // a network read is in flight
    property bool   walking:  false   // the in-flight read is the full walk
    property bool   awaiting: false   // the in-flight read is a single lookup
    property bool   loaded:   false   // the walk has finished at least once
    property bool   errored:  false
    property string derived:  ""      // address a typed 64-char id resolves to
    property string query:    ""      // the live filter text
    property int    sortKey:  0       // 0 registered · 1 name · 2 author · 3 state
    property bool   sortAsc:  false
    property var    allEntries: []    // every walked entry, unfiltered
    property string counterTotal:    "—"
    property string counterVerified: "—"

    // Verified is deliberately not one colour. A build whose only voucher is its
    // own author is not the same claim as one a third party reproduced, and
    // painting both green would hide the difference that matters most.
    function stateColour(state, self) {
        if (state === "verified")       return self ? caution : good
        if (state === "mismatched")     return bad
        if (state === "unreproducible") return bad
        if (state === "claimed")        return caution
        return muted
    }

    // A compact label for the list; the detail page uses the full headline.
    function stateBadge(state, self) {
        if (state === "verified")       return self ? "self-attested" : "verified"
        if (state === "mismatched")     return "mismatched"
        if (state === "unreproducible") return "unreproducible"
        if (state === "claimed")        return "claimed"
        if (state === "unclaimed")      return "no source"
        return state
    }

    function stateHeadline(state, self) {
        if (state === "verified" && self)   return "Verified — by its own author"
        if (state === "verified")           return "Verified — independently rebuilt"
        if (state === "mismatched")         return "Mismatched — the published source does not rebuild to this"
        if (state === "unreproducible")     return "Not reproducible"
        if (state === "claimed")            return "Source claimed, not verified"
        return "No source claimed"
    }

    function stateExplanation(state, self) {
        if (state === "verified" && self)
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

    // Sort key for the "State" column: most trustworthy first when ascending.
    function stateRank(state, self) {
        if (state === "verified" && !self) return 0
        if (state === "verified")          return 1
        if (state === "claimed")           return 2
        if (state === "unreproducible")    return 3
        if (state === "mismatched")        return 4
        return 5
    }

    function publisherColour(pub) { return pub === "canonical" ? accent : muted }

    // The registration timestamp, from the sequencer's clock — the field the
    // whole adoption story rests on, so it is shown, not hidden in a block number.
    function formatStamp(millis) {
        var n = parseInt(millis)
        if (!n || isNaN(n)) return "—"
        var d = new Date(n)
        function p(x) { return (x < 10 ? "0" : "") + x }
        return d.getUTCFullYear() + "-" + p(d.getUTCMonth() + 1) + "-" + p(d.getUTCDate())
             + " " + p(d.getUTCHours()) + ":" + p(d.getUTCMinutes()) + " UTC"
    }

    // A commit or program id that is all zeroes is an absent field, not a value.
    function isZeroHex(s) { return !s || /^0*$/.test(s) }

    // The detail page is handed one of these persistent objects, never a
    // ListModel row: a row is invalidated the moment the list is filtered or
    // reloaded, which would blank an open detail page under the reader.
    function entryByAccount(acct) {
        for (var i = 0; i < root.allEntries.length; ++i)
            if (root.allEntries[i].account === acct) return root.allEntries[i]
        return null
    }

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

    function compareEntries(a, b) {
        var r = 0
        if (root.sortKey === 1)
            r = a.name.toLowerCase().localeCompare(b.name.toLowerCase())
        else if (root.sortKey === 2)
            r = a.author.localeCompare(b.author)
        else if (root.sortKey === 3)
            r = root.stateRank(a.verification, a.selfAttested)
              - root.stateRank(b.verification, b.selfAttested)
        else
            r = (parseInt(a.registeredMillis) || 0) - (parseInt(b.registeredMillis) || 0)
        if (r === 0) r = a.name.toLowerCase().localeCompare(b.name.toLowerCase())
        return root.sortAsc ? r : -r
    }

    // Rebuild the visible list from the full set: filter, then sort. Cheap for a
    // registry of hundreds, and it is the one place order and filtering are
    // decided so the two never disagree.
    function rebuild() {
        var arr = []
        for (var i = 0; i < root.allEntries.length; ++i)
            if (root.matches(root.allEntries[i], root.query))
                arr.push(root.allEntries[i])
        arr.sort(root.compareEntries)
        visibleModel.clear()
        for (var j = 0; j < arr.length; ++j) visibleModel.append(arr[j])
    }

    function setSort(k) {
        if (root.sortKey === k) root.sortAsc = !root.sortAsc
        else { root.sortKey = k; root.sortAsc = (k !== 0) }
        rebuild()
    }

    function reload() {
        if (root.busy) return
        root.allEntries = []
        visibleModel.clear()
        root.loaded   = false
        root.errored  = false
        root.busy     = true
        root.walking  = true
        root.status   = "Reading the registry…"
        bridge.listEntries()
    }

    // ---- a small reusable badge -----------------------------------------
    component Pill: Rectangle {
        property string label: ""
        property color  tint:  root.muted
        implicitHeight: 18
        implicitWidth:  pillText.implicitWidth + 16
        radius: 9
        color: Qt.rgba(tint.r, tint.g, tint.b, 0.14)
        border.color: tint
        border.width: 1
        Text {
            id: pillText
            anchors.centerIn: parent
            text: parent.label
            color: parent.tint
            font.pixelSize: 10
        }
    }

    ListModel { id: visibleModel }

    Connections {
        target: bridge

        function onWalkProgress(done, total) {
            root.status = "Reading " + (done + 1) + " of " + total + "…"
        }

        function onWalkDone(found) {
            root.busy    = false
            root.walking = false
            root.loaded  = true
            root.status  = found + " entries read from " + bridge.endpoint()
            // The walk grows allEntries with in-place push, which a `var`
            // property does not notify on; reassign so the "N programs" count
            // (and anything else bound to its length) updates now that the set
            // is complete.
            root.allEntries = root.allEntries.slice()
            root.rebuild()
        }

        function onEntryFound(name, version, description, author, publisher,
                              verification, selfAttested, frozen, repository,
                              toolchain, offlineBuild, attester, account,
                              registeredBlock, registeredMillis, lastCheckedBlock,
                              idl, commit, programId) {
            var rec = {
                name: name, version: version, description: description,
                author: author, publisher: publisher, verification: verification,
                selfAttested: selfAttested, frozen: frozen, repository: repository,
                toolchain: toolchain, offlineBuild: offlineBuild,
                attester: attester, account: account,
                registeredBlock: registeredBlock, registeredMillis: registeredMillis,
                lastCheckedBlock: lastCheckedBlock, idl: idl,
                commit: commit, programId: programId
            }
            // A walk fills the list; a single lookup opens a page. The same signal
            // serves both, told apart by which read is in flight.
            if (root.walking) {
                root.allEntries.push(rec)
                if (root.matches(rec, root.query)) visibleModel.append(rec)
            } else {
                root.awaiting = false
                root.busy     = false
                root.status   = "Read from " + bridge.endpoint()
                nav.push(detailComponent, { entry: rec })
            }
        }

        function onEntryMissing(account) {
            root.busy = false
            if (root.awaiting) {
                root.awaiting = false
                root.status = "Nothing is registered at " + account
                            + " — the address exists, it was simply never written to."
            }
        }

        function onCountersUpdated(total, verified) {
            root.counterTotal    = total
            root.counterVerified = verified
        }

        function onFailed(what) {
            root.busy     = false
            root.walking  = false
            root.awaiting = false
            root.errored  = true
            root.status   = "Failed: " + what
        }
    }

    Component.onCompleted: root.reload()

    Rectangle {
        anchors.fill: parent
        color: root.bg

        StackView {
            id: nav
            anchors.fill: parent
            initialItem: listComponent
        }
    }

    // ======================================================================
    //  LIST PAGE
    // ======================================================================
    Component {
        id: listComponent

        Item {
            ColumnLayout {
                anchors.fill: parent
                anchors.margins: 18
                spacing: 12

                // -- title + counters --
                RowLayout {
                    Layout.fillWidth: true
                    Text {
                        text: "LEZ Program Registry"
                        color: root.fg; font.pixelSize: 20; font.bold: true
                    }
                    Item { Layout.fillWidth: true }
                    Text {
                        text: root.counterTotal + " registered · " + root.counterVerified + " verified"
                        color: root.muted; font.pixelSize: 13
                    }
                }

                Text {
                    Layout.fillWidth: true
                    wrapMode: Text.WordWrap
                    color: root.muted
                    font.pixelSize: 12
                    text: "Registry " + bridge.registryId().slice(0, 16) + "… on " + bridge.endpoint()
                        + ". Every row was walked from the chain; addresses are derived here, never looked up."
                }

                // -- filter + single-lookup --
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 8

                    TextField {
                        id: input
                        Layout.fillWidth: true
                        placeholderText: "filter by name or author — or paste a 64-character program id to resolve"
                        color: root.fg
                        font.family: "monospace"
                        font.pixelSize: 12
                        selectByMouse: true
                        background: Rectangle { color: root.panel; border.color: root.line; radius: 4 }
                        onTextChanged: {
                            root.query   = text.trim()
                            root.derived = bridge.entryAddressFor(text.trim())
                            root.rebuild()
                        }
                        onAccepted: if (resolve.enabled) resolve.clicked()
                    }

                    Button {
                        id: resolve
                        text: root.awaiting ? "Resolving…" : "Resolve id"
                        // A full program id resolves to a page; anything shorter
                        // is a filter over the list and needs no button.
                        enabled: !root.busy && input.text.trim().length === 64
                        onClicked: {
                            root.awaiting = true
                            root.busy     = true
                            root.status   = "Deriving and reading…"
                            bridge.lookup(input.text.trim())
                        }
                    }

                    Button {
                        text: "Reload"
                        enabled: !root.busy
                        onClicked: root.reload()
                    }
                }

                // The derived address, shown before any answer arrives: it is a
                // function of the program id alone, so it is knowable offline.
                Rectangle {
                    Layout.fillWidth: true
                    visible: root.derived.length > 0
                    color: root.panel; border.color: root.line; radius: 4
                    implicitHeight: derivedText.implicitHeight + 16
                    Text {
                        id: derivedText
                        anchors.fill: parent
                        anchors.margins: 8
                        wrapMode: Text.WrapAnywhere
                        color: root.accent
                        font.family: "monospace"
                        font.pixelSize: 11
                        text: "derives to  " + root.derived
                    }
                }

                // -- sort controls + result count --
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 6
                    Text { text: "sort"; color: root.muted; font.pixelSize: 11 }

                    Repeater {
                        model: [ { k: 0, label: "Registered" },
                                 { k: 1, label: "Name" },
                                 { k: 2, label: "Author" },
                                 { k: 3, label: "State" } ]
                        delegate: Button {
                            required property var modelData
                            text: modelData.label
                                + (root.sortKey === modelData.k ? (root.sortAsc ? "  ↑" : "  ↓") : "")
                            checkable: false
                            font.pixelSize: 11
                            palette.buttonText: root.sortKey === modelData.k ? root.fg : root.muted
                            onClicked: root.setSort(modelData.k)
                        }
                    }

                    Item { Layout.fillWidth: true }

                    Text {
                        color: root.muted; font.pixelSize: 11
                        text: root.query.length > 0 && root.query.length !== 64
                            ? visibleModel.count + " of " + root.allEntries.length + " shown"
                            : root.allEntries.length + " programs"
                    }
                }

                // -- the list, or a state message in its place --
                Rectangle {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    color: root.panel; border.color: root.line; radius: 6

                    // loading / empty / error, each said plainly.
                    Text {
                        anchors.centerIn: parent
                        width: parent.width - 48
                        horizontalAlignment: Text.AlignHCenter
                        wrapMode: Text.WordWrap
                        visible: visibleModel.count === 0
                        color: root.errored ? root.bad : root.muted
                        font.pixelSize: 13
                        text: root.errored
                                ? root.status
                                : (root.busy && !root.loaded)
                                    ? "Reading the registry…"
                                    : (root.allEntries.length === 0)
                                        ? "No programs are registered yet."
                                        : "No entries match “" + root.query + "”."
                    }

                    ListView {
                        id: listView
                        anchors.fill: parent
                        anchors.margins: 6
                        clip: true
                        model: visibleModel
                        spacing: 2
                        ScrollBar.vertical: ScrollBar { }

                        delegate: Rectangle {
                            id: row
                            required property int index
                            required property var model
                            width: listView.width - 4
                            height: 56
                            radius: 5
                            color: hover.hovered ? root.panel2 : "transparent"

                            HoverHandler { id: hover }
                            TapHandler {
                                onTapped: {
                                    var rec = root.entryByAccount(row.model.account)
                                    if (rec) nav.push(detailComponent, { entry: rec })
                                }
                            }

                            ColumnLayout {
                                anchors.fill: parent
                                anchors.leftMargin: 10
                                anchors.rightMargin: 10
                                anchors.topMargin: 7
                                anchors.bottomMargin: 7
                                spacing: 4

                                // line 1: name + version .... timestamp
                                RowLayout {
                                    Layout.fillWidth: true
                                    spacing: 8
                                    Text {
                                        text: row.model.name
                                        color: root.fg; font.pixelSize: 13; font.bold: true
                                        elide: Text.ElideRight
                                    }
                                    Text {
                                        text: row.model.version
                                        color: root.muted; font.pixelSize: 12
                                    }
                                    Item { Layout.fillWidth: true }
                                    Text {
                                        text: root.formatStamp(row.model.registeredMillis)
                                        color: root.muted; font.pixelSize: 11
                                    }
                                }

                                // line 2: state badge + publisher badge .... author
                                RowLayout {
                                    Layout.fillWidth: true
                                    spacing: 6
                                    Pill {
                                        label: root.stateBadge(row.model.verification, row.model.selfAttested)
                                        tint:  root.stateColour(row.model.verification, row.model.selfAttested)
                                    }
                                    Pill {
                                        label: row.model.publisher
                                        tint:  root.publisherColour(row.model.publisher)
                                    }
                                    Pill {
                                        visible: row.model.frozen
                                        label: "frozen"
                                        tint:  root.caution
                                    }
                                    Item { Layout.fillWidth: true }
                                    Text {
                                        text: row.model.author
                                        color: root.muted
                                        font.family: "monospace"; font.pixelSize: 10
                                        elide: Text.ElideMiddle
                                        Layout.maximumWidth: 220
                                    }
                                }
                            }
                        }
                    }
                }

                // -- status footer --
                Text {
                    Layout.fillWidth: true
                    wrapMode: Text.WordWrap
                    color: root.errored ? root.bad : root.muted
                    font.pixelSize: 11
                    text: root.status
                }
            }
        }
    }

    // ======================================================================
    //  DETAIL PAGE
    // ======================================================================
    Component {
        id: detailComponent

        Item {
            property var entry: ({})

            ColumnLayout {
                anchors.fill: parent
                anchors.margins: 18
                spacing: 12

                // -- back + name + badges --
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 10
                    Button {
                        text: "‹  Back"
                        font.pixelSize: 12
                        onClicked: nav.pop()
                    }
                    Text {
                        text: entry.name ? entry.name : ""
                        color: root.fg; font.pixelSize: 18; font.bold: true
                        elide: Text.ElideRight
                        Layout.maximumWidth: 360
                    }
                    Text {
                        text: entry.version ? entry.version : ""
                        color: root.muted; font.pixelSize: 13
                    }
                    Item { Layout.fillWidth: true }
                    Pill {
                        label: entry.publisher ? entry.publisher : ""
                        tint:  root.publisherColour(entry.publisher)
                    }
                    Pill {
                        visible: entry.frozen === true
                        label: "frozen"
                        tint:  root.caution
                    }
                }

                ScrollView {
                    id: scroller
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    clip: true
                    contentWidth: availableWidth

                    ColumnLayout {
                        width: scroller.availableWidth
                        spacing: 12

                        Text {
                            Layout.fillWidth: true
                            wrapMode: Text.WordWrap
                            text: entry.description ? entry.description : "No description."
                            color: root.muted; font.pixelSize: 12
                        }

                        // -- the verdict, spelled out --
                        Rectangle {
                            Layout.fillWidth: true
                            implicitHeight: verdict.implicitHeight + 20
                            color: "transparent"
                            border.width: 1
                            border.color: root.stateColour(entry.verification, entry.selfAttested)
                            radius: 4
                            ColumnLayout {
                                id: verdict
                                anchors.fill: parent
                                anchors.margins: 12
                                spacing: 5
                                Text {
                                    text: root.stateHeadline(entry.verification, entry.selfAttested)
                                    color: root.stateColour(entry.verification, entry.selfAttested)
                                    font.pixelSize: 15; font.bold: true
                                }
                                Text {
                                    Layout.fillWidth: true
                                    wrapMode: Text.WordWrap
                                    text: root.stateExplanation(entry.verification, entry.selfAttested)
                                    color: root.muted; font.pixelSize: 11
                                }
                                Text {
                                    Layout.fillWidth: true
                                    wrapMode: Text.WordWrap
                                    text: "Verified means only that the deployed bytecode was produced from the "
                                        + "stated source. It says nothing about whether that source is safe, "
                                        + "audited or benign."
                                    color: root.muted; font.pixelSize: 11; font.italic: true
                                }
                            }
                        }

                        // -- provenance --
                        Text { text: "PROVENANCE"; color: root.muted; font.pixelSize: 10; font.bold: true }

                        GridLayout {
                            Layout.fillWidth: true
                            columns: 2
                            columnSpacing: 16
                            rowSpacing: 6

                            Text { text: "program id"; color: root.muted; font.pixelSize: 11 }
                            Text {
                                Layout.fillWidth: true; wrapMode: Text.WrapAnywhere
                                text: entry.programId ? entry.programId : "—"
                                color: root.fg; font.pixelSize: 11; font.family: "monospace"
                            }

                            Text { text: "entry account"; color: root.muted; font.pixelSize: 11 }
                            Text {
                                Layout.fillWidth: true; wrapMode: Text.WrapAnywhere
                                text: (entry.account ? entry.account : "—") + "   (derived, not looked up)"
                                color: root.accent; font.pixelSize: 11; font.family: "monospace"
                            }

                            Text { text: "author"; color: root.muted; font.pixelSize: 11 }
                            Text {
                                Layout.fillWidth: true; wrapMode: Text.WrapAnywhere
                                text: entry.author ? entry.author : "—"
                                color: root.fg; font.pixelSize: 11; font.family: "monospace"
                            }

                            Text {
                                text: "attested by"; color: root.muted; font.pixelSize: 11
                                visible: entry.attester && entry.attester.length > 0
                            }
                            Text {
                                visible: entry.attester && entry.attester.length > 0
                                Layout.fillWidth: true; wrapMode: Text.WrapAnywhere
                                text: entry.attester ? entry.attester : ""
                                color: root.fg; font.pixelSize: 11; font.family: "monospace"
                            }

                            Text { text: "registered"; color: root.muted; font.pixelSize: 11 }
                            Text {
                                text: root.formatStamp(entry.registeredMillis)
                                    + "   ·   block " + (entry.registeredBlock ? entry.registeredBlock : "?")
                                color: root.fg; font.pixelSize: 11
                            }

                            Text { text: "last checked"; color: root.muted; font.pixelSize: 11 }
                            Text {
                                text: (entry.lastCheckedBlock && entry.lastCheckedBlock !== "0")
                                    ? "block " + entry.lastCheckedBlock
                                    : "never — no rebuild has been recorded"
                                color: root.fg; font.pixelSize: 11
                            }
                        }

                        // -- source claim --
                        Text { text: "SOURCE"; color: root.muted; font.pixelSize: 10; font.bold: true }

                        Text {
                            Layout.fillWidth: true
                            visible: !(entry.repository && entry.repository.length > 0)
                            wrapMode: Text.WordWrap
                            text: "No source claimed. Nobody has said where this program's source lives."
                            color: root.muted; font.pixelSize: 11
                        }

                        GridLayout {
                            Layout.fillWidth: true
                            visible: entry.repository && entry.repository.length > 0
                            columns: 2
                            columnSpacing: 16
                            rowSpacing: 6

                            Text { text: "repository"; color: root.muted; font.pixelSize: 11 }
                            Text {
                                Layout.fillWidth: true; wrapMode: Text.WrapAnywhere
                                text: entry.repository ? entry.repository : ""
                                color: root.accent; font.pixelSize: 11
                            }

                            Text {
                                text: "commit"; color: root.muted; font.pixelSize: 11
                                visible: entry.commit && !root.isZeroHex(entry.commit)
                            }
                            Text {
                                visible: entry.commit && !root.isZeroHex(entry.commit)
                                Layout.fillWidth: true; wrapMode: Text.WrapAnywhere
                                text: entry.commit ? entry.commit : ""
                                color: root.fg; font.pixelSize: 11; font.family: "monospace"
                            }

                            Text {
                                text: "toolchain"; color: root.muted; font.pixelSize: 11
                                visible: entry.toolchain && entry.toolchain.length > 0
                            }
                            Text {
                                visible: entry.toolchain && entry.toolchain.length > 0
                                text: entry.toolchain ? entry.toolchain : ""
                                color: root.fg; font.pixelSize: 11
                            }

                            Text { text: "build network"; color: root.muted; font.pixelSize: 11 }
                            Text {
                                text: entry.offlineBuild
                                    ? "disabled — a match can mean something"
                                    : "enabled — a match proves nothing"
                                color: entry.offlineBuild ? root.good : root.bad
                                font.pixelSize: 11
                            }
                        }

                        // -- IDL, degrading gracefully to "unavailable" --
                        Text { text: "IDL"; color: root.muted; font.pixelSize: 10; font.bold: true }

                        Rectangle {
                            Layout.fillWidth: true
                            implicitHeight: idlCol.implicitHeight + 20
                            color: root.panel; border.color: root.line; radius: 4
                            ColumnLayout {
                                id: idlCol
                                anchors.fill: parent
                                anchors.margins: 10
                                spacing: 4
                                Text {
                                    text: (entry.idl && entry.idl.length > 0)
                                        ? "Published on Logos Storage."
                                        : "No IDL published for this entry."
                                    color: (entry.idl && entry.idl.length > 0) ? root.fg : root.muted
                                    font.pixelSize: 11
                                }
                                Text {
                                    visible: entry.idl && entry.idl.length > 0
                                    Layout.fillWidth: true; wrapMode: Text.WrapAnywhere
                                    text: entry.idl ? entry.idl : ""
                                    color: root.accent; font.pixelSize: 11; font.family: "monospace"
                                }
                                Text {
                                    visible: entry.idl && entry.idl.length > 0
                                    Layout.fillWidth: true; wrapMode: Text.WordWrap
                                    text: "This is the on-chain content identifier. Resolving the document itself "
                                        + "happens out of band (e.g. `lez-registry idl`); if a Storage node cannot "
                                        + "serve it the identifier is still valid and the entry is untouched — an "
                                        + "unresolvable IDL is reported as unavailable, never as a failure."
                                    color: root.muted; font.pixelSize: 10
                                }
                            }
                        }

                        Text {
                            Layout.fillWidth: true
                            wrapMode: Text.WordWrap
                            text: "Names, descriptions and tags are asserted by whoever registered the entry. "
                                + "The registry does not vouch for them."
                            color: root.muted; font.pixelSize: 10; font.italic: true
                        }

                        Item { Layout.preferredHeight: 4 }
                    }
                }
            }
        }
    }
}
