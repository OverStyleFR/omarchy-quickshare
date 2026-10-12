#!/usr/bin/env bash
set -euo pipefail

CONFIG_HOME=${XDG_CONFIG_HOME:-$HOME/.config}
DATA_HOME=${XDG_DATA_HOME:-$HOME/.local/share}
PLUGIN_DIR="$CONFIG_HOME/omarchy/plugins/macondo.quickshare"
NAUTILUS_TARGET="$PLUGIN_DIR/nautilus/omarchy_quickshare.py"
NAUTILUS_EXTENSION="$DATA_HOME/nautilus-python/extensions/omarchy_quickshare.py"

plugin_is_owned() {
  [[ -f "$PLUGIN_DIR/.omarchy-quickshare-owned" ]] &&
    cmp -s -- "$PLUGIN_DIR/.omarchy-quickshare-owned" <(printf 'omarchy-quickshare\n')
}
if (( EUID == 0 )); then
  printf 'Ejecuta como el usuario del escritorio, no como root.\n' >&2
  exit 1
fi
if [[ -L "$PLUGIN_DIR" ]] || { [[ -e "$PLUGIN_DIR" ]] && ! plugin_is_owned; }; then
  printf 'No se retirará un plugin ajeno al instalador: %s\n' "$PLUGIN_DIR" >&2
  exit 1
fi
if [[ ${1:-} != --yes ]]; then
  printf 'Retirar servicio, binario y plugin propios; conservar configuración y descargas. ¿Continuar? [s/N] '
  read -r answer
  [[ "$answer" == s || "$answer" == S ]] || exit 0
fi
# Preserve inline preferences in shell.json: native disable removes the entire
# layout entry, including its language. Restarting after removing the owned
# plugin unloads its widget and IPC without deleting the saved settings.
systemctl --user disable --now omarchy-quickshare.service
if [[ -L "$NAUTILUS_EXTENSION" && $(readlink -- "$NAUTILUS_EXTENSION") == "$NAUTILUS_TARGET" ]]; then
  rm -f -- "$NAUTILUS_EXTENSION"
fi
if plugin_is_owned; then
  rm -rf -- "$PLUGIN_DIR"
fi
rm -f -- "$CONFIG_HOME/systemd/user/omarchy-quickshare.service" "$HOME/.local/bin/omarchy-quickshare"
rm -f -- "$CONFIG_HOME/systemd/user/omarchy-quickshare.service.d/binary.conf"
rm -f -- "$DATA_HOME/licenses/omarchy-quickshare/LICENSE"
rm -f -- "$DATA_HOME/licenses/omarchy-quickshare/LICENSE-GPL"
rm -f -- "$DATA_HOME/licenses/omarchy-quickshare/LICENSE-APACHE"
rm -f -- "$DATA_HOME/licenses/omarchy-quickshare/LICENSE-CHROMIUM"
rm -f -- "$DATA_HOME/licenses/omarchy-quickshare/NOTICE"
rm -f -- "$DATA_HOME/licenses/omarchy-quickshare/THIRD_PARTY_NOTICES.txt"
systemctl --user daemon-reload
omarchy restart shell
printf 'Retirado. Configuración, descargas y copias anteriores del plugin conservadas.\n'
