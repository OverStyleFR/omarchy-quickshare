#!/usr/bin/env bash
set -euo pipefail
ROOT=$(realpath "$(dirname "${BASH_SOURCE[0]}")/..")
WORK=$(mktemp -d)
trap 'rm -rf -- "$WORK"' EXIT
VERSION=0.1.0
command -v makepkg >/dev/null || { printf 'Falta makepkg.\n' >&2; exit 1; }
[[ -f "$ROOT/Cargo.lock" ]] || { printf 'Falta Cargo.lock; genera y verifica primero la compilación.\n' >&2; exit 1; }

# Stable order, normalized timestamps/owners and timestamp-free gzip make this
# source snapshot reproducible. No build artifacts enter the archive.
LC_ALL=C tar --sort=name --mtime="@${SOURCE_DATE_EPOCH:-0}" --owner=0 --group=0 --numeric-owner \
  --format=gnu -cf - \
  --exclude='target' --exclude='bindings' --exclude='.git' \
  --transform="s,^,omarchy-quickshare-$VERSION/," \
  -C "$ROOT" Cargo.toml Cargo.lock src vendor manifest.json Service.qml BarWidget.qml Panel.qml nautilus packaging scripts LICENSE README.md IMPLEMENTATION_PLAN.md CONTRIBUTING.md CODE_OF_CONDUCT.md SECURITY.md docs |
  gzip -n > "$WORK/omarchy-quickshare-$VERSION.tar.gz"
SOURCE_CHECKSUM=$(sha256sum "$WORK/omarchy-quickshare-$VERSION.tar.gz")
SOURCE_CHECKSUM=${SOURCE_CHECKSUM%% *}
# Ship a standalone PKGBUILD with the digest of this exact source snapshot,
# rather than an environment-dependent checksum or a verification bypass.
while IFS= read -r line || [[ -n "$line" ]]; do
  if [[ "$line" == 'sha256sums=('* ]]; then
    printf 'sha256sums=(%q)\n' "$SOURCE_CHECKSUM"
  else
    printf '%s\n' "$line"
  fi
done < "$ROOT/packaging/PKGBUILD" > "$WORK/PKGBUILD"
(
  cd "$WORK"
  makepkg --cleanbuild "$@"
)
SOURCE_DIR="$ROOT/dist/source/omarchy-quickshare-$VERSION"
mkdir -p "$SOURCE_DIR"
install -m644 "$WORK/PKGBUILD" "$WORK/omarchy-quickshare-$VERSION.tar.gz" "$SOURCE_DIR/"
for package in "$WORK"/*.pkg.tar.*; do
  [[ -f "$package" ]] || continue
  install -m644 "$package" "$ROOT/dist/"
done
printf 'Paquetes construidos en %s/dist; fuente y PKGBUILD con SHA-256 en %s\n' "$ROOT" "$SOURCE_DIR"
