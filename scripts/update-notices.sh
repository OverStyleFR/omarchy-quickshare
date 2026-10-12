#!/usr/bin/env bash
set -euo pipefail
ROOT=$(realpath "$(dirname "${BASH_SOURCE[0]}")/..")
TARGET=${1:-${CARGO_BUILD_TARGET:-}}
if [[ -z "$TARGET" ]]; then
  RUST_INFO=$(rustc -vV)
  while IFS= read -r line; do
    [[ "$line" != 'host: '* ]] || TARGET=${line#host: }
  done <<< "$RUST_INFO"
fi
[[ -n "$TARGET" ]] || { printf 'No se pudo determinar el target Rust.\n' >&2; exit 1; }
WORK=$(mktemp -d)
OUTPUT=$(mktemp "$ROOT/vendor/.third-party-notices.XXXXXXXX")
trap 'rm -rf -- "$WORK"; rm -f -- "$OUTPUT"' EXIT
mkdir "$WORK/texts"
shopt -s nullglob
cargo metadata --manifest-path "$ROOT/Cargo.toml" --locked --offline --format-version 1 \
  --filter-platform "$TARGET" > "$WORK/metadata.json"
# Materialize each producer before consuming it: failures in process
# substitutions do not propagate through set -e/pipefail to the parent shell.
jq -er '.workspace_members as $members | [.packages | sort_by(.name,.version)[] | select(.id as $id | $members | index($id) | not)] | if length == 0 then error("empty dependency inventory") else .[] end | if ((.license // "") | test("AGPL"; "i")) then error("AGPL dependency: " + .name) else [.name,.version,.license // "UNSPECIFIED",.source // "local path",(if (.authors|length)>0 then (.authors|join(", ")) else "No authors declared" end),.manifest_path,.license_file // "-"] | @tsv end' "$WORK/metadata.json" > "$WORK/packages.tsv"
printf 'THIRD-PARTY NOTICES — Omarchy Quick Share\n\nTarget: %s\nVersions and git revisions are fixed in Cargo.lock. This inventory includes build dependencies. Original notices are retained, not relicensed. Identical texts are reproduced once and referenced by SHA-256. Unmodified MPL-covered source is available using the exact crate download URLs below.\n' "$TARGET" > "$OUTPUT"
count=0
while IFS=$'\t' read -r name version license source authors manifest declared_file; do
  [[ "$license" != UNSPECIFIED || "$declared_file" != '-' ]] || { printf 'Falta licencia declarada: %s %s\n' "$name" "$version" >&2; exit 1; }
  directory=${manifest%/*}
  printf '\n========================================================================\n%s %s\nDeclared license: %s\nAuthors declared in Cargo.toml: %s\nSource: %s\n' \
    "$name" "$version" "$license" "$authors" "$source" >> "$OUTPUT"
  if [[ "$source" == registry+* ]]; then
    printf 'Published source: https://crates.io/api/v1/crates/%s/%s/download\n' "$name" "$version" >> "$OUTPUT"
  fi
  files=("$directory"/LICENSE* "$directory"/LICENCE* "$directory"/COPYING* "$directory"/COPYRIGHT* \
    "$directory"/NOTICE* "$directory"/UNLICENSE* "$directory"/license* "$directory"/licence* \
    "$directory"/AUTHORS* "$directory"/CONTRIBUTORS*)
  if [[ "$declared_file" != '-' ]]; then
    # Cargo permits license-file paths relative to the package manifest.
    [[ "$declared_file" == /* ]] || declared_file="$directory/$declared_file"
    files+=("$declared_file")
  fi
  if (( ${#files[@]} == 0 )); then
    case "$name:$version:$license" in
      'custom_debug:0.6.2:Apache-2.0 OR MIT'|'custom_debug_derive:0.6.2:Apache-2.0 OR MIT')
        printf 'The published crate and upstream revision 5e3d98809be2b621160122720828b69ac9d335c2 contain no separate license-text files. Cargo.toml declares Apache-2.0 OR MIT. This distribution selects Apache-2.0; its complete standard text is referenced below. Attribution above is reproduced from package metadata.\n' >> "$OUTPUT"
        files=("$ROOT/vendor/rquickshare/LICENSE-APACHE")
        ;;
      *) printf 'Falta texto de licencia: %s %s; auditar upstream antes de distribuir.\n' "$name" "$version" >&2; exit 1 ;;
    esac
  fi
  printf '%s\0' "${files[@]}" | sort -zu > "$WORK/notice-files"
  while IFS= read -r -d '' file; do
    [[ -f "$file" ]] || { printf 'El aviso requiere auditoría de directorio: %s\n' "$file" >&2; exit 1; }
    checksum=$(sha256sum "$file")
    checksum=${checksum%% *}
    [[ -f "$WORK/texts/$checksum" ]] || cp -- "$file" "$WORK/texts/$checksum"
    printf 'Notice file: %s — text SHA-256 %s\n' "${file##*/}" "$checksum" >> "$OUTPUT"
  done < "$WORK/notice-files"
  ((count+=1))
done < "$WORK/packages.tsv"
for file in "$WORK/texts/"*; do
  printf '\n========================================================================\nLICENSE TEXT SHA-256 %s\n\n' "${file##*/}" >> "$OUTPUT"
  cat -- "$file" >> "$OUTPUT"
  printf '\n' >> "$OUTPUT"
done
chmod 644 "$OUTPUT"
mv -- "$OUTPUT" "$ROOT/vendor/THIRD_PARTY_NOTICES.txt"
printf 'Avisos actualizados: %s dependencias, target %s.\n' "$count" "$TARGET"
