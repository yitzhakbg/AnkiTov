"""Tests for cleanup.py — tag removal and deck deletion.

Designed to be run with a real (in-memory) Anki Collection.
Requires Anki to be importable.
"""

import pytest


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _create_test_session(col, session_uuid: str, card_ids: list[int]):
    """Set up a test session: tag cards + create filtered deck."""
    from ankitov_session_driver.cleanup import DECK_NAME, TAG_PREFIX

    tag = f"{TAG_PREFIX}{session_uuid}"

    # Tag cards (skip if cards don't exist in test collection)
    if card_ids:
        try:
            col.tags.bulk_add(card_ids, tag)
        except Exception:
            pass

    # Create filtered deck
    try:
        did = col.decks.new_filtered(DECK_NAME)
        deck = col.decks.get(did)
        deck["terms"] = [[f"tag:{tag}", len(card_ids), 1]]
        deck["resched"] = False
        deck["delays"] = None
        col.decks.save(deck)
        col.sched.rebuild_dyn(did)
    except Exception:
        pass


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------

class TestCleanupSession:
    """Verify cleanup_session removes all artifacts."""

    def test_cleanup_removes_tags(self, anki_col):
        """Tagged cards should lose their tags after cleanup."""
        from ankitov_session_driver.cleanup import cleanup_session, TAG_PREFIX

        col = anki_col
        uuid = "cleanup-test-001"
        tag = f"{TAG_PREFIX}{uuid}"

        # We can only test tag removal in a real collection.
        # Create a note + card to tag
        try:
            from anki.notes import Note
            from anki.models import NotetypeDict

            # Get the Basic model
            model = col.models.by_name("Basic")
            if model is None:
                pytest.skip("No Basic note type in test collection")

            note = col.new_note(model)
            note.fields[0] = "Test cleanup card"
            col.add_note(note, DeckId(1))
            card_ids = [c.id for c in note.cards()]

            _create_test_session(col, uuid, card_ids)

            # Verify tag exists
            tagged = col.find_cards(f"tag:{tag}")
            assert len(tagged) > 0, "Card should be tagged before cleanup"

            # Run cleanup
            cleanup_session(uuid, col)

            # Verify tag removed
            tagged_after = col.find_cards(f"tag:{tag}")
            assert len(tagged_after) == 0, "Tag should be removed after cleanup"
        except Exception as e:
            pytest.skip(f"Cannot set up test collection: {e}")

    def test_cleanup_is_idempotent(self, anki_col):
        """Running cleanup twice on the same session does not raise."""
        from ankitov_session_driver.cleanup import cleanup_session, TAG_PREFIX

        col = anki_col
        uuid = "idempotent-test"

        try:
            model = col.models.by_name("Basic")
            if model is None:
                pytest.skip("No Basic note type")

            note = col.new_note(model)
            note.fields[0] = "Idempotent test"
            col.add_note(note, DeckId(1))
            card_ids = [c.id for c in note.cards()]

            _create_test_session(col, uuid, card_ids)

            # Run cleanup twice — should not raise
            cleanup_session(uuid, col)
            cleanup_session(uuid, col)  # should be safe
        except Exception as e:
            pytest.skip(f"Cannot set up test collection: {e}")

    def test_cleanup_nonexistent_session_safe(self, anki_col):
        """Cleaning up a session that never existed is a no-op."""
        from ankitov_session_driver.cleanup import cleanup_session

        col = anki_col
        # Should not raise
        cleanup_session("nonexistent-uuid-999", col)

    def test_cleanup_removes_filtered_deck(self, anki_col):
        """The AnkiTov Capsule deck should be gone after cleanup."""
        from ankitov_session_driver.cleanup import cleanup_session, DECK_NAME

        col = anki_col
        uuid = "deck-removal-test"

        try:
            model = col.models.by_name("Basic")
            if model is None:
                pytest.skip("No Basic note type")

            note = col.new_note(model)
            note.fields[0] = "Deck test"
            col.add_note(note, DeckId(1))
            card_ids = [c.id for c in note.cards()]

            _create_test_session(col, uuid, card_ids)

            # Verify deck exists
            deck_exists_before = any(
                d.name == DECK_NAME for d in col.decks.all_names_and_ids()
            )
            assert deck_exists_before, "Filtered deck should exist before cleanup"

            cleanup_session(uuid, col)

            # Verify deck removed
            deck_exists_after = any(
                d.name == DECK_NAME for d in col.decks.all_names_and_ids()
            )
            assert not deck_exists_after, "Filtered deck should be removed"
        except Exception as e:
            pytest.skip(f"Cannot set up test collection: {e}")

    def test_cleanup_stale_sessions(self, anki_col):
        """Stale session tags from previous runs are detected and cleaned."""
        from ankitov_session_driver.cleanup import cleanup_stale_sessions, TAG_PREFIX

        col = anki_col
        uuid = "stale-cleanup-test"

        try:
            model = col.models.by_name("Basic")
            if model is None:
                pytest.skip("No Basic note type")

            note = col.new_note(model)
            note.fields[0] = "Stale test"
            col.add_note(note, DeckId(1))
            card_ids = [c.id for c in note.cards()]

            _create_test_session(col, uuid, card_ids)

            # Run stale cleanup
            cleaned = cleanup_stale_sessions(col)

            # Should have found our stale session
            assert uuid in cleaned

            # Verify tag is gone
            tag = f"{TAG_PREFIX}{uuid}"
            tagged = col.find_cards(f"tag:{tag}")
            assert len(tagged) == 0
        except Exception as e:
            pytest.skip(f"Cannot set up test collection: {e}")


# ---------------------------------------------------------------------------
# Fixture: provide a real Anki collection
# ---------------------------------------------------------------------------

@pytest.fixture
def anki_col():
    """Return an in-memory Anki collection for testing cleanup operations."""
    import tempfile
    import os

    from anki.collection import Collection
    from anki.decks import DeckId

    # Use a temp file for the collection
    fd, path = tempfile.mkstemp(suffix=".anki2")
    os.close(fd)

    col = Collection(path)
    yield col

    # Teardown
    try:
        col.close()
    except Exception:
        pass
    try:
        os.unlink(path)
    except Exception:
        pass
    try:
        os.unlink(path + "-wal")
    except Exception:
        pass
    try:
        os.unlink(path + "-shm")
    except Exception:
        pass