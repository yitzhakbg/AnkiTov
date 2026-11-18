"""
AnkiTov Session Driver Addon.

Bridges server-side IMP capsules to Anki's reviewer inside the Selkies
container. Listens on 127.0.0.1:18765 for session commands from the
Loco.rs backend.

Configuration (environment variables):
    ANKITOV_CAPSULE_PORT     (default: 18765)  — HTTP listener port
    ANKITOV_BACKEND_URL      (required)         — backend origin; the addon posts per-card
                                                telemetry to /api/v1/telemetry/card-answer and
                                                session signals to /api/v1/telemetry/session-complete
                                                (T-180: the old /session/complete path never existed)

Startup: the HTTP server starts automatically when Anki loads this addon.
It runs on Qt's event loop via QTimer — no threads, no blocking.
"""

import os
from aqt import gui_hooks

from .server import start_server
from .session import SessionManager
from .cleanup import cleanup_on_collection_close, cleanup_stale_sessions

# Global session manager — initialized once per Anki process
session_manager = SessionManager()

# Start the HTTP session listener (socket + QTimer pattern)
port = int(os.environ.get("ANKITOV_CAPSULE_PORT", "18765"))
backend_url = os.environ.get(
    "ANKITOV_BACKEND_URL", "http://localhost:5150"
)
start_server(port, session_manager, backend_url)

# Register hooks
gui_hooks.reviewer_did_answer_card.append(session_manager.on_card_answered)
gui_hooks.reviewer_will_end.append(session_manager.on_reviewer_end)
gui_hooks.profile_will_close.append(cleanup_on_collection_close)


def _cleanup_stale_on_open() -> None:
    """Self-heal artifacts from an unclean shutdown (e.g. Anki killed before
    profile_will_close could run): remove stale session tags and the leftover
    `AnkiTov Capsule` filtered deck. No-op when the collection is clean."""
    from aqt import mw

    if mw.col is None:
        return
    try:
        stale = cleanup_stale_sessions(mw.col)
        if stale:
            print(
                f"[ankitov-session-driver] Open cleanup: "
                f"removed {len(stale)} stale session(s)"
            )
    except Exception as e:
        print(f"[ankitov-session-driver] Open cleanup failed (non-fatal): {e}")


gui_hooks.profile_did_open.append(_cleanup_stale_on_open)

print(
    f"[ankitov-session-driver] Initialized — listening on 127.0.0.1:{port}"
)
print(f"[ankitov-session-driver] Backend URL: {backend_url}")