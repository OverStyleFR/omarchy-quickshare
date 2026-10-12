#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/service-common.sh"
initialize
check_checkout
check_resources
check_idle
PACKAGE=false
package_installed && PACKAGE=true
confirm \
  'Remove the Quick Share service and Nautilus action? The native Git plugin, preferences and received files will stay. [y/N]' \
  '¿Retirar el servicio Quick Share y la acción Nautilus? Se conservarán el plugin Git nativo, las preferencias y los archivos recibidos. [s/N]' || exit 2
REMOVE_PACKAGE=false
if $PACKAGE; then
  require_commands sudo
  if confirm \
    'Also authorize sudo pacman -R omarchy-quickshare to remove the installed package (not shared dependencies)? Choose No to keep the package disabled. [y/N]' \
    '¿Autorizar también sudo pacman -R omarchy-quickshare para retirar el paquete instalado (sin dependencias compartidas)? Elige No para conservar el paquete desactivado. [s/N]'; then
    REMOVE_PACKAGE=true
  fi
fi
check_resources
preserve_language
pause_service
systemctl --user disable omarchy-quickshare.service
# Keep an owned mask while authorizing package removal, and permanently only
# if the user chooses to retain the package. Install service clears that mask.
if [[ -e $UNIT && ! -L $UNIT ]]; then rm -- "$UNIT"; fi
if [[ ! -L $UNIT ]]; then ln -s /dev/null "$UNIT"; fi
systemctl --user daemon-reload
if $REMOVE_PACKAGE; then
  sudo pacman -R -- omarchy-quickshare
  if package_installed; then fail 'Package removal did not complete; the service remains disabled.' 'La retirada del paquete no terminó; el servicio sigue desactivado.'; fi
fi
if [[ -L $NAUTILUS_EXTENSION && $(readlink "$NAUTILUS_EXTENSION") == "$NAUTILUS_TARGET" ]]; then rm -- "$NAUTILUS_EXTENSION"; fi
if [[ -f $DROPIN ]]; then rm -- "$DROPIN"; fi
if owned_file "$BINARY" "$STATE_DIR/binary.sha256"; then rm -- "$BINARY"; fi
if [[ -f $BACKEND_DATA/.service-owned ]] && cmp -s "$BACKEND_DATA/.service-owned" <(printf 'omarchy-quickshare-service\n'); then rm -rf -- "$BACKEND_DATA"; fi
if [[ -d "$STATE_DIR/build-cache" && ! -L "$STATE_DIR/build-cache" ]] &&
  cmp -s "$STATE_DIR/build-cache/.compiler-owned" <(printf 'omarchy-quickshare-build\n'); then
  rm -rf -- "$STATE_DIR/build-cache"
fi
rm -f -- "$STATE_DIR/unit.sha256" "$STATE_DIR/dropin.sha256" "$STATE_DIR/binary.sha256"
printf 'disabled\n' > "$STATE_DIR/mode"
if ! $PACKAGE || $REMOVE_PACKAGE; then
  if [[ -L $UNIT && $(readlink "$UNIT") == /dev/null ]]; then rm -- "$UNIT"; fi
fi
systemctl --user daemon-reload
if $PACKAGE && ! $REMOVE_PACKAGE; then
  say 'Service disabled and Nautilus action removed. The system package is retained, NOT uninstalled. Install service can enable it again.' \
    'Servicio desactivado y acción Nautilus retirada. El paquete del sistema se conserva, NO está desinstalado. Instalar servicio puede habilitarlo de nuevo.'
  printf 'retained\n' > "$STATE_DIR/removal-result"
else
  say 'Backend removed and Nautilus action unregistered. Preferences, downloads and the native Git plugin are preserved.' \
    'Servicio retirado y acción Nautilus eliminada. Se conservan preferencias, descargas y el plugin Git nativo.'
  printf 'removed\n' > "$STATE_DIR/removal-result"
fi
say 'To remove the widget/checkout, now run: omarchy plugin remove macondo.quickshare' \
  'Para retirar el widget/checkout, ejecuta ahora: omarchy plugin remove macondo.quickshare'
