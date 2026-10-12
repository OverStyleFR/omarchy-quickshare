"""Native Nautilus selection hand-off to the already running Omarchy shell."""

import json
import os
from pathlib import Path

import gi

gi.require_version("Nautilus", "4.1")
gi.require_version("Gtk", "4.0")
from gi.repository import Gio, GLib, GObject, Gtk, Nautilus


class OmarchyQuickShare(GObject.GObject, Nautilus.MenuProvider):
    def __init__(self):
        super().__init__()
        config = Path(os.environ.get("XDG_CONFIG_HOME") or Path.home() / ".config")
        data = Path(os.environ.get("XDG_DATA_HOME") or Path.home() / ".local/share")
        self._shell_config = config / "omarchy/shell.json"
        self._plugin = config / "omarchy/plugins/macondo.quickshare"
        self._target = self._plugin / "nautilus/omarchy_quickshare.py"
        self._registration = data / "nautilus-python/extensions/omarchy_quickshare.py"
        self._monitors = []
        self._watch_error = None
        self._item = None
        # Nautilus keeps providers loaded after unlinking their Python module.
        # Invalidate its cached menu as well as checking ownership on every call.
        for directory in (self._plugin.parent, self._registration.parent, self._shell_config.parent):
            try:
                monitor = Gio.File.new_for_path(str(directory)).monitor_directory(
                    Gio.FileMonitorFlags.WATCH_MOVES, None
                )
                monitor.connect("changed", self._integration_changed)
                self._monitors.append(monitor)
            except GLib.Error as error:
                self._watch_error = str(error)
                self._show_error(self._tr("Cannot monitor the Nautilus integration: ", "No se pudo vigilar la integración Nautilus: ") + str(error))

    def _tr(self, english, spanish):
        try:
            config = json.loads(self._shell_config.read_text())
            layout = config.get("bar", {}).get("layout", {})
            entries = [
                *layout.get("left", []),
                *layout.get("center", []),
                *layout.get("right", []),
                *config.get("plugins", []),
            ]
            for entry in entries:
                if isinstance(entry, dict) and entry.get("id") == "macondo.quickshare":
                    return spanish if entry.get("language") == "es" else english
        except (OSError, ValueError, AttributeError, TypeError):
            pass  # Missing preferences use the plugin's English default.
        return english

    def _registered(self):
        if self._watch_error is not None:
            return False
        try:
            return (
                self._registration.is_symlink()
                and os.readlink(self._registration) == str(self._target)
                and (self._plugin / ".omarchy-quickshare-owned").read_bytes()
                == b"omarchy-quickshare\n"
                and self._target.is_file()
            )
        except OSError:
            return False

    def _integration_changed(self, monitor, file, other_file, event):
        if self._item is not None and not self._registered():
            self._item.set_property("sensitive", False)
        self.emit_items_updated_signal()

    def get_file_items(self, files):
        if not files or not self._registered():
            return []
        paths = []
        for file in files:
            if file.is_gone() or file.get_uri_scheme() != "file":
                return []
            location = file.get_location()
            path = location.get_path() if location.is_native() else None
            if path is None or not os.path.isfile(path):
                return []
            paths.append(path)
        item = Nautilus.MenuItem(
            name="OmarchyQuickShare::send",
            label=self._tr("Send via Quick Share", "Enviar por Quick Share"),
            tip=self._tr("Choose a device in the Omarchy Quick Share panel", "Elegir un dispositivo en el panel Quick Share de Omarchy"),
            icon="document-send-symbolic",
        )
        item.connect("activate", self._activate, tuple(paths))
        self._item = item
        return [item]

    def _show_error(self, detail):
        dialog = Gtk.AlertDialog(
            message=self._tr("Cannot open Quick Share", "No se pudo abrir Quick Share"),
            detail=detail,
            modal=True,
        )
        application = Gio.Application.get_default()
        parent = application.get_active_window() if isinstance(application, Gtk.Application) else None
        dialog.show(parent)

    def _activate(self, item, paths):
        if not self._registered():
            self._show_error(self._tr("The Quick Share integration is no longer installed.", "La integración Quick Share ya no está instalada."))
            return
        local_binary = Path.home() / ".local/bin/omarchy-quickshare"
        binary = str(local_binary) if os.access(local_binary, os.X_OK) else "/usr/bin/omarchy-quickshare"
        try:
            process = Gio.Subprocess.new(
                [binary, "share", "--", *paths],
                Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_PIPE,
            )
            process.communicate_utf8_async(None, None, self._completed, None)
        except GLib.Error as error:
            self._show_error(str(error))

    def _completed(self, process, result, user_data):
        try:
            success, stdout, stderr = process.communicate_utf8_finish(result)
            if not success or not process.get_successful():
                self._show_error((stderr or stdout or self._tr("The Quick Share command failed.", "El comando Quick Share falló.")).strip())
        except GLib.Error as error:
            self._show_error(str(error))
