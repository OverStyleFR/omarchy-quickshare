#!/usr/bin/env bash
set -euo pipefail
ROOT=$(realpath "$(dirname "${BASH_SOURCE[0]}")/../..")
WORK=$(mktemp -d)
trap 'rm -rf -- "$WORK"' EXIT

check_transition() {
  local name=$1 load_state=$2 query_status=$3 stop_status=$4 expected=$5 result
  mkdir "$WORK/$name"
  # Model only systemd's load/stop boundary; run the published pause function
  # under errexit, as the installer does, and observe whether replacement proceeds.
  if CASE_DIR="$WORK/$name" COMMON="$ROOT/scripts/service-common.sh" \
    LOAD_STATE="$load_state" QUERY_STATUS="$query_status" STOP_STATUS="$stop_status" \
    bash <<'CASE'
set -euo pipefail
source "$COMMON"
STATE_DIR=$CASE_DIR
systemctl() {
  case "$*" in
    *--property=ActiveState*) printf 'inactive\n' ;;
    *--property=LoadState*) printf '%s\n' "$LOAD_STATE"; return "$QUERY_STATUS" ;;
    '--user stop omarchy-quickshare.service')
      [[ $LOAD_STATE != not-found ]] || return 5
      return "$STOP_STATUS"
      ;;
    *) printf 'Unexpected manager operation\n' >&2; return 64 ;;
  esac
}
pause_service
printf 'replacement may proceed\n' > "$CASE_DIR/replacement"
CASE
  then result=0; else result=$?; fi
  if (( result != expected )); then
    printf '%s: expected exit %s, got %s\n' "$name" "$expected" "$result" >&2
    exit 1
  fi
  if (( expected == 0 )); then
    [[ -f "$WORK/$name/replacement" ]] || { printf '%s: installation was blocked\n' "$name" >&2; exit 1; }
  else
    [[ ! -e "$WORK/$name/replacement" ]] || { printf '%s: replacement continued after a stop failure\n' "$name" >&2; exit 1; }
  fi
  printf '%s: passed\n' "$name"
}

check_transition first-install not-found 0 0 0
check_transition absent-unit-query-status not-found 4 0 0
check_transition existing-unit loaded 0 0 0
check_transition existing-unit-stop-failure loaded 0 29 29
check_transition masked-unit-stop-failure masked 0 31 31
