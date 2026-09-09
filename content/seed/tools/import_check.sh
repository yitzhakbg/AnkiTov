#!/usr/bin/env bash
# scripts/seed/import_check.sh - AnkiTov Phase 0 headless-Anki gate (plan section 6).
#
# STATUS (2026-09-03): SKELETON authored by the en/math-definitions slice.
# NOT executed by authoring; the headless-Anki run is a later Phase 0 gate.
# Steps follow plan section 6.1; constraints follow section 6.2 (self-contained:
# never touches the developer's real Anki profile - only a temp base dir/profile).
#
# Usage:    scripts/seed/import_check.sh [--locale en|he|fr] [--deck <slug>] [--keep]
# Env:      ANKICONNECT_PORT (default 8765), ANKI_BIN (default: auto-detect), WAIT_SECS (default 60)
# Exit codes (plan section 6.2):
#   0  all green            1  preflight failure        2  import failure
#   3  count mismatch       4  integrity/encoding failure

set -u

EX_OK=0; EX_PREFLIGHT=1; EX_IMPORT=2; EX_COUNT=3; EX_INTEGRITY=4

PORT="${ANKICONNECT_PORT:-8765}"
WAIT_SECS="${WAIT_SECS:-60}"
KEEP=0; WANT_LOCALE=""; WANT_DECK=""
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SEED_DIR="$(cd "$SCRIPT_DIR/../../content/seed" && pwd)"
MANIFEST="$SEED_DIR/manifest.json"
ANKI_BIN="${ANKI_BIN:-}"
TMP=""; ANKI_PID=""; FAILURES=0

usage() {
  cat <<'USAGE'
Usage: scripts/seed/import_check.sh [--locale en|he|fr] [--deck <slug>] [--keep]

Imports every manifest package into a scratch headless Anki profile via
AnkiConnect, asserts per-note-type card counts against manifest.json, and
checks package integrity. Never touches a real Anki profile.
Exit codes: 0 all green | 1 preflight | 2 import | 3 counts | 4 integrity.
USAGE
}

log() { printf '[import_check] %s\n' "$*"; }

fail_pkg() { printf '[import_check] FAIL  %s: %s\n' "$1" "$2" >&2; FAILURES=$((FAILURES + 1)); }

cleanup() {
  if [ "$KEEP" -eq 1 ]; then
    log "--keep: leaving temp dir '$TMP' and Anki pid '${ANKI_PID:-none}' for debugging"
  else
    [ -n "$ANKI_PID" ] && kill "$ANKI_PID" 2>/dev/null
    [ -n "$TMP" ] && rm -rf "$TMP"
  fi
}
trap cleanup EXIT

anki_connect() { # $1=action  $2=params-json
  curl -s "http://127.0.0.1:${PORT}" -X POST \
    -d "{\"action\": \"$1\", \"version\": 6, \"params\": $2}"
}

usage_help() {
  while [ $# -gt 0 ]; do
    case "$1" in
      --locale) WANT_LOCALE="${2:?--locale needs a value}"; shift 2 ;;
      --deck)   WANT_DECK="${2:?--deck needs a value}"; shift 2 ;;
      --keep)   KEEP=1; shift ;;
      -h|--help) sed -n '2,14p' "$0"; exit "$EX_OK" ;;
      *) echo "unknown argument: $1" >&2; exit "$EX_PREFLIGHT" ;;
    esac
  done
}

# --- Step 1: preflight (plan 6.1 step 1) -------------------------------------
preflight() {
  local rc=0 tool
  for tool in python3 jq curl; do
    command -v "$tool" >/dev/null 2>&1 || { log "preflight: missing tool: $tool"; rc=1; }
  done
  if [ -z "$ANKI_BIN" ]; then
    local cand
    for cand in anki Anki; do
      command -v "$cand" >/dev/null 2>&1 && { ANKI_BIN="$cand"; break; }
    done
  fi
  [ -n "$ANKI_BIN" ] || { log "preflight: Anki binary not found (set ANKI_BIN)"; rc=1; }
  [ -f "$MANIFEST" ] || { log "preflight: missing manifest $MANIFEST"; rc=1; }

  if [ "$rc" -eq 0 ]; then
    # Package presence + sha256 drift detection BEFORE any Anki work.
    python3 - "$MANIFEST" "$SEED_DIR" "$WANT_LOCALE" "$WANT_DECK" <<'PY' || rc=1
import hashlib, json, pathlib, sys
manifest = json.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))
seed = pathlib.Path(sys.argv[2]); loc = sys.argv[3]; deck = sys.argv[4]
bad = 0
for d in manifest["decks"]:
    if loc and d["locale"] != loc: continue
    if deck and d["slug"] != deck: continue
    f = seed / d["file"]
    if not f.is_file():
        print(f"preflight: missing package {d['file']}"); bad = 1; continue
    h = hashlib.sha256(f.read_bytes()).hexdigest()
    if h != d["sha256"]:
        print(f"preflight: sha256 drift for {d['file']}"); bad = 1
sys.exit(1 if bad else 0)
PY
  fi
  return $rc
}

# --- Step 2: scratch environment (plan 6.1 step 2) ----------------------------
start_anki() {
  local stale waited=0 resp
  # Kill any stale listener on the port first so re-runs are safe.
  stale=$(lsof -ti tcp:"$PORT" 2>/dev/null || true)
  if [ -n "$stale" ]; then
    log "killing stale listener(s) on port $PORT: $stale"
    kill $stale 2>/dev/null || true
    sleep 1
  fi
  TMP="$(mktemp -d)"
  mkdir -p "$TMP/base"
  # TODO(plan 6.1 step 2): ensure the AnkiConnect addon is active for this
  # scratch profile (bundle/sync the addon into "$TMP/base" before launch if
  # the bundled install does not apply to a fresh base dir).
  QT_QPA_PLATFORM=offscreen "$ANKI_BIN" -b "$TMP/base" >/dev/null 2>&1 &
  ANKI_PID=$!
  log "waiting up to ${WAIT_SECS}s for AnkiConnect on 127.0.0.1:${PORT}"
  while [ "$waited" -lt "$WAIT_SECS" ]; do
    resp=$(curl -s "http://127.0.0.1:${PORT}" -X POST -d '{"action":"version","version":6}')
    if printf '%s' "$resp" | jq -e '(.result | type) == "number"' >/dev/null 2>&1; then
      log "AnkiConnect is up"
      return 0
    fi
    sleep 1; waited=$((waited + 1))
  done
  log "preflight: AnkiConnect did not come up on port $PORT"
  return 1
}

# --- Steps 3-5: import, counts, integrity for one package ---------------------
import_deck() { # $1=title $2=apkg path
  local title="$1" file="$2" resp
  resp=$(anki_connect importPackage "{\"path\": \"$file\"}")
  # TODO(plan 6.1 step 3): confirm importPackage result semantics on the
  # bundled Anki version (null result + null error = success) and tighten here.
  if ! printf '%s' "$resp" | jq -e '.error == null' >/dev/null 2>&1; then
    fail_pkg "$file" "import failed: $resp"
    return 1
  fi
  return 0
}

check_counts() { # $1=title $2=file $3=basic $4=cloze $5=type_in
  local title="$1" file="$2" model nt q resp n
  local -a wants=( "$3" "$4" "$5" )
  local -a models=( AnkiTovBasic AnkiTovCloze AnkiTovTypeIn )
  local -a keys=( basic cloze type_in )
  local i
  for i in 0 1 2; do
    nt="${models[$i]}"; q="deck:\"${title}\" note:\"${nt}\""
    resp=$(anki_connect findCards "{\"query\": \"$q\"}")
    n=$(printf '%s' "$resp" | jq '.result | length')
    if [ "$n" != "${wants[$i]}" ]; then
      fail_pkg "$file" "count mismatch ${keys[$i]}: got $n want ${wants[$i]}"
      return 1
    fi
  done
  # Per plan 6.1 step 4: the census values must sum to card_count (checked
  # here via $sum) and the total must match findCards over the whole deck.
  local sum=$(( $3 + $4 + $5 ))
  resp=$(anki_connect findCards "{\"query\": \"deck:\\\"${title}\\\"\"}")
  n=$(printf '%s' "$resp" | jq '.result | length')
  if [ "$n" != "$sum" ]; then
    fail_pkg "$file" "total count: got $n want $sum"
    return 1
  fi
  return 0
}

check_integrity() { # $1=title $2=file $3=rtl(true/false)
  # Plan 6.1 step 5 (skeleton):
  #  - sample cardsInfo: required fields non-empty
  #  - v1 seed is text-only: zero media references expected
  #  - he packages: UTF-8 round-trip on every field incl. typed-answer Backs
  #  - type_in > 0 packages: modelFieldNames/modelTemplates assert AnkiTovTypeIn
  #    has fields Front/Back and {{type:Back}} in the question template;
  #    he additionally asserts .ankitov-he CSS rules (plan 3.6).
  # TODO: implement via cardsInfo / modelFieldNames / modelTemplates;
  # failures here are exit-code 4.
  fail_pkg "$1" "integrity checks not yet implemented (skeleton)"
  return 1
}

main() {
  usage_help "$@"

  preflight || exit "$EX_PREFLIGHT"
  start_anki  || exit "$EX_PREFLIGHT"

  local idx=0 total
  total=$(jq '[.decks[]
    | select((env.WANT_LOCALE == "" or .locale == env.WANT_LOCALE)
      and (env.WANT_DECK == "" or .slug == env.WANT_DECK))] | length' \
    WANT_LOCALE="$WANT_LOCALE" WANT_DECK="$WANT_DECK" "$MANIFEST")

  # Import loop + count asserts in manifest order (deterministic, plan 6.2).
  while [ "$idx" -lt "$total" ]; do
    local row title file basic cloze type_in card_count
    row=$(jq -c "[.decks[]
      | select((env.WANT_LOCALE == \"\" or .locale == env.WANT_LOCALE)
        and (env.WANT_DECK == \"\" or .slug == env.WANT_DECK))][$idx]" \
      WANT_LOCALE="$WANT_LOCALE" WANT_DECK="$WANT_DECK" "$MANIFEST")
    title=$(printf '%s' "$row" | jq -r .title)
    file=$(printf '%s' "$row" | jq -r .file)
    card_count=$(printf '%s' "$row" | jq -r .card_count)
    basic=$(printf '%s' "$row" | jq -r '.note_type_census.basic')
    cloze=$(printf '%s' "$row" | jq -r '.note_type_census.cloze')
    type_in=$(printf '%s' "$row" | jq -r '.note_type_census.type_in')

    if import_deck "$title" "$SEED_DIR/$file"; then
      check_counts "$title" "$file" "$basic" "$cloze" "$type_in" || true
    else
      fail_pkg "$file" "AnkiConnect importPackage failed"
    fi
    idx=$((idx + 1))
  done

  log "12-row report table (package -> imported -> count -> count-match -> media-ok): TODO in skeleton"
  log "failures: $FAILURES"
  if [ "$FAILURES" -gt 0 ]; then
    exit "$EX_COUNT"
  fi
  exit "$EX_OK"
}

main "$@"
