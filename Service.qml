import QtQuick
import Quickshell
import Quickshell.Io

Item {
  id: root
  property var shell: null
  property QtObject panelBar: null
  property Item panelAnchor: null
  property var manifest: null
  property var snapshot: null
  property bool online: false
  property int generation: 0
  property bool watchActive: false
  property int retrySeconds: 1
  property var cancellingTransfers: ({})
  property int pendingCommands: 0
  property bool picking: false
  property var selectedFiles: []
  property var localIssue: null
  readonly property string localError: localIssue ? tr(localIssue.en, localIssue.es) + (localIssue.detail ? "\n" + localIssue.detail : "") : ""
  property var entrySettings: ({})
  readonly property string language: entrySettings.language === "es" ? "es" : "en"
  property string watchError: ""
  property string dismissedBackendErrors: ""
  property double now: Date.now() / 1000
  readonly property bool busy: pendingCommands > 0
  readonly property var devices: snapshot ? snapshot.devices : []
  readonly property var transfers: snapshot ? snapshot.transfers : []
  readonly property bool discoverable: online && snapshot && snapshot.visible &&
    (snapshot.visibility_expires_at === null || snapshot.visibility_expires_at > now)
  readonly property int remainingSeconds: discoverable && snapshot.visibility_expires_at !== null
    ? Math.max(0, Math.ceil(snapshot.visibility_expires_at - now)) : 0
  readonly property string backendErrors: snapshot ? (snapshot.errors || []).map(function(e) {
    return typeof e === "string" ? e : JSON.stringify(e)
  }).join("\n") : ""
  readonly property string errorText: [localError, !online ? (watchError || tr("Service disconnected; reconnecting…", "Servicio desconectado; reconectando…")) : watchError, backendErrors !== dismissedBackendErrors ? backendErrors : ""].filter(function(s) { return s.length > 0 }).join("\n")

  function tr(en, es) { return language === "es" ? es : en }
  function fail(en, es, detail) { localIssue = { en: en, es: es, detail: detail || "" } }
  function loadSettings(raw) {
    try {
      var config = JSON.parse(raw)
      var layout = config.bar && config.bar.layout ? config.bar.layout : {}
      var entries = (layout.left || []).concat(layout.center || [], layout.right || [], config.plugins || [])
      entrySettings = entries.find(function(entry) { return entry && entry.id === "macondo.quickshare" }) || {}
    } catch (e) {
      fail("Cannot read the language preference.", "No se pudo leer la preferencia de idioma.", e.message)
    }
  }
  function setLanguage(value) {
    if (value !== "en" && value !== "es" || value === language) return
    var next = Object.assign({}, entrySettings, { language: value })
    if (!shell || !shell.updateEntryInline("macondo.quickshare", next))
      fail("Cannot save the language preference.", "No se pudo guardar la preferencia de idioma.")
  }

  function consume(line, sourceGeneration) {
    if (sourceGeneration !== undefined && sourceGeneration !== generation) return
    if (!line.trim()) return
    try {
      var next = JSON.parse(line)
      if (next.version !== 1 || typeof next.instance_id !== "string" || !Number.isSafeInteger(next.revision) ||
          !Array.isArray(next.devices) || !Array.isArray(next.transfers))
        throw new Error(tr("Incompatible snapshot format", "Formato de snapshot incompatible"))
      if (snapshot && next.instance_id === snapshot.instance_id && next.revision <= snapshot.revision) return
      var remaining = {}
      if (snapshot && next.instance_id === snapshot.instance_id) {
        next.transfers.forEach(function(transfer) {
          if (root.cancellingTransfers[transfer.id] &&
              ["completed", "rejected", "cancelled", "failed"].indexOf(transfer.state) === -1)
            remaining[transfer.id] = true
        })
      }
      cancellingTransfers = remaining
      snapshot = next
      online = true
      watchError = ""
      retrySeconds = 1
    } catch (e) {
      fail("Cannot read the service state.", "No se pudo leer el estado del servicio.", e.message)
    }
  }
  function clearErrors() {
    localIssue = null
    watchError = ""
    dismissedBackendErrors = backendErrors
  }
  function invoke(args, kind, transferId) {
    var task = commandComponent.createObject(root, {
      command: ["/usr/bin/env", "omarchy-quickshare"].concat(args), purpose: kind || "action",
      sourceGeneration: generation, transferId: transferId || ""
    })
    if (kind !== "picker") pendingCommands++
    task.running = true
  }
  function refresh() { invoke(["status"], "snapshot") }
  function setDiscoverable(enabled) { invoke(["visible", enabled ? "120" : "0"]) }
  function consent(enabled) { invoke(["network-consent", enabled ? "on" : "off"]) }
  function downloadDirectory(path) {
    if (!path || path[0] !== "/") { fail("Choose an absolute folder path.", "Elige una ruta absoluta de carpeta."); return }
    invoke(["download-dir", path])
  }
  function setSelectedFiles(files) {
    if (!Array.isArray(files) || files.length === 0 ||
        files.some(function(path) { return typeof path !== "string" || path.length === 0 || path[0] !== "/" })) {
      fail("Choose one or more files with absolute paths.", "Selecciona uno o varios archivos con rutas absolutas.")
      return false
    }
    selectedFiles = files.slice()
    return true
  }
  function pickFiles() {
    if (picking) return
    picking = true
    invoke(["pick-files"], "picker")
  }
  function send(peer) {
    if (!online || busy || !peer || selectedFiles.length === 0) return
    invoke(["send", peer].concat(selectedFiles), "send")
  }
  function respond(id, accept) { invoke([accept ? "accept" : "reject", id]) }
  function cancellationRequested(id) { return !!cancellingTransfers[id] }
  function clearCancellationRequest(id) {
    var next = Object.assign({}, cancellingTransfers)
    delete next[id]
    cancellingTransfers = next
  }
  function cancel(id) {
    if (!online || cancellationRequested(id)) return
    cancellingTransfers = Object.assign({}, cancellingTransfers, { [id]: true })
    invoke(["cancel", id], "cancel", id)
  }
  function disconnected(message) {
    if (watchActive) { generation++; watchActive = false }
    online = false
    if (!watchError && message) watchError = message
    if (!reconnect.running) {
      reconnect.restart()
      retrySeconds = Math.min(30, retrySeconds * 2)
    }
  }

  Component.onCompleted: watch.running = true
  Timer {
    interval: 1000
    repeat: true
    running: true
    onTriggered: root.now = Date.now() / 1000
  }
  Timer {
    id: reconnect
    interval: root.retrySeconds * 1000
    onTriggered: watch.running = true
  }
  FileView {
    path: (Quickshell.env("XDG_CONFIG_HOME") || Quickshell.env("HOME") + "/.config") + "/omarchy/shell.json"
    watchChanges: true
    printErrors: false
    onLoaded: root.loadSettings(text())
    onFileChanged: reload()
  }
  Process {
    id: watch
    // env turns a missing CLI into exit 127, which Process reports via exited.
    command: ["/usr/bin/env", "omarchy-quickshare", "watch"]
    onStarted: {
      root.generation++
      root.watchActive = true
      root.snapshot = null
    }
    stdout: SplitParser { onRead: line => root.consume(line) }
    stderr: SplitParser { onRead: line => { if (line.trim()) root.watchError = line.trim() } }
    onExited: function(exitCode, exitStatus) { root.disconnected("") }
  }
  Component {
    id: commandComponent
    Process {
      id: task
      property string purpose: "action"
      property int sourceGeneration: 0
      property string transferId: ""
      stdout: StdioCollector { id: output; waitForEnd: true }
      stderr: StdioCollector { id: errors; waitForEnd: true }
      onExited: function(exitCode, exitStatus) { finish(exitCode, exitStatus) }
      function finish(exitCode, exitStatus) {
        if (purpose !== "picker") root.pendingCommands--
        if (purpose === "picker") root.picking = false
        if (exitCode !== 0 || exitStatus !== 0) {
          if (purpose === "cancel") root.clearCancellationRequest(transferId)
          root.fail("The command could not be completed (" + exitCode + ").", "El comando no se pudo completar (" + exitCode + ").", errors.text.trim())
        } else {
          if (errors.text.trim()) root.fail("Service details", "Detalles del servicio", errors.text.trim())
          if (purpose === "snapshot") root.consume(output.text, sourceGeneration)
          else if (purpose === "picker") {
            try {
              var files = JSON.parse(output.text)
              // An empty portal result means cancellation, not a new selection.
              if (!Array.isArray(files) || files.length > 0) root.setSelectedFiles(files)
            } catch (e) { root.fail("Cannot read the file picker result.", "No se pudo leer el resultado del selector de archivos.", e.message) }
          } else if (purpose === "send") root.selectedFiles = []
        }
        Qt.callLater(function() { task.destroy() })
      }
    }
  }
}
