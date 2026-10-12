#!/usr/bin/env bash
set -euo pipefail
ROOT=$(realpath "$(dirname "${BASH_SOURCE[0]}")/..")
ACTION=${1:-}
LANGUAGE=${2:-en}
[[ $ACTION == install || $ACTION == update || $ACTION == remove ]] || { printf 'Usage: %s install|update|remove [en|es]\n' "$0" >&2; exit 2; }
[[ $LANGUAGE == en || $LANGUAGE == es ]] || exit 2
(( EUID != 0 )) || { printf 'Run as the desktop user, not root.\n' >&2; exit 1; }
export QUICKSHARE_LANGUAGE=$LANGUAGE
STATE_DIR=${XDG_STATE_HOME:-$HOME/.local/state}/omarchy-quickshare/service
[[ $STATE_DIR == /* && ! -L $STATE_DIR ]] || exit 1
install -d -m700 "$STATE_DIR"
say() { if [[ $LANGUAGE == es ]]; then printf '%s\n' "$2"; else printf '%s\n' "$1"; fi; }
RESULT=1
OUTCOME=failed
write_receipt() {
  local receipt
  receipt=$(mktemp "$STATE_DIR/.setup-result.XXXXXXXX")
  printf '{"action":"%s","outcome":"%s","time":%s}\n' "$ACTION" "$OUTCOME" "$(date +%s%3N)" > "$receipt"
  chmod 600 "$receipt"
  mv -f -- "$receipt" "$STATE_DIR/setup-result.json"
}
trap write_receipt EXIT
say 'Quick Share · Service maintenance' 'Quick Share · Mantenimiento del servicio'
if [[ $ACTION == remove ]]; then
  if bash "$ROOT/scripts/uninstall.sh"; then
    RESULT=0
    OUTCOME=$(<"$STATE_DIR/removal-result")
  fi
else
  if bash "$ROOT/scripts/install.sh" "$ACTION"; then
    RESULT=0
    OUTCOME=installed
  fi
fi
if (( RESULT != 0 )); then
  say 'Maintenance did not complete. Review the actual error above; no success is claimed. Return to the panel and retry after resolving it.' \
    'El mantenimiento no terminó. Revisa el error real arriba; no se declara éxito. Vuelve al panel y reintenta después de resolverlo.' >&2
fi
write_receipt
trap - EXIT
say 'Press Enter to close this terminal.' 'Pulsa Intro para cerrar esta terminal.'
read -r _ || true
exit "$RESULT"
