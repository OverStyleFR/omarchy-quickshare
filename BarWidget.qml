import QtQuick
import qs.Ui as Ui
import qs.Commons

Ui.BarIconButton {
  id: root
  property var shell: bar ? bar.shell : null
  property var manifest: null
  property var settings: ({})
  readonly property var service: shell ? shell.serviceFor("macondo.quickshare") : null
  readonly property bool incoming: service && service.transfers.some(function(t) {
    return t.direction === "inbound" && t.state === "waiting_acceptance"
  })
  readonly property bool awaitingPin: service && service.transfers.some(function(t) {
    return t.direction === "outbound" && t.state === "waiting_acceptance"
  })
  readonly property int activeTransfers: service ? service.transfers.filter(function(t) {
    return ["completed", "rejected", "cancelled", "failed"].indexOf(t.state) === -1
  }).length : 0
  function tr(en, es) { return service ? service.tr(en, es) : en }
  readonly property string statusText: !service || !service.online ? tr("Quick Share: service disconnected", "Quick Share: servicio desconectado") :
    incoming ? tr("Quick Share: incoming request · check the PIN", "Quick Share: solicitud recibida · comprueba el PIN") :
    awaitingPin ? tr("Quick Share: confirm the PIN", "Quick Share: pendiente de confirmar el PIN") :
    service.errorText !== "" ? tr("Quick Share: check the errors", "Quick Share: revisa los errores") :
    activeTransfers > 0 ? "Quick Share: " + activeTransfers + (activeTransfers === 1 ? tr(" active transfer", " transferencia activa") : tr(" active transfers", " transferencias activas")) :
    service.discoverable ? tr("Quick Share: visible", "Quick Share: visible") + " · " + service.remainingSeconds + " s" : tr("Quick Share: visibility off", "Quick Share: visibilidad desactivada")
  Accessible.role: Accessible.Button
  Accessible.name: statusText
  text: "󰒗"
  foreground: incoming || awaitingPin ? (bar ? bar.urgent : Color.urgent) : (bar ? bar.foreground : Color.foreground)
  opacity: service && service.online ? 1 : 0.5
  onPressed: function(buttonCode) {
    if (buttonCode === Qt.RightButton && service) service.refresh()
    else if (shell) {
      if (service) {
        service.panelBar = root.bar
        service.panelAnchor = root
      }
      shell.toggle("macondo.quickshare", "{}")
    }
  }
  HoverHandler {
    onHoveredChanged: {
      if (!root.bar) return
      if (hovered) root.bar.showTooltip(root, root.statusText)
      else root.bar.hideTooltip(root)
    }
  }
}
