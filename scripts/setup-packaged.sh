#!/usr/bin/env bash
set -euo pipefail

CONFIG_HOME=${XDG_CONFIG_HOME:-$HOME/.config}
DATA_HOME=${XDG_DATA_HOME:-$HOME/.local/share}
PLUGIN_DIR="$CONFIG_HOME/omarchy/plugins/macondo.quickshare"
NAUTILUS_TARGET="$PLUGIN_DIR/nautilus/omarchy_quickshare.py"
NAUTILUS_EXTENSION="$DATA_HOME/nautilus-python/extensions/omarchy_quickshare.py"
SOURCE=/usr/share/omarchy-quickshare/plugin

plugin_is_owned() {
  [[ -f "$PLUGIN_DIR/.omarchy-quickshare-owned" ]] &&
    cmp -s -- "$PLUGIN_DIR/.omarchy-quickshare-owned" <(printf 'omarchy-quickshare\n')
}
if (( EUID == 0 )); then
  printf 'Ejecuta como el usuario del escritorio, no como root.\n' >&2
  exit 1
fi
if [[ -L "$PLUGIN_DIR" ]] || { [[ -e "$PLUGIN_DIR" ]] && ! plugin_is_owned; }; then
  printf 'El destino ya existe y no pertenece a este instalador: %s\n' "$PLUGIN_DIR" >&2
  exit 1
fi
if [[ -e "$NAUTILUS_EXTENSION" || -L "$NAUTILUS_EXTENSION" ]]; then
  if [[ ! -L "$NAUTILUS_EXTENSION" || $(readlink -- "$NAUTILUS_EXTENSION") != "$NAUTILUS_TARGET" ]]; then
    printf 'La extensión Nautilus ya existe y no pertenece a este instalador: %s\n' "$NAUTILUS_EXTENSION" >&2
    exit 1
  fi
fi
for command in omarchy omarchy-shell systemctl install jq python nautilus cmp; do
  command -v "$command" >/dev/null || { printf 'Falta %s\n' "$command" >&2; exit 1; }
done
python -c 'import gi; gi.require_version("Nautilus", "4.1"); gi.require_version("Gtk", "4.0"); from gi.repository import Nautilus, Gtk' || {
  printf 'Instala nautilus, nautilus-python, python-gobject y gtk4 antes de continuar.\n' >&2
  exit 1
}
[[ -f /usr/lib/nautilus/extensions-4/libnautilus-python.so ]] || {
  printf 'Falta nautilus-python (libnautilus-python.so).\n' >&2
  exit 1
}
[[ -x /usr/bin/omarchy-quickshare ]] || { printf 'Falta el cliente del paquete: /usr/bin/omarchy-quickshare\n' >&2; exit 1; }
omarchy plugin validate "$SOURCE"
if systemctl --user is-active --quiet omarchy-quickshare.service; then
  if ! /usr/bin/omarchy-quickshare status | jq -e 'all(.transfers[]; .state == "completed" or .state == "rejected" or .state == "cancelled" or .state == "failed")' >/dev/null; then
    printf 'Termina o cancela las transferencias antes de actualizar.\n' >&2
    exit 1
  fi
  systemctl --user stop omarchy-quickshare.service
fi
UPDATE=false
[[ ! -d "$PLUGIN_DIR" ]] || UPDATE=true
mkdir -p "$CONFIG_HOME/omarchy/plugins" "$CONFIG_HOME/systemd/user/omarchy-quickshare.service.d"
# Staging and backups must remain outside the shell's plugin registry.
STAGE=$(mktemp -d "$CONFIG_HOME/omarchy/.quickshare-stage.XXXXXXXX")
trap 'rm -rf -- "$STAGE"' EXIT
cp -a "$SOURCE/." "$STAGE/"
printf 'omarchy-quickshare\n' > "$STAGE/.omarchy-quickshare-owned"
if [[ -d "$PLUGIN_DIR" ]]; then
  BACKUP="$CONFIG_HOME/omarchy/plugin-backups/macondo.quickshare.$(date +%s).$$"
  mkdir -p "$(dirname "$BACKUP")"
  mv -- "$PLUGIN_DIR" "$BACKUP"
  printf 'Plugin anterior conservado: %s\n' "$BACKUP"
fi
mv -- "$STAGE" "$PLUGIN_DIR"
trap - EXIT
mkdir -p "$(dirname "$NAUTILUS_EXTENSION")"
if [[ ! -e "$NAUTILUS_EXTENSION" && ! -L "$NAUTILUS_EXTENSION" ]]; then
  ln -s -- "$NAUTILUS_TARGET" "$NAUTILUS_EXTENSION"
elif [[ ! -L "$NAUTILUS_EXTENSION" || $(readlink -- "$NAUTILUS_EXTENSION") != "$NAUTILUS_TARGET" ]]; then
  printf 'El registro Nautilus cambió durante la instalación: %s\n' "$NAUTILUS_EXTENSION" >&2
  exit 1
fi
install -Dm644 /usr/lib/systemd/user/omarchy-quickshare.service.d/binary.conf "$CONFIG_HOME/systemd/user/omarchy-quickshare.service.d/binary.conf"
# A marked source installation belongs to this project; remove its obsolete executable.
if $UPDATE && [[ -f "$HOME/.local/bin/omarchy-quickshare" ]]; then
  rm -f -- "$HOME/.local/bin/omarchy-quickshare"
fi
systemctl --user daemon-reload
systemctl --user enable omarchy-quickshare.service
systemctl --user restart omarchy-quickshare.service
if $UPDATE; then
  omarchy restart shell
else
  omarchy-shell shell rescanPlugins
fi
deadline=$((SECONDS + 15))
until omarchy plugin list --json | jq -e 'any(.[]; .id == "macondo.quickshare")' >/dev/null; do
  if (( SECONDS >= deadline )); then
    printf 'El shell no registró el plugin a tiempo; el servicio permanece instalado.\n' >&2
    exit 1
  fi
  sleep 0.2
done
omarchy plugin enable macondo.quickshare
/usr/bin/omarchy-quickshare status
