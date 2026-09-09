"""
AnkiTov Session Driver Addon.

Bridges server-side IMP capsules to Anki's reviewer inside the Selkies
container. Listens on 127.0.0.1:18765 for session commands from the
Loco.rs backend.

Configuration (environment variables):
    ANKITOV_CAPSULE_PORT     (default: 18765)  — HTTP listener port
    ANKITOV_BACKEND_URL      (required)         — where to POST /session/complete

Startup: the HTTP server starts automatically when Anki loads this addon.
It runs on Qt's event loop via QTimer — no threads, no blocking.
"""

import os
from aqt import gui_hooks

from .server import start_server
from .session import SessionManager
from .cleanup import cleanup_on_collection_close

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

print(
    f"[ankitov-session-driver] Initialized — listening on 127.0.0.1:{port}"
)
print(f"[ankitov-session-driver] Backend URL: {backend_url}")