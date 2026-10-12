#!/usr/bin/env bash
set -euo pipefail

ROOT=$(realpath "$(dirname "${BASH_SOURCE[0]}")/..")
CONFIG_HOME=${XDG_CONFIG_HOME:-$HOME/.config}
DATA_HOME=${XDG_DATA_HOME:-$HOME/.local/share}
PLUGIN_ID=macondo.quickshare
PLUGIN_DIR="$CONFIG_HOME/omarchy/plugins/$PLUGIN_ID"
NAUTILUS_TARGET="$PLUGIN_DIR/nautilus/omarchy_quickshare.py"
NAUTILUS_EXTENSION="$DATA_HOME/nautilus-python/extensions/omarchy_quickshare.py"
UNIT_DIR="$CONFIG_HOME/systemd/user"
BINARY="$HOME/.local/bin/omarchy-quickshare"

plugin_is_owned() {
  [[ -f "$PLUGIN_DIR/.omarchy-quickshare-owned" ]] &&
    cmp -s -- "$PLUGIN_DIR/.omarchy-quickshare-owned" <(printf 'omarchy-quickshare\n')
}

if (( EUID == 0 )); then
  printf 'Instala como el usuario del escritorio, no como root.\n' >&2
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
for command in cargo omarchy omarchy-shell systemctl install jq python nautilus cmp; do
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

omarchy plugin validate "$ROOT"
# Cargo reports the actual artifact path, including custom target directories/targets.
BUILT_BINARY=$(cargo build --manifest-path "$ROOT/Cargo.toml" --release --locked \
  --package omarchy-quickshare --message-format=json-render-diagnostics |
  jq -er 'select(.reason == "compiler-artifact" and .target.name == "omarchy-quickshare" and (.target.kind | index("bin")) != null) | .executable | select(. != null)')
if systemctl --user is-active --quiet omarchy-quickshare.service; then
  if ! "$BUILT_BINARY" status | jq -e 'all(.transfers[]; .state == "completed" or .state == "rejected" or .state == "cancelled" or .state == "failed")' >/dev/null; then
    printf 'Termina o cancela las transferencias antes de actualizar.\n' >&2
    exit 1
  fi
  systemctl --user stop omarchy-quickshare.service
fi
UPDATE=false
[[ ! -d "$PLUGIN_DIR" ]] || UPDATE=true

mkdir -p "$CONFIG_HOME/omarchy/plugins" "$UNIT_DIR" "$HOME/.local/bin"
# Staging and backups must remain outside the shell's plugin registry.
STAGE=$(mktemp -d "$CONFIG_HOME/omarchy/.quickshare-stage.XXXXXXXX")
trap 'rm -rf -- "$STAGE"' EXIT
cp -a -- "$ROOT/manifest.json" "$ROOT/Service.qml" "$ROOT/BarWidget.qml" "$ROOT/Panel.qml" "$ROOT/nautilus" "$STAGE/"
printf 'omarchy-quickshare\n' > "$STAGE/.omarchy-quickshare-owned"
# The source is validated before replacing the plugin. Preserve the previous copy.
if [[ -d "$PLUGIN_DIR" ]]; then
  BACKUP="$CONFIG_HOME/omarchy/plugin-backups/$PLUGIN_ID.$(date +%s).$$"
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
install -Dm755 "$BUILT_BINARY" "$BINARY"
install -Dm644 "$ROOT/packaging/omarchy-quickshare.service" "$UNIT_DIR/omarchy-quickshare.service"
install -Dm644 "$ROOT/packaging/local-binary.conf" "$UNIT_DIR/omarchy-quickshare.service.d/binary.conf"
install -Dm644 "$ROOT/LICENSE" "$DATA_HOME/licenses/omarchy-quickshare/LICENSE"
install -Dm644 "$ROOT/vendor/rquickshare/LICENSE" "$DATA_HOME/licenses/omarchy-quickshare/LICENSE-GPL"
install -Dm644 "$ROOT/vendor/rquickshare/LICENSE-APACHE" "$DATA_HOME/licenses/omarchy-quickshare/LICENSE-APACHE"
install -Dm644 "$ROOT/vendor/rquickshare/LICENSE-CHROMIUM" "$DATA_HOME/licenses/omarchy-quickshare/LICENSE-CHROMIUM"
install -Dm644 "$ROOT/vendor/rquickshare/NOTICE" "$DATA_HOME/licenses/omarchy-quickshare/NOTICE"
install -Dm644 "$ROOT/vendor/THIRD_PARTY_NOTICES.txt" "$DATA_HOME/licenses/omarchy-quickshare/THIRD_PARTY_NOTICES.txt"

systemctl --user daemon-reload
systemctl --user enable omarchy-quickshare.service
systemctl --user restart omarchy-quickshare.service
if $UPDATE; then
  omarchy restart shell
else
  omarchy-shell shell rescanPlugins
fi
deadline=$((SECONDS + 15))
until omarchy plugin list --json | jq -e --arg id "$PLUGIN_ID" 'any(.[]; .id == $id)' >/dev/null; do
  if (( SECONDS >= deadline )); then
    printf 'El shell no registró el plugin a tiempo; el servicio permanece instalado.\n' >&2
    exit 1
  fi
  sleep 0.2
done
omarchy plugin enable "$PLUGIN_ID"
"$BINARY" status
printf '\nInstalado. La visibilidad empieza desactivada; actívala temporalmente desde el panel.\n'
