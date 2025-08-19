// SPDX-License-Identifier: MPL-2.0

import Quickshell
import Quickshell.Io
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ShellRoot {
    property var allFiles: []
    property var filteredFiles: []
    property int selectedIndex: 0
    property int totalFiles: 0
    property bool daemonConnected: false
    property bool responseServerActive: false
    property string userUid: "1000"
    property bool uidReady: false

    onUidReadyChanged: {
        if (uidReady && window) {
            responseServer.active = true
            requestSocket.connected = true
            searchInput.forceActiveFocus()
        }
    }

    function handleDaemonResponse(response) {
        if (response.type === "SearchResults") {
            totalFiles = response.total_files || 0

            filteredFiles = (response.results || []).map(result => ({
                path: result.path,
                displayPath: result.display_path,
                matches: result.matches || [],
                score: result.score || 0
            }))

            selectedIndex = 0
            fileList.positionViewAtIndex(0, ListView.Beginning)

        } else if (response.type === "Error") {
            filteredFiles = []
            totalFiles = 0
        }
    }

    function getHighlightedSegments(text, matches) {
        if (!text) return [{ text: "", highlighted: false, part: 'file' }];

        const lastSlash = text.lastIndexOf('/');
        const dirEnd = lastSlash + 1;

        const matchIndices = matches.map(match => match.char_index).sort((a, b) => a - b);

        if (!matchIndices || matchIndices.length === 0) {
            if (lastSlash === -1) {
                return [{ text: text, highlighted: false, part: 'file' }];
            }
            return [
                { text: text.substring(0, dirEnd), highlighted: false, part: 'dir' },
                { text: text.substring(dirEnd), highlighted: false, part: 'file' }
            ];
        }

        let segments = [];
        if (lastSlash !== -1) {
            const dirText = text.substring(0, dirEnd);
            if (dirText.length) segments.push({ text: dirText, highlighted: false, part: 'dir' });
        }

        const filename = text.substring(dirEnd);
        const relMatches = matchIndices
            .filter(idx => idx >= dirEnd)
            .map(idx => idx - dirEnd);

        if (relMatches.length === 0) {
            segments.push({ text: filename, highlighted: false, part: 'file' });
        } else {
            let lastIndex = 0;
            for (let m of relMatches) {
                if (m > lastIndex) {
                    segments.push({
                        text: filename.substring(lastIndex, m),
                        highlighted: false,
                        part: 'file'
                    });
                }
                segments.push({
                    text: filename[m],
                    highlighted: true,
                    part: 'file'
                });
                lastIndex = m + 1;
            }
            if (lastIndex < filename.length) {
                segments.push({
                    text: filename.substring(lastIndex),
                    highlighted: false,
                    part: 'file'
                });
            }
        }

        let merged = [];
        for (const seg of segments) {
            if (merged.length > 0 &&
                merged[merged.length - 1].highlighted === seg.highlighted &&
                merged[merged.length - 1].part === seg.part) {
                merged[merged.length - 1].text += seg.text;
            } else {
                merged.push(Object.assign({}, seg));
            }
        }

        return merged;
    }

    Process {
        running: true
        command: ["id", "-u"]
        stdout: StdioCollector {
            onStreamFinished: {
                userUid = this.text.trim()
                console.log("User UID:", userUid)
                uidReady = true
            }
        }
    }

    PanelWindow {
        id: window
        implicitWidth: 680
        implicitHeight: 400
        focusable: true
        color: "transparent"

        Component.onCompleted: {
            if (uidReady) {
                responseServer.active = true
                requestSocket.connected = true
                searchInput.forceActiveFocus()
            }
        }

        Component.onDestruction: {
            responseServer.active = false
            requestSocket.connected = false
        }

        SocketServer {
            id: responseServer
            path: "/run/user/" + userUid + "/quickfile-response.sock"

            onActiveChanged: {
                responseServerActive = active
                if (active) {
                } else {
                }
            }

            handler: Socket {
                property string responseBuffer: ""

                onConnectedChanged: {
                    if (connected) {
                    } else {
                    }
                }

                parser: SplitParser {
                    onRead: data => {
                        if (data.trim() !== "") {
                            try {
                                let response = JSON.parse(data)
                                handleDaemonResponse(response)
                            } catch (e) {
                            }
                        }
                    }
                }
            }
        }

        Socket {
            id: requestSocket
            path: "/run/user/" + userUid + "/quickfile-daemon.sock"

            onConnectedChanged: {
                if (connected) {
                    daemonConnected = true
                    sendSearchRequest("")
                } else {
                    daemonConnected = false
                }
            }

            function sendSearchRequest(query) {
                if (!connected) {
                    return
                }

                let request = {
                    "type": "Search",
                    "query": query,
                    "limit": 100
                }

                let jsonRequest = JSON.stringify(request) + "\n"
                write(jsonRequest)
                flush()
            }
        }

        Rectangle {
            anchors.fill: parent
            color: "#1a1a1d"
            radius: 8
            border.color: "#2a2a2f"
            border.width: 1
            layer.enabled: true

            ColumnLayout {
                anchors.fill: parent
                anchors.margins: 8
                spacing: 0

                Rectangle {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 36
                    color: "transparent"

                    TextField {
                        id: searchInput
                        anchors.fill: parent
                        anchors.leftMargin: 12
                        anchors.rightMargin: 12
                        placeholderText: "Search files..."
                        placeholderTextColor: "#6c7086"
                        color: "#cdd6f4"
                        font.pixelSize: 14
                        font.family: "SF Pro Display, -apple-system, system-ui, sans-serif"
                        selectByMouse: true

                        background: Rectangle {
                            color: "transparent"
                        }

                        property string lastQuery: ""
                        Timer {
                            id: searchTimer
                            interval: 100
                            onTriggered: {
                                if (searchInput.text !== searchInput.lastQuery) {
                                    searchInput.lastQuery = searchInput.text
                                    if (daemonConnected) {
                                        requestSocket.sendSearchRequest(searchInput.text)
                                    }
                                }
                            }
                        }

                        onTextChanged: {
                            searchTimer.restart()
                        }

                        Keys.onDownPressed: {
                            if (selectedIndex < filteredFiles.length - 1) {
                                selectedIndex++
                                fileList.positionViewAtIndex(selectedIndex, ListView.Contain)
                            }
                        }

                        Keys.onUpPressed: {
                            if (selectedIndex > 0) {
                                selectedIndex--
                                fileList.positionViewAtIndex(selectedIndex, ListView.Contain)
                            }
                        }

                        Keys.onReturnPressed: {
                            if (filteredFiles.length > 0 && selectedIndex < filteredFiles.length) {
                                Quickshell.execDetached(["xdg-open", filteredFiles[selectedIndex].path])
                                Qt.quit()
                            }
                        }

                        Keys.onEscapePressed: {
                            Qt.quit()
                        }
                    }
                }

                Rectangle {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 1
                    color: "#2a2a2f"
                }

                ListView {
                    id: fileList
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    model: filteredFiles
                    currentIndex: selectedIndex
                    spacing: 0
                    clip: true

                    delegate: Rectangle {
                        width: fileList.width
                        height: 30
                        color: index === selectedIndex ? "#2a2a3e" : (mouseArea.containsMouse ? "#222226" : "transparent")

                        Row {
                            anchors.verticalCenter: parent.verticalCenter
                            anchors.left: parent.left
                            anchors.right: parent.right
                            anchors.leftMargin: 12
                            anchors.rightMargin: 12
                            spacing: 0
                            clip: true

                            Repeater {
                                model: getHighlightedSegments(modelData.displayPath, modelData.matches)

                                Text {
                                    text: modelData.text
                                    color: modelData.highlighted ? "#89b4fa" : "#e0e0e0"
                                    font.pixelSize: 13
                                    font.family: "SF Mono, Monaco, 'Cascadia Code', 'Roboto Mono', monospace"
                                    font.weight: modelData.highlighted ? Font.Bold : Font.Normal
                                }
                            }
                        }

                        MouseArea {
                            id: mouseArea
                            anchors.fill: parent
                            hoverEnabled: true

                            onEntered: {
                                selectedIndex = index
                            }

                            onClicked: {
                                Quickshell.execDetached(["xdg-open", modelData.path])
                                Qt.quit()
                            }
                        }
                    }

                    ScrollBar.vertical: ScrollBar {
                        active: true
                        policy: ScrollBar.AsNeeded
                        width: 6

                        contentItem: Rectangle {
                            implicitWidth: 6
                            radius: 3
                            color: parent.pressed ? "#505050" : "#404040"
                        }

                        background: Rectangle {
                            color: "transparent"
                        }
                    }
                }

                Rectangle {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 1
                    color: "#2a2a2f"
                }

                Rectangle {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 24
                    color: "transparent"

                    Row {
                        anchors.verticalCenter: parent.verticalCenter
                        anchors.left: parent.left
                        anchors.leftMargin: 12
                        spacing: 6

                        Text {
                            text: {
                                if (!responseServerActive) {
                                    return "Starting..."
                                } else if (!daemonConnected) {
                                    return "Connecting..."
                                } else if (totalFiles === 0) {
                                    return "Loading..."
                                } else {
                                    return `${filteredFiles.length}/${totalFiles}`
                                }
                            }
                            color: "#606060"
                            font.pixelSize: 11
                            font.weight: Font.Medium
                        }

                    }

                    Text {
                        anchors.verticalCenter: parent.verticalCenter
                        anchors.right: parent.right
                        anchors.rightMargin: 12
                        text: "⏎ Open  ⎋ Close"
                        color: "#505050"
                        font.pixelSize: 11
                    }
                }
            }
        }
    }
}
