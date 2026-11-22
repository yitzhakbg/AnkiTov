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
trap cleanup EXIT

echo "sample_ts,seats_up,rss_shared_kb,rss_private_kb,cpu_percent,gpu_used" > "$OUT_CSV"

spawn_seat() {
  local n="$1" port="$2"
  local coll="$COLL_ROOT/bench-$n"
  mkdir -p "$coll"
  # Same sandbox shape as SeatManager (backend/src/services/seat_manager.rs).
  bwrap \
    --ro-bind "$ROOTFS" / \
    --tmpfs /data --tmpfs /tmp --proc /proc --dev /dev \
    --dev-bind-try /dev/dri /dev/dri \
    --dev-bind-try /dev/nvidia0 /dev/nvidia0 \
    --dev-bind-try /dev/nvidiactl /dev/nvidiactl \
    --dev-bind-try /dev/nvidia-modeset /dev/nvidia-modeset \
    --bind "$coll" /home/ubuntu/.local/share/Anki2 \
    --unshare-pid --unshare-ipc --unshare-uts --die-with-parent \
    /stream/start-seat.sh &
  PIDS+=("$!")
}

sample() {
  local ts seats_up=0 shared=0 priv=0 cpu=0
  for p in "${PIDS[@]:-}"; do
    [ -d "/proc/$p" ] || continue
    seats_up=$((seats_up + 1))
    # smaps_rollup: Rss (total incl. shared) + Pss (proportional = the honest
    # per-seat cost) + Private (this seat's exclusive pages).
    local rss pss
    rss=$(awk '/^Rss:/ {print $2}' "/proc/$p/smaps_rollup" 2>/dev/null || echo 0)
    pss=$(awk '/^Pss:/ {print $2}' "/proc/$p/smaps_rollup" 2>/dev/null || echo 0)
    shared=$((shared + rss))
    priv=$((priv + pss))
    cpu=$((cpu + $(awk '{int($14/100)}' "/proc/$p/stat" 2>/dev/null || echo 0)))
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
      -d '{"action":"guiDeckReview","version":6,"params":{"deck":"default"}}' >/dev/null 2>&1 || true
  done
  sample
  sleep 15
done

echo "[bench] DONE — samples in $OUT_CSV"
echo "[bench] summary (last sample):"
tail -1 "$OUT_CSV"
echo "[bench] Pss = proportional set size = the honest per-seat RAM cost."
echo "[bench] Record results in specs/strategy/2026-11-20-selkies-packaging-strategic-review.md"
