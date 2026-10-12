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
  readonly property string language: entrySettings.language === "es" ? "es" : entrySettings.language === "en" ? "en" : savedLanguage
  property string watchError: ""
  property string savedLanguage: "en"
  property bool backendInstalled: false
  property bool backendCompatible: false
  property string backendBinary: ""
  property bool serviceDisabled: false
  property bool disabledLoaded: false
  property double setupAttempt: 0
  property var setupNotice: null
  readonly property bool setupBusy: setupAttempt > 0
  readonly property string setupMessage: setupNotice ? tr(setupNotice.en, setupNotice.es) : ""
  readonly property string configHome: Quickshell.env("XDG_CONFIG_HOME") || Quickshell.env("HOME") + "/.config"
  readonly property string serviceState: (Quickshell.env("XDG_STATE_HOME") || Quickshell.env("HOME") + "/.local/state") + "/omarchy-quickshare/service"
  readonly property string setupScript: decodeURIComponent(String(Qt.resolvedUrl("scripts/setup-service.sh")).replace(/^file:\/\//, ""))
  readonly property bool activeTransfers: transfers.some(function(transfer) {
    return ["completed", "rejected", "cancelled", "failed"].indexOf(transfer.state) === -1
  })
  property string dismissedBackendErrors: ""
  property double now: Date.now() / 1000
  readonly property bool busy: pendingCommands > 0 || setupBusy
  readonly property var devices: snapshot ? snapshot.devices : []
  readonly property var transfers: snapshot ? snapshot.transfers : []
  readonly property bool discoverable: online && snapshot && snapshot.visible &&
    (snapshot.visibility_expires_at === null || snapshot.visibility_expires_at > now)
  readonly property int remainingSeconds: discoverable && snapshot.visibility_expires_at !== null
    ? Math.max(0, Math.ceil(snapshot.visibility_expires_at - now)) : 0
  readonly property string backendErrors: snapshot ? (snapshot.errors || []).map(function(e) {
    return typeof e === "string" ? e : JSON.stringify(e)
  }).join("\n") : ""
  readonly property string errorText: [localError, !online && backendInstalled && backendCompatible && !serviceDisabled && !setupBusy ? (watchError || tr("Service unavailable. Use Install service to repair setup, or review its terminal output.", "Servicio no disponible. Usa Instalar servicio para reparar la instalación o revisa su terminal.")) : "", online ? watchError : "", backendErrors !== dismissedBackendErrors ? backendErrors : ""].filter(function(s) { return s.length > 0 }).join("\n")

  function tr(en, es) { return language === "es" ? es : en }
  function fail(en, es, detail) { localIssue = { en: en, es: es, detail: detail || "" } }
  function loadSettings(raw) {
    try {
      var config = JSON.parse(raw)
      var layout = config.bar && config.bar.layout ? config.bar.layout : {}
      var entries = (layout.left || []).concat(layout.center || [], layout.right || [], config.plugins || [])
      entrySettings = entries.find(function(entry) { return entry && entry.id === "macondo.quickshare" }) || {}
      if (entrySettings.language === "en" || entrySettings.language === "es") persistLanguage(entrySettings.language)
    } catch (e) {
      fail("Cannot read the language preference.", "No se pudo leer la preferencia de idioma.", e.message)
    }
  }
  function persistLanguage(value) {
    if (value === savedLanguage) return
    var task = commandComponent.createObject(root, {
      command: ["/usr/bin/env", "mkdir", "-p", configHome + "/omarchy-quickshare"],
      purpose: "language", preference: value
    })
    pendingCommands++
    task.running = true
  }
  function setLanguage(value) {
    if (value !== "en" && value !== "es" || value === language) return
    var next = Object.assign({}, entrySettings, { language: value })
    if (!shell || !shell.updateEntryInline("macondo.quickshare", next)) {
      fail("Cannot save the language preference.", "No se pudo guardar la preferencia de idioma.")
      return
    }
    persistLanguage(value)
  }
  function probeBackend() { if (!backendProbe.running) backendProbe.running = true }
  function runSetup(action) {
    if (["install", "update", "remove"].indexOf(action) === -1 || busy) return
    if (activeTransfers) {
      fail("Finish or cancel transfers before maintaining the service.", "Termina o cancela las transferencias antes de mantener el servicio.")
      return
    }
    setupAttempt = Date.now()
    setupNotice = { en: "Authorize maintenance in the Omarchy terminal. Preferences and received files are preserved.", es: "Autoriza el mantenimiento en la terminal de Omarchy. Se conservan preferencias y archivos recibidos." }
    reconnect.stop()
    watch.running = false
    online = false
    setupLauncher.command = ["omarchy", "launch", "tui", "--app-id=io.omarchy.quickshare.setup", "bash", setupScript, action, language]
    setupLauncher.running = true
  }
  function consumeSetup(raw) {
    try {
      var receipt = JSON.parse(raw)
      if (!setupBusy || receipt.time < setupAttempt) return
      setupAttempt = 0
      if (receipt.outcome === "installed")
        setupNotice = { en: "Service installed and running.", es: "Servicio instalado y activo." }
      else if (receipt.outcome === "removed")
        setupNotice = { en: "Backend and Nautilus action removed. The native plugin, preferences and files are preserved.", es: "Servicio y acción Nautilus retirados. Se conservan el plugin nativo, las preferencias y los archivos." }
      else if (receipt.outcome === "retained")
        setupNotice = { en: "Service disabled; Nautilus action removed. The system package is retained, not uninstalled.", es: "Servicio desactivado; acción Nautilus retirada. El paquete del sistema se conserva, no está desinstalado." }
      else
        setupNotice = { en: "Maintenance did not complete. Review the actual error in the terminal and retry after resolving it.", es: "El mantenimiento no terminó. Revisa el error real en la terminal y reintenta después de resolverlo." }
      disabledFile.reload()
      probeBackend()
    } catch (e) {
      fail("Cannot read the maintenance result.", "No se pudo leer el resultado de mantenimiento.", e.message)
    }
  }
  onServiceDisabledChanged: {
    if (serviceDisabled) { reconnect.stop(); watch.running = false; online = false }
    else if (!setupBusy) probeBackend()
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
    if (serviceDisabled || !backendInstalled) {
      fail("Install service before using this action.", "Instala el servicio antes de usar esta acción.")
      return
    }
    var task = commandComponent.createObject(root, {
      command: ["/usr/bin/env", backendBinary].concat(args), purpose: kind || "action",
      sourceGeneration: generation, transferId: transferId || ""
    })
    if (kind !== "picker") pendingCommands++
    task.running = true
  }
  function refresh() {
    if (setupBusy && !setupLauncher.running) {
      setupAttempt = 0
      setupNotice = { en: "Review the maintenance terminal for the result. No success has been assumed.", es: "Revisa la terminal de mantenimiento para conocer el resultado. No se ha supuesto éxito." }
    }
    disabledFile.reload()
    probeBackend()
    if (online) invoke(["status"], "snapshot")
  }
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
    if (picking || serviceDisabled || !backendInstalled) return
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
    if (backendInstalled && !serviceDisabled && !setupBusy && !reconnect.running) {
      reconnect.restart()
      retrySeconds = Math.min(30, retrySeconds * 2)
    }
  }

  Component.onCompleted: probeBackend()
  Timer {
    interval: 1000
    repeat: true
    running: true
    onTriggered: root.now = Date.now() / 1000
  }
  Timer {
    id: reconnect
    interval: root.retrySeconds * 1000
    onTriggered: root.probeBackend()
  }
  Timer {
    interval: 1000
    repeat: true
    running: root.setupBusy
    onTriggered: { setupReceipt.reload(); disabledFile.reload() }
  }
  Process {
    id: backendProbe
    command: ["bash", "-c", "binary=''; if [[ -x /usr/bin/omarchy-quickshare ]]; then binary=/usr/bin/omarchy-quickshare; elif [[ -x \"$1/.local/bin/omarchy-quickshare\" ]]; then binary=\"$1/.local/bin/omarchy-quickshare\"; fi; compatible=false; if [[ -n $binary ]]; then version=$(\"$binary\" --version) || exit 1; version=${version##* }; if [[ $version =~ ^[0-9]+\\.[0-9]+\\.[0-9]+$ ]] && [[ $(vercmp \"$version\" 0.1.1) -ge 0 ]]; then compatible=true; fi; fi; jq -cn --arg binary \"$binary\" --argjson compatible \"$compatible\" '{binary:$binary,compatible:$compatible}'", "bash", Quickshell.env("HOME")]
    stdout: StdioCollector { id: backendOutput; waitForEnd: true }
    onExited: function(code) {
      var result = {}
      try { if (code === 0) result = JSON.parse(backendOutput.text) } catch (e) {}
      root.backendBinary = result.binary || ""
      root.backendInstalled = root.backendBinary !== ""
      root.backendCompatible = result.compatible === true
      if (root.backendInstalled && root.backendCompatible && root.disabledLoaded && !root.serviceDisabled && !root.setupBusy && !watch.running) {
        watch.command = ["/usr/bin/env", root.backendBinary, "watch"]
        watch.running = true
      }
    }
  }
  Process {
    id: setupLauncher
    onExited: function(code) {
      if (code !== 0) {
        root.setupAttempt = 0
        root.fail("Cannot open the Omarchy setup terminal.", "No se pudo abrir la terminal de instalación de Omarchy.")
        disabledFile.reload()
        root.probeBackend()
      }
    }
  }
  FileView {
    id: disabledFile
    path: root.serviceState + "/disabled"
    watchChanges: true
    printErrors: false
    onLoaded: { root.serviceDisabled = text().trim() === "disabled"; root.disabledLoaded = true; root.probeBackend() }
    onLoadFailed: { root.serviceDisabled = false; root.disabledLoaded = true; root.probeBackend() }
    onFileChanged: reload()
  }
  FileView {
    id: setupReceipt
    path: root.serviceState + "/setup-result.json"
    watchChanges: true
    printErrors: false
    onLoaded: root.consumeSetup(text())
    onFileChanged: reload()
  }
  FileView {
    id: languageFile
    path: root.configHome + "/omarchy-quickshare/language"
    atomicWrites: true
    watchChanges: true
    printErrors: false
    onLoaded: {
      root.savedLanguage = text().trim() === "es" ? "es" : "en"
      if (root.entrySettings.language === "en" || root.entrySettings.language === "es")
        root.persistLanguage(root.entrySettings.language)
    }
    onFileChanged: reload()
  }
  FileView {
    path: Quickshell.env("HOME") + "/.config/omarchy/shell.json"
    watchChanges: true
    printErrors: false
    onLoaded: root.loadSettings(text())
    onFileChanged: reload()
  }
  Process {
    id: watch
    // Absolute backend selection avoids a stale graphical-session PATH or a shadow binary.
    command: []
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
      property string preference: ""
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
          if (purpose === "language") {
            try {
              languageFile.setText(preference + "\n")
              root.savedLanguage = preference
            } catch (e) {
              root.fail("Cannot persist the language preference.", "No se pudo conservar la preferencia de idioma.", e.message)
            }
          }
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
