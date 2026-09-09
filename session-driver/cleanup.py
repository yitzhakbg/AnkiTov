"""Cleanup routines for AnkiTov capsule sessions.

Pure functions that remove session artifacts (tags, filtered decks)
from an Anki collection. Designed to be idempotent — safe to call
multiple times on the same session.
"""

from anki.collection import Collection


TAG_PREFIX = "ankitov-capsule-"
DECK_NAME = "AnkiTov Capsule"


def cleanup_session(session_uuid: str, col: Collection) -> None:
    """Remove all artifacts for a capsule session. Idempotent.

    Args:
        session_uuid: The session UUID (without tag prefix).
        col: An Anki Collection instance (mw.col).
    """
    tag = f"{TAG_PREFIX}{session_uuid}"

    # Step 1: Remove tags from all cards that still carry them
    _remove_session_tag(col, tag)

    # Step 2: Delete the filtered deck
    _remove_session_deck(col)


def cleanup_stale_sessions(col: Collection) -> list[str]:
    """Find and clean up any stale capsule artifacts from previous runs.

    Scans for all tags matching {TAG_PREFIX}* and removes them along
    with the filtered deck. Used on collection open to recover from
    an unclean shutdown.

    Returns:
        List of stale session UUIDs that were cleaned up.
    """
    cleaned = []

    # Find all tags starting with our prefix
    try:
        all_tags = col.tags.all()
    except Exception:
        return cleaned

    for tag_entry in all_tags:
        tag_name = tag_entry.name if hasattr(tag_entry, "name") else str(tag_entry)
        if tag_name.startswith(TAG_PREFIX):
            session_uuid = tag_name[len(TAG_PREFIX):]
            _remove_session_tag(col, tag_name)
            cleaned.append(session_uuid)

    _remove_session_deck(col)

    if cleaned:
        print(
            f"[ankitov-session-driver] Cleaned {len(cleaned)} stale session(s): "
            f"{', '.join(cleaned[:5])}{'...' if len(cleaned) > 5 else ''}"
        )

    return cleaned


def cleanup_on_collection_close():
    """Hook handler for gui_hooks.profile_will_close.

    Performs fail-safe cleanup of any active session artifacts.
    Called when Anki is shutting down.
    """
    from aqt import mw

    if mw.col is None:
        return

    try:
        stale = cleanup_stale_sessions(mw.col)
        if stale:
            print(
                f"[ankitov-session-driver] Shutdown cleanup: "
                f"removed {len(stale)} stale session(s)"
            )
    except Exception as e:
        print(f"[ankitov-session-driver] Shutdown cleanup failed (non-fatal): {e}")


# ---------------------------------------------------------------------------
# Internal helpers
# ---------------------------------------------------------------------------

def _remove_session_tag(col: Collection, tag: str) -> None:
    """Remove a tag from all cards that carry it. No-op if no cards match."""
    try:
        card_ids = col.find_cards(f"tag:{tag}")
    except Exception:
        return  # tag doesn't exist or other error — safe to ignore

    if card_ids:
        try:
            col.tags.bulk_remove(card_ids, tag)
        except Exception as e:
            print(
                f"[ankitov-session-driver] Warning: tag removal failed for "
                f"'{tag}': {e}"
            )


def _remove_session_deck(col: Collection) -> None:
    """Find and delete the AnkiTov Capsule filtered deck. No-op if absent."""
    try:
        all_decks = col.decks.all_names_and_ids()
    except Exception:
        return

    for deck_entry in all_decks:
        deck_name = deck_entry.name if hasattr(deck_entry, "name") else str(deck_entry)
        if deck_name == DECK_NAME:
            try:
                deck_id = deck_entry.id if hasattr(deck_entry, "id") else int(deck_entry)
                col.decks.remove([deck_id])
                print(f"[ankitov-session-driver] Removed deck '{DECK_NAME}'")
            except Exception as e:
                print(
                    f"[ankitov-session-driver] Warning: deck removal failed: {e}"
                )
            break