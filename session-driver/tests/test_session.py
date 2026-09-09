"""Tests for session.py — SessionManager state machine and lifecycle.

Requires Anki to be importable (run from within Anki's Python environment
or with ANKIDEV=1 for the Anki dev environment).
"""

import json
import time
import pytest


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

@pytest.fixture
def session_manager():
    """Return a fresh SessionManager in IDLE state."""
    from ankitov_session_driver.session import SessionManager

    return SessionManager()


# ---------------------------------------------------------------------------
# State transitions
# ---------------------------------------------------------------------------

class TestSessionLifecycle:
    """Happy-path: start → review → complete → idle."""

    def test_initial_state_is_idle(self, session_manager):
        assert session_manager.state == "IDLE"
        assert session_manager.session_uuid is None

    def test_get_status_idle(self, session_manager):
        status = session_manager.get_status()
        assert status["session_active"] is False
        assert status["state"] == "IDLE"

    def test_start_session_with_empty_capsule(self, session_manager):
        """Empty capsule returns 'empty' without creating deck."""
        result = session_manager.start_session(
            {
                "session_uuid": "test-uuid-001",
                "card_ids": [],
                "capsule_size": 0,
                "track_profile_name": "Test Profile",
            }
        )
        assert result["status"] == "empty"
        assert result["card_count"] == 0
        assert session_manager.state == "IDLE"

    def test_start_session_missing_uuid(self, session_manager):
        """Missing session_uuid returns error."""
        result = session_manager.start_session(
            {"card_ids": [1, 2, 3], "capsule_size": 3}
        )
        assert result["status"] == "error"
        assert "missing session_uuid" in result["error"]

    def test_cancel_session_when_idle(self, session_manager):
        """Cancelling when nothing is active returns error."""
        result = session_manager.cancel_session()
        assert result["status"] == "error"
        assert "No active session" in result["error"]

    def test_get_status_returns_all_fields(self, session_manager):
        """Status includes all expected keys."""
        status = session_manager.get_status()
        expected_keys = {
            "session_active",
            "session_uuid",
            "state",
            "track_profile_name",
            "cards_total",
            "cards_reviewed",
            "started_at",
        }
        assert expected_keys.issubset(set(status.keys()))


class TestCardCounting:
    """Card answering and counting logic."""

    def test_on_card_answered_increments_counter(self, session_manager, monkeypatch):
        """Answering a tagged card increments the counter."""
        session_manager.state = "REVIEWING"
        session_manager.session_uuid = "count-test-uuid"

        # Create a mock card that reports having our session tag
        class MockCard:
            def has_tag(self, tag):
                return tag == "ankitov-capsule-count-test-uuid"

        session_manager.on_card_answered(MockCard(), ease=3)
        assert session_manager.cards_reviewed == 1

    def test_on_card_answered_ignores_untagged(self, session_manager, monkeypatch):
        """Cards without the session tag do not increment."""
        session_manager.state = "REVIEWING"
        session_manager.session_uuid = "count-test-uuid"

        class MockOtherCard:
            def has_tag(self, tag):
                return False  # wrong tag

        session_manager.on_card_answered(MockOtherCard(), ease=3)
        assert session_manager.cards_reviewed == 0

    def test_on_card_answered_ignores_when_idle(self, session_manager):
        """No counting when not in REVIEWING state."""
        session_manager.state = "IDLE"

        class MockCard:
            def has_tag(self, tag):
                return True

        session_manager.on_card_answered(MockCard(), ease=3)
        assert session_manager.cards_reviewed == 0


class TestErrorHandling:
    """Edge cases from §6 of the spec."""

    def test_start_session_while_tagging(self, session_manager):
        """POST /session while TAGGING returns error."""
        session_manager.state = "TAGGING"
        session_manager.session_uuid = "mid-creation"

        result = session_manager.start_session(
            {
                "session_uuid": "new-uuid",
                "card_ids": [1, 2],
                "capsule_size": 2,
            }
        )
        assert result["status"] == "error"
        assert "in progress" in result["error"]

    def test_mark_complete_acknowledges(self, session_manager):
        """POST /session/complete acknowledges and resets state."""
        session_manager.state = "REVIEWING"
        session_manager.session_uuid = "complete-me"

        result = session_manager.mark_complete(
            {"session_uuid": "complete-me", "status": "completed"}
        )
        assert result["status"] == "acknowledged"
        assert session_manager.state == "IDLE"

    def test_mark_complete_wrong_uuid_ignored(self, session_manager):
        """Only the active session UUID is acknowledged."""
        session_manager.state = "REVIEWING"
        session_manager.session_uuid = "active-uuid"

        result = session_manager.mark_complete(
            {"session_uuid": "wrong-uuid", "status": "completed"}
        )
        assert result["status"] == "acknowledged"
        assert session_manager.state == "REVIEWING"  # unchanged


# ---------------------------------------------------------------------------
# State machine invariants
# ---------------------------------------------------------------------------

class TestStateMachineInvariants:
    """Verify that transitions respect the state machine diagram."""

    def test_cleanup_always_returns_to_idle(self, session_manager, monkeypatch):
        """After cleanup_and_notify, state MUST be IDLE."""
        # Simulate a session that ends
        session_manager.state = "REVIEWING"
        session_manager.session_uuid = "will-cleanup"
        session_manager.cards_reviewed = 5
        session_manager.capsule_size = 10

        # Mock mw.col to avoid actual Anki operations
        monkeypatch.setattr("aqt.mw.col", None)

        # Mock _notify_backend to avoid HTTP
        monkeypatch.setattr(
            session_manager, "_notify_backend", lambda *a, **kw: None
        )

        session_manager._do_cleanup_and_notify("completed")

        assert session_manager.state == "IDLE"
        assert session_manager.session_uuid is None
        assert session_manager.cards_reviewed == 0
        assert session_manager.started_at is None

    def test_start_then_cancel_leaves_clean_state(self, session_manager, monkeypatch):
        """start → cancel should leave state clean (IDLE)."""
        monkeypatch.setattr("aqt.mw.col", None)
        monkeypatch.setattr(
            session_manager, "_notify_backend", lambda *a, **kw: None
        )

        session_manager.session_uuid = "clean-test"
        session_manager.state = "REVIEWING"

        result = session_manager.cancel_session()
        assert result["status"] == "cancelled"
        assert session_manager.state == "IDLE"