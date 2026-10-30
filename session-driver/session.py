"""Session lifecycle management for AnkiTov capsule delivery.

State machine: IDLE → TAGGING → REVIEWING → CLEANUP → IDLE

Public methods are called by the HTTP handler (server.py) and Anki hooks.
All methods return a dict suitable for JSON serialization in the HTTP response.
"""

import json
import threading
import time
import urllib.request
from typing import Any, Dict, List, Optional

from aqt import mw
from anki.decks import DeckId
from anki.collection import Collection

from .cleanup import cleanup_session


# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

DECK_NAME = "AnkiTov Capsule"
TAG_PREFIX = "ankitov-capsule-"

# HTTP timeout for notifying backend (seconds)
CALLBACK_TIMEOUT = 5


def _post_json(path: str, payload: Dict[str, Any], log_label: str = "") -> None:
    """POST JSON to the backend — blocking, best-effort, call off the UI thread.

    T-180 helper: all outbound backend notifications funnel through here so
    the paths live in ONE place. Callers on Anki's UI thread must wrap this
    in a daemon thread (see SessionManager._post_card_telemetry).
    """
    from .server import _server

    backend_url = "http://host.docker.internal:5150"
    if _server is not None:
        backend_url = _server.backend_url

    url = f"{backend_url.rstrip('/')}{path}"
    data = json.dumps(payload).encode("utf-8")
    try:
        req = urllib.request.Request(
            url,
            data=data,
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        urllib.request.urlopen(req, timeout=CALLBACK_TIMEOUT)
        print(f"[ankitov-session-driver] Backend notified: {path} {log_label}")
    except Exception as e:  # noqa: BLE001 — best-effort telemetry only
        print(
            f"[ankitov-session-driver] Backend notification failed "
            f"({path}): {e}"
        )


# ---------------------------------------------------------------------------
# Session Manager
# ---------------------------------------------------------------------------

class SessionManager:
    """Manages the lifecycle of one active capsule session at a time."""

    def __init__(self):
        # Core state
        self.state: str = "IDLE"
        self.session_uuid: str | None = None
        self.track_profile_name: str | None = None
        self.capsule_size: int = 0
        self.card_ids: List[int] = []
        self.cards_reviewed: int = 0
        self.started_at: str | None = None
        # monotonic timestamp of the previous card answer (for time_ms telemetry)
        self._card_started_at: float | None = None

    # ── Hook handlers ────────────────────────────────────────────────

    def on_card_answered(self, card, ease):
        """Increment counter each time a card is answered.

        T-180: also fires per-card telemetry to the backend's real ingestion
        path (`POST /api/v1/telemetry/card-answer`). Previously this hook only
        bumped a local counter and the only outbound call was a session-level
        POST to `/session/complete` — a route the backend never had, so the
        server's audit ledger and session-progress counters never saw this
        addon's answers (discovered by the Testing Army fleet, Oct 2026).
        """
        # Only count if we're in REVIEWING state and this card belongs to
        # the active session (tag check is a belts-and-suspenders guard).
        if self.state != "REVIEWING":
            return

        if self.session_uuid is None:
            return

        tag = f"{TAG_PREFIX}{self.session_uuid}"
        if card.has_tag(tag):
            self.cards_reviewed += 1
            self._post_card_telemetry(card, ease)

    def _post_card_telemetry(self, card, ease):
        """POST one card-answer event to /api/v1/telemetry/card-answer.

        Best-effort and non-blocking: runs on a daemon thread because Anki
        hooks fire on the UI thread — a synchronous HTTP call here would
        freeze the review surface for up to the timeout on every answer.
        Telemetry must NEVER break the review flow: any failure here is
        swallowed (the local counter already advanced; the server's audit
        ledger tolerates best-effort ingestion).
        """
        try:
            now = time.monotonic()
            started = self._card_started_at
            self._card_started_at = now
            time_ms = 10_000 if started is None else min(int((now - started) * 1000), 600_000)

            deck_name = ""
            col = getattr(mw, "col", None)
            if col is not None:
                try:
                    deck_name = col.decks.name(card.did)
                except Exception:  # noqa: BLE001 — deck name is best-effort metadata
                    deck_name = ""

            payload = {
                "student_id": self.track_profile_name or "unknown",
                "card_id": int(card.id),
                "deck_name": deck_name,
                "ease": int(ease),
                "time_ms": time_ms,
                "session_uuid": self.session_uuid,
            }
            threading.Thread(
                target=_post_json,
                args=("/api/v1/telemetry/card-answer", payload),
                daemon=True,
            ).start()
        except Exception as e:  # noqa: BLE001 — telemetry is fire-and-forget
            print(f"[ankitov-session-driver] Card telemetry skipped: {e}")

    def on_reviewer_end(self):
        """Called by gui_hooks.reviewer_will_end when the reviewer closes."""
        if self.state != "REVIEWING":
            return

        status = "completed" if self.cards_reviewed >= self.capsule_size else "aborted"
        self._do_cleanup_and_notify(status)

    # ── HTTP handlers (called from server.py) ────────────────────────

    def start_session(self, payload: Dict[str, Any]) -> Dict[str, Any]:
        """POST /session — start a new capsule review session.

        Aborts any active session first.
        """
        session_uuid = payload.get("session_uuid", "")
        card_ids = payload.get("card_ids", [])
        capsule_size = payload.get("capsule_size", len(card_ids))
        track_profile_name = payload.get("track_profile_name", "Unknown")

        # Validate
        if not session_uuid:
            return {"status": "error", "error": "missing session_uuid"}

        # Guard: don't start if we're in TAGGING (mid-creation)
        if self.state == "TAGGING":
            return {
                "status": "error",
                "error": "Session creation in progress. Retry.",
                "session_uuid": self.session_uuid,
            }

        # Abort any active session
        replaced = None
        if self.state in ("REVIEWING",):
            replaced = self.session_uuid
            self._do_cleanup_and_notify("replaced")

        # Handle empty capsule
        if not card_ids or capsule_size == 0:
            self.state = "IDLE"
            self.session_uuid = session_uuid
            return {
                "status": "empty",
                "session_uuid": session_uuid,
                "card_count": 0,
            }

        # ── State → TAGGING ──
        self.state = "TAGGING"
        self.session_uuid = session_uuid
        self.track_profile_name = track_profile_name
        self.capsule_size = capsule_size
        self.card_ids = card_ids
        self.cards_reviewed = 0
        self.started_at = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())

        try:
            self._create_filtered_session()
        except Exception as e:
            print(f"[ankitov-session-driver] Failed to create session: {e}")
            self.state = "IDLE"
            return {
                "status": "error",
                "error": str(e),
                "session_uuid": session_uuid,
            }

        response = {
            "status": "replaced" if replaced else "started",
            "session_uuid": session_uuid,
            "deck_name": DECK_NAME,
            "card_count": capsule_size,
        }
        if replaced:
            response["replaced_session"] = replaced

        return response

    def get_status(self) -> Dict[str, Any]:
        """GET /status — return current session state."""
        return {
            "session_active": self.state in ("TAGGING", "REVIEWING"),
            "session_uuid": self.session_uuid,
            "state": self.state,
            "track_profile_name": self.track_profile_name,
            "cards_total": self.capsule_size,
            "cards_reviewed": self.cards_reviewed,
            "started_at": self.started_at,
        }

    def cancel_session(self) -> Dict[str, Any]:
        """DELETE /session — force-cancel the active session."""
        if self.state not in ("TAGGING", "REVIEWING"):
            return {
                "status": "error",
                "error": "No active session to cancel",
            }

        uuid = self.session_uuid
        self._do_cleanup_and_notify("cancelled")
        return {
            "status": "cancelled",
            "session_uuid": uuid,
        }

    def mark_complete(self, payload: Dict[str, Any]) -> Dict[str, Any]:
        """POST /session/complete — the addon itself calls this internally."""
        # This is a pass-through for cases where the backend queries the
        # addon about a session. The actual completion notification is
        # sent outward to the backend via _notify_backend().
        session_uuid = payload.get("session_uuid", "")
        if session_uuid and session_uuid == self.session_uuid:
            self.state = "IDLE"
        return {"status": "acknowledged", "session_uuid": session_uuid}

    # ── Internal methods ─────────────────────────────────────────────

    def _create_filtered_session(self):
        """Create filtered deck, tag cards, open reviewer. Assumes TAGGING state."""
        session_uuid = self.session_uuid
        if session_uuid is None:
            raise RuntimeError("No session UUID in TAGGING state")

        col = mw.col
        if col is None:
            raise RuntimeError("Collection not loaded")

        tag = f"{TAG_PREFIX}{session_uuid}"

        # Step 1: Tag the capsule cards
        col.tags.bulk_add(self.card_ids, tag)

        # Step 2: Create filtered deck
        did = col.decks.new_filtered(DECK_NAME)
        deck = col.decks.get(did)
        deck["terms"] = [
            [
                f"tag:{tag}",
                self.capsule_size,
                1,  # order=1 (random — preserves interleaving)
            ]
        ]
        deck["resched"] = False   # Don't override FSRS scheduling
        deck["delays"] = None      # Use card's own learning steps
        col.decks.save(deck)

        # Step 3: Rebuild (populates deck with matching cards)
        col.sched.rebuild_filtered_deck(did)

        # Step 4: Select deck and open reviewer
        col.decks.select(did)
        mw.onReview()

        # ── State → REVIEWING ──
        self.state = "REVIEWING"

    def _do_cleanup_and_notify(self, status: str):
        """Run cleanup and notify backend. Always returns to IDLE."""
        uuid = self.session_uuid
        reviewed = self.cards_reviewed

        # Cleanup (tags + deck)
        if mw.col is not None and uuid is not None:
            cleanup_session(uuid, mw.col)

        # Notify backend (fire-and-forget, best-effort)
        if uuid is not None:
            self._notify_backend(uuid, reviewed, status)

        # Reset state
        self.state = "IDLE"
        self.session_uuid = None
        self.track_profile_name = None
        self.capsule_size = 0
        self.card_ids = []
        self.cards_reviewed = 0
        self.started_at = None
        self._card_started_at = None

    def _notify_backend(self, session_uuid: str, cards_reviewed: int, status: str):
        """POST session-complete signal to the Loco.rs backend. Best-effort.

        T-180: the real route is `/api/v1/telemetry/session-complete` — the
        old `/session/complete` path never existed server-side (404).
        """
        _post_json(
            "/api/v1/telemetry/session-complete",
            {
                "session_uuid": session_uuid,
                "cards_reviewed": cards_reviewed,
                "status": status,
            },
            log_label=f"session {session_uuid} status={status} reviewed={cards_reviewed}",
        )