#!/usr/bin/env bash
# Synthetic seat load benchmark — ub3090 (or any Linux seat host).
#
# WHY (Gemini review, 2026-11-20): our $5–8/mo pricing and the "~99% margin"
# claim rested on theoretical page-sharing math. This measures the real
# ceilings: spawn N seats that ACTIVELY flip cards (WebRTC keyframe bursts +
# JS heap churn), and sample per-seat private/shared RAM, CPU, and GPU state.
#
# Usage (on the seat host, e.g. ub3090):
#   ./seat_loadtest.sh --seats 10            # ramp: 10 seats, 3 min
#   ./seat_loadtest.sh --seats 25 --minutes 5
#   ./seat_loadtest.sh --seats 35 --ramp 30
#
# Prereqs:
#   * rootfs exported: stream/seat-rootfs/build-rootfs.sh --out $ROOTFS
#   * bwrap installed; EITHER/OR policy respected (no model server running —
#     the fleet and Strata/ninfer must never coexist on ub3090).
#
# Output: CSV per sample + a summary line per seat count. Feed the summary
# into specs/strategy/2026-11-20-selkies-packaging-strategic-review.md §bench.
set -euo pipefail

SEATS=10
MINUTES=3
RAMP=5
ROOTFS="${ANKITOV_SEAT_ROOTFS:-/srv/ankitov-seat/rootfs}"
BASE_PORT="${ANKITOV_SEAT_BASE_PORT:-8100}"
COLL_ROOT="${ANKITOV_COLLECTION_ROOT:-/tmp/seat-bench-collections}"
OUT_CSV="${1:-/tmp/seat-loadtest.csv}"
[ $# -gt 0 ] && shift   # positional csv consumed; options follow

while [ $# -gt 0 ]; do
  case "$1" in
    --seats) SEATS="$2"; shift 2 ;;
    --minutes) MINUTES="$2"; shift 2 ;;
    --ramp) RAMP="$2"; shift 2 ;;
    *) echo "unknown arg $1" >&2; exit 1 ;;
  esac
done

command -v bwrap >/dev/null || { echo "bwrap not installed" >&2; exit 1; }
[ -d "$ROOTFS/stream" ] || { echo "rootfs missing at $ROOTFS (build it first)" >&2; exit 1; }

# EITHER/OR guard: refuse to start if a model server is up (ub3090 policy).
if pgrep -f "strata|ninfer" >/dev/null 2>&1; then
  echo "REFUSING: a model server (strata/ninfer) is running — either/or policy." >&2
  exit 1
fi

PIDS=()
cleanup() {
  echo "[bench] tearing down seats ..."
  for p in "${PIDS[@]:-}"; do kill -TERM "$p" 2>/dev/null || true; done
  sleep 2
  for p in "${PIDS[@]:-}"; do kill -KILL "$p" 2>/dev/null || true; done
}
trap 'rc=$?; echo "[bench] exit rc=$rc (137=SIGKILL/OOM 143=SIGTERM 141=SIGPIPE)"; cleanup' EXIT

echo "sample_ts,seats_up,rss_shared_kb,rss_private_kb,cpu_percent,gpu_used" > "$OUT_CSV"

ANKICONNECT_SRC="${ANKITOV_ANKICONNECT_SRC:-/home/ybg/ankitov-army/students/student-1/Anki2/addons21/2055491513}"

spawn_seat() {
  local n="$1" port="$2"
  local coll="$COLL_ROOT/bench-$n"
  mkdir -p "$coll"
  # Export the per-seat Selkies port — without it every seat fights over the
  # default :8080 and only the first one streams.
  export ANKITOV_SEAT_PORT="$port"
  # SeatManager always provides these; start-seat.sh runs set -u and dies
  # without them (ANKITOV_SEAT_STUDENT unbound -> instant seat death).
  export ANKITOV_SEAT_STUDENT="bench-$n"
  export ANKITOV_BACKEND_URL="${ANKITOV_BACKEND_URL:-http://127.0.0.1:1}"
  # Same sandbox shape as SeatManager (backend/src/services/seat_manager.rs):
  # writable per-seat s6 service copy (s6 needs write access), tmpfs
  # /home/ubuntu + /dev/shm (QtWebEngine), /sys ro-bind.
  mkdir -p "$coll/etc-service"
  cp -a "$ROOTFS/etc/service/." "$coll/etc-service/" 2>/dev/null || true
  # Stage AnkiConnect into the collection so the flip driver can reach it
  # (fresh collections ship no addons; per-seat port = 18700+n).
  if [ -d "$ANKICONNECT_SRC" ]; then
    mkdir -p "$coll/addons21/2055491513"
    cp -a "$ANKICONNECT_SRC/." "$coll/addons21/2055491513/" 2>/dev/null || true
    printf '{"webPort": %d}\n' "$((18700 + n))" > "$coll/addons21/2055491513/config.json"
    # AnkiConnect reads config via Anki's addon-config store, which is EMPTY
    # in fresh collections -> hardcoded 8765 default wins and all seats
    # collide on one port. Sed the per-seat port straight into the source.
    sed -i "s/8765/$((18700 + n))/g" "$coll/addons21/2055491513/__init__.py" 2>/dev/null || true
  fi
  bwrap \
    --ro-bind "$ROOTFS" / \
    --tmpfs /data --tmpfs /tmp --tmpfs /dev/shm --tmpfs /home/ubuntu \
    --proc /proc --dev /dev --ro-bind-try /sys /sys \
    --dev-bind-try /dev/dri /dev/dri \
    --dev-bind-try /dev/nvidia0 /dev/nvidia0 \
    --dev-bind-try /dev/nvidiactl /dev/nvidiactl \
    --dev-bind-try /dev/nvidia-modeset /dev/nvidia-modeset \
    --bind "$coll" /home/ubuntu/.local/share/Anki2 \
    --bind "$coll/etc-service" /etc/service \
    --unshare-pid --unshare-ipc --unshare-uts --die-with-parent \
    /stream/start-seat.sh &
  PIDS+=("$!")
}

sample() {
  local ts seats_up=0 shared=0 priv=0 cpu=0
  # Build a ppid->children map once, then walk each bwrap pid's DESCENDANT
  # tree: bwrap's own smaps_rollup is just the wrapper (~2MB) — the seat's
  # real memory lives in its children (s6, Anki, QtWebEngine, selkies).
  local -A kids=()
  local sp pid ppid
  for sp in /proc/[0-9]*/stat; do
    pid="${sp#/proc/}"; pid="${pid%/stat}"
    ppid=$(sed 's/^[^)]*) //' "$sp" 2>/dev/null | awk '{print $2}') || true
    if [ -n "$ppid" ]; then kids[$ppid]="${kids[$ppid]:-} $pid"; fi
  done
  local p root frontier next tree sum_pid rss pss c
  for p in "${PIDS[@]:-}"; do
    [ -d "/proc/$p" ] || continue
    seats_up=$((seats_up + 1))
    frontier="$p"; tree=""
    while [ -n "$frontier" ]; do
      next=""
      for root in $frontier; do
        tree="$tree $root"
        next="$next${kids[$root]:-}"
      done
      frontier="$next"
    done
    for sum_pid in $tree; do
      rss=$(awk '/^Rss:/ {print $2}' "/proc/$sum_pid/smaps_rollup" 2>/dev/null) || true
      pss=$(awk '/^Pss:/ {print $2}' "/proc/$sum_pid/smaps_rollup" 2>/dev/null) || true
      c=$(awk '{int($14/100)}' "/proc/$sum_pid/stat" 2>/dev/null) || true
      rss=${rss:-0}; pss=${pss:-0}; c=${c:-0}
      shared=$((shared + rss))
      priv=$((priv + pss))
      cpu=$((cpu + c))
    done
  done
  local gpu="no"
  command -v nvidia-smi >/dev/null 2>&1 && \
    gpu=$(nvidia-smi --query-gpu=utilization.gpu --format=csv,noheader 2>/dev/null | head -1 || echo "no")
  echo "$(date +%s),$seats_up,$shared,$priv,$cpu,${gpu:-no}" >> "$OUT_CSV"
}

echo "[bench] spawning $SEATS seats (ramp ${RAMP}s) on ports $BASE_PORT+ ..."
for i in $(seq 1 "$SEATS"); do
  spawn_seat "$i" "$((BASE_PORT + i))"
  sleep "$RAMP"
done

echo "[bench] all seats up — sampling for $MINUTES min (active card flips via AnkiConnect) ..."
END=$((SECONDS + MINUTES * 60))
while [ $SECONDS -lt "$END" ]; do
  # Drive activity: flip the front card on every live seat (AnkiConnect).
  for i in $(seq 1 "$SEATS"); do
    port=$((18700 + i))
    curl -s -m 1 "http://127.0.0.1:$port" -X POST \
      -d '{"action":"guiDeckReview","version":6,"params":{"deck":"Default"}}' >/dev/null 2>&1 || true
    curl -s -m 1 "http://127.0.0.1:$port" -X POST \
      -d '{"action":"guiAnswerCard","version":6,"params":{"ease":3}}' >/dev/null 2>&1 || true
  done
  sample
  sleep 15
done

echo "[bench] DONE — samples in $OUT_CSV"
echo "[bench] summary (last sample):"
tail -1 "$OUT_CSV"
echo "[bench] Pss = proportional set size = the honest per-seat RAM cost."
echo "[bench] Record results in specs/strategy/2026-11-20-selkies-packaging-strategic-review.md"
