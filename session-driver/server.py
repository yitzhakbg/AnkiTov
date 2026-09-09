"""Minimal non-blocking HTTP server for Anki addons.

Pattern: raw socket + select + QTimer pump.
Why: Threads freeze Qt. http.server blocks. This runs on Qt's event loop.

Adapted from the battle-tested pattern in AnkiConnect (2055492159, AGPL-3.0),
which has been stable across every Anki version since 2.1.22.
"""

import json
import select
import socket
import traceback
from aqt.qt import QTimer

# ---------------------------------------------------------------------------
# Module-level singleton for start/stop lifecycle
# ---------------------------------------------------------------------------

_server: "WebServer | None" = None


class WebServer:
    """Single-client HTTP server on localhost. Non-blocking."""

    def __init__(self, port: int, session_manager, backend_url: str):
        self.port = port
        self.session_manager = session_manager
        self.backend_url = backend_url
        self.sock: socket.socket | None = None
        self.client: socket.socket | None = None
        self.timer: QTimer | None = None
        self._read_buf = b""
        self._write_buf = b""

    # ── Lifecycle ────────────────────────────────────────────────────

    def listen(self):
        self.sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.sock.setblocking(False)
        self.sock.bind(("127.0.0.1", self.port))
        self.sock.listen(1)
        self.timer = QTimer()
        self.timer.timeout.connect(self._advance)
        self.timer.start(25)  # 40 polls/sec — negligible CPU

    def stop(self):
        self._close_client()
        if self.timer is not None:
            self.timer.stop()
            self.timer = None
        if self.sock is not None:
            try:
                self.sock.close()
            except OSError:
                pass
            self.sock = None

    # ── Internal pump ────────────────────────────────────────────────

    def _advance(self):
        """Pump: accept new connections, read requests, send responses."""
        if self.client is None and self.sock is not None:
            # Accept new connection
            r, _, _ = select.select([self.sock], [], [], 0)
            if r:
                try:
                    conn, _addr = self.sock.accept()
                    conn.setblocking(False)
                    self.client = conn
                    self._read_buf = b""
                except BlockingIOError:
                    pass
                except OSError:
                    pass
            return

        if self.client is None:
            return

        # Read from client
        try:
            data = self.client.recv(4096)
            if not data:
                self._close_client()
                return
            self._read_buf += data
        except BlockingIOError:
            pass
        except (ConnectionResetError, OSError):
            self._close_client()
            return

        # Parse HTTP request — look for header-body separator
        if b"\r\n\r\n" in self._read_buf:
            response = self._handle(self._read_buf)
            self._write_buf = response
            self._read_buf = b""

        # Write response
        if self._write_buf:
            try:
                sent = self.client.send(self._write_buf)
                self._write_buf = self._write_buf[sent:]
            except BlockingIOError:
                pass
            except (ConnectionResetError, OSError):
                self._close_client()

    def _handle(self, raw: bytes) -> bytes:
        """Parse request, dispatch to handler, return HTTP response bytes."""
        try:
            decoded = raw.decode("utf-8", errors="replace")
            lines = decoded.split("\r\n")
            if not lines or " " not in lines[0]:
                return _http_response(400, {"status": "error", "error": "malformed request"})

            method, path, _version = lines[0].split(" ", 2)

            # Extract body
            body = ""
            parts = decoded.split("\r\n\r\n", 1)
            if len(parts) > 1:
                body = parts[1]

            payload = json.loads(body) if body.strip() else {}

            # Dispatch
            if method == "POST" and path == "/session":
                result = self.session_manager.start_session(payload)
            elif method == "GET" and path == "/status":
                result = self.session_manager.get_status()
            elif method == "DELETE" and path == "/session":
                result = self.session_manager.cancel_session()
            elif method == "POST" and path == "/session/complete":
                result = self.session_manager.mark_complete(payload)
            else:
                result = {"status": "error", "error": f"not found: {method} {path}"}

            return _http_response(200, result)

        except json.JSONDecodeError as e:
            return _http_response(400, {"status": "error", "error": f"invalid JSON: {e}"})
        except Exception:
            tb = traceback.format_exc()
            print(f"[ankitov-session-driver] Handler exception:\n{tb}")
            return _http_response(500, {"status": "error", "error": "internal error"})

    def _close_client(self):
        if self.client is not None:
            try:
                self.client.close()
            except OSError:
                pass
            self.client = None
            self._read_buf = b""
            self._write_buf = b""


# ---------------------------------------------------------------------------
# Module helpers
# ---------------------------------------------------------------------------

def _http_response(status: int, body: dict) -> bytes:
    """Encode a JSON dict as an HTTP response."""
    resp_body = json.dumps(body)
    status_text = {200: "OK", 400: "Bad Request", 500: "Internal Server Error"}.get(
        status, "OK"
    )
    return (
        f"HTTP/1.1 {status} {status_text}\r\n"
        f"Content-Type: application/json\r\n"
        f"Content-Length: {len(resp_body)}\r\n"
        f"Connection: close\r\n"
        f"\r\n"
        f"{resp_body}"
    ).encode("utf-8")


# ---------------------------------------------------------------------------
# Public API
# ---------------------------------------------------------------------------

def start_server(port: int, session_manager, backend_url: str):
    """Start the HTTP listener. Called from __init__.py on addon load."""
    global _server
    _server = WebServer(port, session_manager, backend_url)
    try:
        _server.listen()
        print(f"[ankitov-session-driver] Server listening on :{port}")
    except OSError as e:
        print(f"[ankitov-session-driver] FATAL: cannot bind :{port}: {e}")


def stop_server():
    """Stop the HTTP listener. Called on addon unload / Anki shutdown."""
    global _server
    if _server is not None:
        _server.stop()
        _server = None