import QtQuick
import qs.Commons

// Four narrow textures around the protected card, or one small courier texture.
// Nothing is painted inside the card + safety margin, including bloom/trails.
Item {
  id: root
  property bool active: false
  property bool halo: false
  property rect protectedRect: Qt.rect(0, 0, 0, 0)
  property var projection: []
  property real clock: 0
  property bool previewing: false
  property int activityCount: 0
  property string language: "en"
  readonly property color ink: Color.popups.text
  readonly property color accent: Color.accent
  readonly property color soft: Color.muted
  readonly property color paper: Color.popups.background
  readonly property real unit: Style.space(1)
  readonly property var bands: {
    if (!halo) return [Qt.rect(0, 0, width, height)]
    var p = protectedRect
    var top = Math.max(0, Math.min(height, p.y))
    var bottom = Math.max(top, Math.min(height, p.y + p.height))
    var left = Math.max(0, Math.min(width, p.x))
    var right = Math.max(left, Math.min(width, p.x + p.width))
    return [Qt.rect(0, 0, width, top), Qt.rect(0, bottom, width, height - bottom),
            Qt.rect(0, top, left, bottom - top), Qt.rect(right, top, width - right, bottom - top)]
  }
  readonly property string caption: {
    var seen = ({})
    var labels = []
    var names = language === "es"
      ? { preparing: "Preparando", sending: "Enviando", receiving: "Recibiendo", stopping: "Deteniendo" }
      : { preparing: "Preparing", sending: "Sending", receiving: "Receiving", stopping: "Stopping" }
    for (var i = 0; i < projection.length; i++) {
      var key = projection[i].mode
      if (!seen[key]) { seen[key] = true; labels.push(names[key]) }
    }
    return labels.join(" · ")
  }
  readonly property string mediumCaption: {
    var seen = ({})
    var labels = []
    var names = { lan: "Wi-Fi / LAN", bluetooth: "Bluetooth", hotspot: language === "es" ? "Wi-Fi directo" : "Direct Wi-Fi", unknown: language === "es" ? "Sin medio confirmado" : "Transport unconfirmed" }
    for (var i = 0; i < projection.length; i++) {
      var key = projection[i].transport
      if (!seen[key]) { seen[key] = true; labels.push(names[key]) }
    }
    return labels.join(" · ")
  }

  function hash(value) {
    var n = 0
    for (var i = 0; i < value.length; i++) n = (n * 31 + value.charCodeAt(i)) >>> 0
    return (n % 1000) / 1000
  }
  function tint(color, alpha) { return Qt.rgba(color.r, color.g, color.b, alpha) }
  function line(ctx, points, color, width, opacity) {
    ctx.beginPath()
    ctx.moveTo(points[0][0], points[0][1])
    for (var i = 1; i < points.length; i++) ctx.lineTo(points[i][0], points[i][1])
    ctx.strokeStyle = tint(color, opacity)
    ctx.lineWidth = width * unit
    ctx.lineCap = "round"
    ctx.lineJoin = "round"
    ctx.stroke()
  }
  function circle(ctx, x, y, radius, color, alpha, filled) {
    ctx.beginPath()
    ctx.arc(x, y, radius, 0, Math.PI * 2)
    if (filled) { ctx.fillStyle = tint(color, alpha); ctx.fill() }
    else { ctx.strokeStyle = tint(color, alpha); ctx.lineWidth = unit; ctx.stroke() }
  }
  function track(ctx, rect, color, alpha, thickness) {
    var r = Math.min(18 * unit, rect.width / 2, rect.height / 2)
    ctx.beginPath()
    ctx.moveTo(rect.x + r, rect.y)
    ctx.arcTo(rect.x + rect.width, rect.y, rect.x + rect.width, rect.y + rect.height, r)
    ctx.arcTo(rect.x + rect.width, rect.y + rect.height, rect.x, rect.y + rect.height, r)
    ctx.arcTo(rect.x, rect.y + rect.height, rect.x, rect.y, r)
    ctx.arcTo(rect.x, rect.y, rect.x + rect.width, rect.y, r)
    ctx.closePath()
    ctx.strokeStyle = tint(color, alpha)
    ctx.lineWidth = thickness * unit
    ctx.stroke()
  }
  function route(position, lane) {
    var phase = ((position % 1) + 1) % 1
    if (!halo) {
      var angle = phase * Math.PI * 2
      return { x: width / 2 + Math.cos(angle) * (78 + lane * 2) * unit,
               y: 66 * unit + Math.sin(angle) * (27 + lane) * unit,
               angle: Math.atan2(Math.cos(angle) * (27 + lane), -Math.sin(angle) * (78 + lane * 2)) }
    }
    var p = protectedRect
    var offset = (9 + lane * 4) * unit
    var x = p.x - offset, y = p.y - offset
    var w = p.width + offset * 2, h = p.height + offset * 2
    var distance = phase * (w + h) * 2
    if (distance < w) return { x: x + distance, y: y, angle: 0 }
    distance -= w
    if (distance < h) return { x: x + w, y: y + distance, angle: Math.PI / 2 }
    distance -= h
    if (distance < w) return { x: x + w - distance, y: y + h, angle: Math.PI }
    return { x: x, y: y + h - (distance - w), angle: -Math.PI / 2 }
  }
  function packet(ctx, point, color, inbound, alpha) {
    ctx.save()
    ctx.translate(point.x, point.y)
    ctx.rotate(point.angle + (inbound ? Math.PI : 0))
    // Original folded paper courier; its split tail gives a directional silhouette.
    ctx.beginPath()
    ctx.moveTo(10 * unit, 0)
    ctx.lineTo(-7 * unit, -6 * unit)
    ctx.lineTo(-3 * unit, 0)
    ctx.lineTo(-7 * unit, 6 * unit)
    ctx.closePath()
    ctx.fillStyle = tint(color, alpha)
    ctx.fill()
    line(ctx, [[-3 * unit, 0], [10 * unit, 0]], paper, 1, alpha * 0.7)
    ctx.restore()
  }
  function transportMark(ctx, x, y, medium, color, alpha) {
    ctx.save()
    ctx.translate(x, y)
    if (medium === "bluetooth") {
      // Paired angular crystals and a short zigzag, not a borrowed logo.
      line(ctx, [[-10 * unit, 0], [-6 * unit, -5 * unit], [-2 * unit, 0], [-6 * unit, 5 * unit], [-10 * unit, 0]], color, 1.5, alpha)
      line(ctx, [[2 * unit, 0], [6 * unit, -5 * unit], [10 * unit, 0], [6 * unit, 5 * unit], [2 * unit, 0]], color, 1.5, alpha)
      line(ctx, [[-2 * unit, 0], [0, -2 * unit], [2 * unit, 0]], color, 1, alpha)
    } else if (medium === "lan" || medium === "hotspot") {
      for (var r = 0; r < 3; r++) {
        ctx.beginPath()
        ctx.arc(0, 4 * unit, (4 + r * 4) * unit, Math.PI * 1.2, Math.PI * 1.8)
        ctx.strokeStyle = tint(color, alpha * (1 - r * 0.17))
        ctx.lineWidth = 1.4 * unit
        ctx.stroke()
      }
      circle(ctx, 0, 3 * unit, 1.5 * unit, color, alpha, true)
      if (medium === "hotspot") {
        circle(ctx, 0, 3 * unit, 8 * unit, color, alpha * 0.45, false)
        circle(ctx, -14 * unit, 3 * unit, 2 * unit, color, alpha, true)
        circle(ctx, 14 * unit, 3 * unit, 2 * unit, color, alpha, true)
      }
    } else {
      circle(ctx, -6 * unit, 0, 2 * unit, color, alpha, false)
      circle(ctx, 6 * unit, 0, 2 * unit, color, alpha, false)
      line(ctx, [[-2 * unit, 0], [2 * unit, 0]], color, 1, alpha * 0.5)
    }
    ctx.restore()
  }
  function drawCompanion(ctx) {
    var cx = width / 2, cy = 66 * unit
    var receiving = projection.some(function(a) { return a.mode === "receiving" })
    var sending = projection.some(function(a) { return a.mode === "sending" })
    var preparation = projection.some(function(a) { return a.mode === "preparing" })
    // A floating folded-page core and comet fins; no character or external asset.
    circle(ctx, cx, cy, 24 * unit, accent, 0.035, true)
    circle(ctx, cx, cy, 19 * unit, accent, 0.1, false)
    if (receiving) {
      for (var j = 0; j < 3; j++) {
        var radius = (23 + ((clock * 12 + j * 9) % 27)) * unit
        circle(ctx, cx, cy, radius, ink, 0.14 * (1 - (radius / unit - 23) / 27), false)
      }
    }
    if (sending) {
      for (var k = 0; k < 3; k++) {
        var stretch = (18 + k * 7 + Math.sin(clock * 3 + k) * 3) * unit
        line(ctx, [[cx - 18 * unit, cy + (k - 1) * 5 * unit], [cx - stretch - 26 * unit, cy + (k - 1) * 9 * unit]], accent, 2 - k * 0.4, 0.32 - k * 0.07)
      }
    }
    var angle = preparation && !sending && !receiving ? Math.sin(clock * 1.6) * 0.18 : -0.25
    ctx.save()
    ctx.translate(cx, cy + Math.sin(clock * 2) * 2 * unit)
    ctx.rotate(angle)
    ctx.beginPath()
    ctx.moveTo(-13 * unit, -16 * unit)
    ctx.lineTo(5 * unit, -16 * unit)
    ctx.lineTo(14 * unit, -7 * unit)
    ctx.lineTo(14 * unit, 16 * unit)
    ctx.lineTo(-13 * unit, 16 * unit)
    ctx.closePath()
    ctx.fillStyle = tint(paper, 0.9)
    ctx.fill()
    ctx.strokeStyle = tint(accent, 0.9)
    ctx.lineWidth = 1.5 * unit
    ctx.stroke()
    line(ctx, [[5 * unit, -16 * unit], [5 * unit, -7 * unit], [14 * unit, -7 * unit]], ink, 1, 0.65)
    for (var n = 0; n < 3; n++) line(ctx, [[-7 * unit, (n * 5 - 2) * unit], [(n === 2 ? 2 : 8) * unit, (n * 5 - 2) * unit]], ink, 1, 0.45)
    ctx.restore()
  }
  function draw(ctx) {
    if (!active || projection.length === 0) return
    if (halo) {
      var p = protectedRect
      track(ctx, Qt.rect(p.x - 5 * unit, p.y - 5 * unit, p.width + 10 * unit, p.height + 10 * unit), accent, 0.055, 6)
      track(ctx, Qt.rect(p.x - 5 * unit, p.y - 5 * unit, p.width + 10 * unit, p.height + 10 * unit), accent, 0.2, 1)
    } else drawCompanion(ctx)
    for (var i = 0; i < projection.length; i++) {
      var a = projection[i]
      var seed = hash(a.key)
      var lane = Math.floor(hash(a.direction + a.transport) * 4)
      var color = a.direction === "inbound" ? ink : accent
      var sign = a.direction === "inbound" ? -1 : 1
      var phase = seed + (a.mode === "stopping" ? 0 : clock * (a.mode === "preparing" ? 0.027 : a.transport === "bluetooth" ? 0.085 : 0.14) * sign)
      var beacon = route(seed, lane)
      transportMark(ctx, beacon.x, beacon.y, a.transport, color, a.mode === "stopping" ? 0.22 : 0.6)
      if (a.mode === "stopping") {
        line(ctx, [[beacon.x - 3 * unit, beacon.y + 10 * unit], [beacon.x - 3 * unit, beacon.y + 15 * unit]], soft, 2, 0.5)
        line(ctx, [[beacon.x + 3 * unit, beacon.y + 10 * unit], [beacon.x + 3 * unit, beacon.y + 15 * unit]], soft, 2, 0.5)
        continue
      }
      for (var particle = 0; particle < 2; particle++) {
        var head = phase + particle * 0.5
        var point = route(head, lane)
        if (a.mode === "preparing") {
          var pulse = (Math.sin(clock * 2.5 + seed * 6) + 1) / 2
          circle(ctx, point.x, point.y, (4 + pulse * 5) * unit, color, 0.15 + pulse * 0.2, false)
          circle(ctx, point.x, point.y, 2 * unit, color, 0.7, true)
          var partner = route(head + 0.012 * sign, lane)
          circle(ctx, partner.x, partner.y, 1.5 * unit, color, 0.45, true)
        } else {
          // Short procedural trails: no persistent framebuffer or stale pixels.
          for (var tail = 5; tail >= 1; tail--) {
            var trail = route(head - sign * tail * 0.0035, lane)
            circle(ctx, trail.x, trail.y, (1.3 + (5 - tail) * 0.22) * unit, color, (6 - tail) * 0.055, true)
          }
          circle(ctx, point.x, point.y, 9 * unit, color, 0.055, true)
          packet(ctx, point, color, a.direction === "inbound", 0.9)
          if (a.mode === "receiving") {
            circle(ctx, point.x, point.y, (10 + Math.sin(clock * 4 + particle) * 2) * unit, color, 0.25, false)
          }
        }
      }
    }
  }

  Repeater {
    model: root.bands
    delegate: Canvas {
      id: strip
      required property var modelData
      x: modelData.x
      y: modelData.y
      width: modelData.width
      height: modelData.height
      visible: root.active && width > 0 && height > 0
      renderTarget: Canvas.Image
      onPaint: {
        var ctx = getContext("2d")
        ctx.setTransform(1, 0, 0, 1, 0, 0)
        ctx.clearRect(0, 0, width, height)
        ctx.save()
        ctx.translate(-x, -y)
        root.draw(ctx)
        ctx.restore()
      }
      onVisibleChanged: requestPaint()
      onWidthChanged: requestPaint()
      onHeightChanged: requestPaint()
      Component.onCompleted: requestPaint()
      Connections {
        target: root
        function onClockChanged() { if (strip.visible) strip.requestPaint() }
        function onProjectionChanged() { strip.requestPaint() }
        function onActiveChanged() { strip.requestPaint() }
        function onProtectedRectChanged() { strip.requestPaint() }
        function onInkChanged() { strip.requestPaint() }
        function onAccentChanged() { strip.requestPaint() }
        function onSoftChanged() { strip.requestPaint() }
        function onPaperChanged() { strip.requestPaint() }
        function onUnitChanged() { strip.requestPaint() }
      }
    }
  }

  Column {
    visible: root.active && !root.halo
    x: Style.space(10)
    y: Style.space(114)
    width: root.width - Style.space(20)
    spacing: Style.space(2)
    Text {
      width: parent.width
      horizontalAlignment: Text.AlignHCenter
      text: (root.previewing ? (root.language === "es" ? "VISTA PREVIA · " : "UI PREVIEW · ") : "") + "ALPHA · " + root.activityCount
      color: root.soft
      font.family: Style.font.family
      font.pixelSize: Style.space(9)
      font.letterSpacing: Style.space(1)
      textFormat: Text.PlainText
      elide: Text.ElideRight
    }
    Text {
      width: parent.width
      horizontalAlignment: Text.AlignHCenter
      text: root.caption
      color: root.ink
      font.family: Style.font.family
      font.pixelSize: Style.space(11)
      textFormat: Text.PlainText
      elide: Text.ElideRight
    }
    Text {
      width: parent.width
      horizontalAlignment: Text.AlignHCenter
      text: root.mediumCaption
      color: root.soft
      font.family: Style.font.family
      font.pixelSize: Style.space(9)
      textFormat: Text.PlainText
      elide: Text.ElideRight
    }
  }
}
