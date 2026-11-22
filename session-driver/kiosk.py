# Copyright 2026 AnkiTov project owners
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, see the License for the
# specific language governing permissions and limitations under the License.
#
# This add-on runs inside Anki's Python interpreter (Anki is AGPL-3.0).
# It is deliberately licensed Apache-2.0 (AGPL-compatible) and talks to
# Anki only through public add-on hooks — Anki core files are never patched.

"""Seat kiosk: land the student directly in practice, lock the UI down.

Owner directive (2026-11-23): the student must land RIGHT in the practice
page — deck already chosen, everything disabled except the practice buttons
at the bottom. Higher-level students get menu access back.

Levels (env `ANKITOV_SEAT_KIOSK_LEVEL`, default 0 inside a seat):
  0  practice-only kiosk — menu bar hidden (Hide Menu Bar add-on), top bar
     hidden at runtime, deck auto-chosen, reviewer auto-started, deck-list
     escapes bounced back into practice
  1  guided — deck auto-chosen and opened, but the full UI stays available
  2  full Anki — no lockdown at all (also the default OUTSIDE a seat)

Seat mode is detected via `ANKITOV_SEAT_STUDENT` (set by SeatManager), so
this module is inert in normal desktop Anki and in the Testing Army's
supervisor-driven containers unless they set the same env.

The runtime toolbar hide (mw.toolbar.web.hide()) is deliberate: patch_prefs'
native-keys approach mis-fires when Anki materializes appearance keys lazily
and the absent-as-default assumption is wrong for the running build. The
runtime hide is what the Testing Army proved live.
"""

import os

from aqt import gui_hooks, mw
from aqt.qt import QTimer

# How long after profile-open we wait before the first auto-start (webviews
# and the collection need a moment; too early and moveToState is a no-op).
_STARTUP_DELAY_MS = 2500
# Guard cadence: bounce escapes + re-hide the toolbar.
_GUARD_INTERVAL_MS = 1500

_state = {
    "level": None,       # resolved kiosk level (int) or None = not resolved yet
    "started": False,    # initial auto-practice fired
    "deck": None,        # chosen deck name (for logs)
    "sessions": None,    # SessionManager (set by setup_kiosk) — capsule guard
}


def _level() -> int:
    if _state["level"] is None:
        if not os.environ.get("ANKITOV_SEAT_STUDENT"):
            _state["level"] = 2  # not a seat — never lock a desktop Anki
        else:
            try:
                _state["level"] = int(os.environ.get("ANKITOV_SEAT_KIOSK_LEVEL", "0"))
            except ValueError:
                _state["level"] = 0
    return _state["level"]


def _pick_deck():
    """The deck this seat practices: ANKITOV_SEAT_DECK (exact, then substring
    match), else the first non-empty deck that is not the stock Default."""
    if mw.col is None:
        return None
    want = os.environ.get("ANKITOV_SEAT_DECK", "").strip()
    names = [n for n in mw.col.decks.all_names() if n != "Default"]
    if want:
        for n in names:
            if n == want:
                return n
        for n in names:
            if want.lower() in n.lower():
                return n
        print(f"[ankitov-kiosk] ANKITOV_SEAT_DECK={want!r} not found; falling back")
    for n in sorted(names):
        if mw.col.decks.cids(mw.col.decks.id(n)):
            return n
    return names[0] if names else None


def _enter_practice() -> bool:
    """Select the deck and drop straight into the reviewer. True on success."""
    if mw.col is None:
        return False
    name = _pick_deck()
    if name is None:
        print("[ankitov-kiosk] no decks in the collection — honest empty state")
        return False
    try:
        did = mw.col.decks.id(name)
        mw.col.decks.select(did)
        # Straight to the reviewer — do NOT pass through the overview first.
        # The overview's web render is ASYNC: a back-to-back overview→review
        # lets the overview's late render overwrite the reviewer's content,
        # leaving state=review with the overview HTML on screen (live seat,
        # 2026-11-23: "unrecognized anki link: study" from reviewer.pyc).
        mw.moveToState("review")
        _state["deck"] = name
        print(f"[ankitov-kiosk] practice started: {name} (state now: {mw.state})")
        return True
    except Exception as e:  # noqa: BLE001 — never kill Anki over kiosk polish
        print(f"[ankitov-kiosk] auto-start failed (non-fatal): {e}")
        return False


def _hide_toolbar() -> None:
    try:
        if mw.toolbar is not None and mw.toolbar.web is not None:
            mw.toolbar.web.hide()
    except Exception:  # noqa: BLE001
        pass


def _hide_bottom_extras() -> None:
    """Best-effort: in the reviewer's bottom bar keep only the practice
    (answer) buttons — hide Edit/More, which are escape hatches into the
    editor/browser. Anki 26 renders them as id-less buttons wired through
    pycmd: <button onclick="pycmd('edit')"> — so match on the onclick
    attribute, never on ids. Unknown nodes no-op; safe every guard tick."""
    try:
        web = mw.reviewer.bottom.web
        if web is not None:
            web.eval(
                "document.querySelectorAll('button').forEach(b=>{"
                "const o=b.getAttribute('onclick')||'';"
                "if(o.includes(\"'edit'\")||o.includes(\"'more'\"))"
                "{b.style.display='none';}});true;"
            )
    except Exception:  # noqa: BLE001
        pass


def _guard() -> None:
    """Periodic kiosk enforcement (level 0 only, after the first start)."""
    if not _state["started"] or _level() != 0:
        return
    if mw.col is None or mw.state is None:
        return
    # Diagnostic heartbeat (every ~12s) — cheap and invaluable live.
    _state["tick"] = _state.get("tick", 0) + 1
    if _state["tick"] % 8 == 0:
        print(f"[ankitov-kiosk] heartbeat state={mw.state}")
    # A capsule session owns the UI while it runs — never fight it.
    try:
        sessions = _state["sessions"]
        if sessions is not None and sessions.state != "IDLE":
            return
    except Exception:  # noqa: BLE001
        pass
    if mw.state == "deckBrowser":
        # Escaped to the deck list — bounce straight back into practice.
        print("[ankitov-kiosk] deckBrowser escape bounced back to practice")
        _enter_practice()
    _hide_toolbar()
    if mw.state == "review":
        _hide_bottom_extras()


def _on_profile_open() -> None:
    if _level() >= 2:
        return
    QTimer.singleShot(_STARTUP_DELAY_MS, _on_startup_tick)


def _on_startup_tick() -> None:
    if mw.col is None:
        # Profile still loading — retry on the same cadence as the guard.
        QTimer.singleShot(_GUARD_INTERVAL_MS, _on_startup_tick)
        return
    if _level() == 0:
        _hide_toolbar()
        if _enter_practice():
            _state["started"] = True
    else:
        # Level 1: open the deck list on the chosen deck, leave UI free.
        name = _pick_deck()
        if name is not None:
            try:
                mw.col.decks.select(mw.col.decks.id(name))
                _state["deck"] = name
            except Exception:  # noqa: BLE001
                pass
        _state["started"] = True


def setup_kiosk(session_manager=None) -> None:
    _state["sessions"] = session_manager
    lvl = _level()
    if lvl >= 2:
        print("[ankitov-kiosk] level 2 — full Anki, no lockdown")
        return
    gui_hooks.profile_did_open.append(_on_profile_open)
    timer = QTimer(mw)
    timer.timeout.connect(_guard)
    timer.start(_GUARD_INTERVAL_MS)
    print(f"[ankitov-kiosk] level {lvl} armed (auto-practice"
          f"{' + lockdown' if lvl == 0 else ''})")
