import QtQuick
import Quickshell
import Quickshell.Wayland
import qs.Commons

// Alpha presentation only: no commands, radio decisions, or transfer mutation.
Scope {
  id: root
  property bool enabled: false
  property bool connected: false
  property var transfers: []
  property var cancellingTransfers: ({})
  property var preview: null
  property var anchorWindow: null
  property var anchorItem: null
  property var bar: null
  property string language: "en"

  readonly property var activeTransfers: (transfers || []).filter(function(t) {
    return t && ["completed", "rejected", "cancelled", "failed"].indexOf(t.state) < 0
      && ["outbound", "inbound"].indexOf(t.direction) >= 0
  })
  readonly property bool previewing: activeTransfers.length === 0 && !!preview
  readonly property var activities: activeTransfers.length > 0 ? activeTransfers : preview ? [preview] : []
  readonly property int activityCount: activities.length
  readonly property var transferIdentities: activeTransfers.map(function(t) { return String(t.id) }).sort()
  readonly property var projection: project()
  readonly property string mode: projection.length === 0 ? "idle" : projection.length === 1 ? projection[0].mode : "mixed"
  readonly property var barWindow: anchorItem ? anchorItem.QsWindow.window : null
  readonly property bool panelVisible: !!anchorWindow && !!anchorWindow.visible
  readonly property bool panelOpen: panelVisible && !!anchorWindow.open
  readonly property var targetScreen: {
    var candidate = panelVisible ? anchorWindow.screen : barWindow ? barWindow.screen : null
    // Never leave an effects surface on an output removed by the compositor.
    var screens = Quickshell.screens
    for (var i = 0; candidate && i < screens.length; i++)
      if (screens[i].name === candidate.name) return screens[i]
    return null
  }
  readonly property bool running: enabled && connected && activityCount > 0 && !!targetScreen
  readonly property bool animating: running && projection.some(function(a) { return a.mode !== "stopping" })
  property int animationFrame: 0
  readonly property real clock: animationFrame / 30
  readonly property bool surfaceVisible: effectWindow.visible
  readonly property var layerWindow: effectWindow
  readonly property bool inputRegionEmpty: effectWindow.mask.width === 0 && effectWindow.mask.height === 0
  readonly property string screenName: targetScreen ? targetScreen.name : ""
  readonly property real screenWidth: targetScreen ? targetScreen.width : 0
  readonly property real screenHeight: targetScreen ? targetScreen.height : 0
  readonly property real haloReach: Style.space(44)
  readonly property real safetyInset: Style.space(8)
  readonly property rect cardRect: panelVisible
    ? Qt.rect(anchorWindow.cardOrigin.x, anchorWindow.cardOrigin.y,
              anchorWindow.contentWidth, anchorWindow.contentHeight) : Qt.rect(0, 0, 0, 0)
  readonly property rect surfaceRect: {
    if (!targetScreen) return Qt.rect(0, 0, 1, 1)
    if (panelVisible) {
      var x = Math.max(0, Math.floor(cardRect.x - haloReach))
      var y = Math.max(0, Math.floor(cardRect.y - haloReach))
      var right = Math.min(screenWidth, Math.ceil(cardRect.x + cardRect.width + haloReach))
      var bottom = Math.min(screenHeight, Math.ceil(cardRect.y + cardRect.height + haloReach))
      return Qt.rect(x, y, Math.max(1, right - x), Math.max(1, bottom - y))
    }
    var w = Math.min(Style.space(258), screenWidth)
    var h = Math.min(Style.space(164), screenHeight)
    var position = bar ? bar.position : "top"
    var strip = barWindow ? ((position === "top" || position === "bottom") ? barWindow.height : barWindow.width) : 0
    var inset = Style.space(16)
    var left = position === "left" ? strip + inset : screenWidth - w - inset
    var top = position === "bottom" ? screenHeight - strip - h - inset : position === "top" ? strip + inset : screenHeight - h - inset
    return Qt.rect(Math.round(Math.max(0, Math.min(left, screenWidth - w))),
                   Math.round(Math.max(0, Math.min(top, screenHeight - h))), Math.round(w), Math.round(h))
  }
  readonly property rect protectedRect: panelVisible
    ? Qt.rect(cardRect.x - surfaceRect.x - safetyInset, cardRect.y - surfaceRect.y - safetyInset,
              cardRect.width + safetyInset * 2, cardRect.height + safetyInset * 2)
    : Qt.rect(0, 0, 0, 0)

  function project() {
    var grouped = ({})
    var result = []
    for (var i = 0; i < activities.length; i++) {
      var t = activities[i]
      var medium = ["lan", "bluetooth", "hotspot"].indexOf(t.transport) >= 0 ? t.transport : "unknown"
      var stopping = !!(t.id && cancellingTransfers && cancellingTransfers[t.id])
      var state = stopping ? "stopping" : t.state === "transferring"
        ? (t.direction === "inbound" ? "receiving" : "sending") : "preparing"
      var key = t.direction + ":" + medium + ":" + state
      if (!grouped[key]) {
        grouped[key] = { key: key, direction: t.direction, transport: medium, mode: state, count: 0 }
        result.push(grouped[key])
      }
      grouped[key].count++
    }
    // At most 24 direction/medium/stage groups; no transfer is discarded.
    result.sort(function(a, b) { return a.key < b.key ? -1 : a.key > b.key ? 1 : 0 })
    return result
  }

  onRunningChanged: if (!running) animationFrame = 0
  Timer {
    interval: 33
    repeat: true
    running: root.animating
    onTriggered: root.animationFrame = (root.animationFrame + 1) % 108000
  }

  PanelWindow {
    id: effectWindow
    screen: root.targetScreen
    visible: root.running
    color: "transparent"
    exclusionMode: ExclusionMode.Ignore
    anchors { top: true; left: true }
    margins { top: root.surfaceRect.y; left: root.surfaceRect.x }
    implicitWidth: Math.round(root.surfaceRect.width)
    implicitHeight: Math.round(root.surfaceRect.height)
    WlrLayershell.namespace: "omarchy-quickshare-alpha-effects"
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.None
    // An explicit empty input region, not the window's default full region.
    mask: Region { x: 0; y: 0; width: 0; height: 0 }

    TransferEffectsArt {
      anchors.fill: parent
      active: root.running
      halo: root.panelVisible
      protectedRect: root.protectedRect
      projection: root.projection
      clock: root.clock
      previewing: root.previewing
      activityCount: root.activityCount
      language: root.language
    }
  }
}
