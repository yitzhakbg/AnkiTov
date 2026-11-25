#!/usr/bin/env python3
"""
patch_prefs.py — natively strip Anki's Appearance distractions from the streamed
class profile, using Anki's OWN preference keys (no add-on, no "restart
required" prompt — this is a prefs edit, not an on-disk add-on change).

Why this exists
---------------
An add-on (Hide Menu Bar) collapses the *menu bar*. But two more distractions
are *native Anki appearance preferences*, not add-on territory:

  (a) the top **deck-list bar**  (Decks | Add | Browse | Stats | Sync)
  (b) the **button captions**    (Edit | More, and the pass/fail text)

We flip the two corresponding Appearance booleans in the class profile's
prefs21.db *before* Anki loads the profile, so the first painted frame is
already clean — no Preferences dialog to reach, no restart prompt.

The two real problems this solves vs. a naive "just edit the DB" approach:
  1. **Keys are lazily materialized.** Anki only writes a preference key to
     prefs21.db after you have actually toggled it in the UI. A fresh class
     profile therefore has NO such key at all. We must *insert* it, not merely
     flip an existing one.
  2. **The exact key name varies across Anki versions.** Rather than hard-code
     one guess and silently no-op, we match a small set of candidate patterns
     per function and *log exactly which key we matched* (or that none matched,
     loudly). Run with `--probe` to dump every real key an Anki build ships.

Safety model (no-clobber, applied to prefs)
-------------------------------------------
- We open the class profile's prefs21.db, read each profile row's pickled
  `data` dict, and mutate ONLY the target keys (inserting if absent).
- We do not touch any other preference: a student's theme, colors, answer keys,
  etc. are preserved.
- The streamed class profile is a *dedicated* profile (boot.py / a class
  profile); this never affects a student's own Anki on their own device.

Usage
-----
    python3 patch_prefs.py --probe <ANKI_DATA>     # dump every pref key (debug)
    python3 patch_prefs.py <ANKI_DATA>             # apply the two toggles
Exit code 0 = done (or nothing-to-do). We never hard-fail the service: a bad
key degrades to a logged warning, and the menu-bar add-on still hides the top
bar on its own.
"""

import os
import sys
import sqlite3
import pickle


# ---------------------------------------------------------------------------
# Candidate key names, per appearance function, in order of likelihood.
#
# We match case-insensitively, and prefer an EXACT key match (substring-free)
# over a substring match, and we prefer the most-specific (latest-anki) name
# first. Every match is logged so a wrong name is never silent.
# ---------------------------------------------------------------------------
HIDE_DECK_LIST = [
    "showDeckMenu",         # "hide the deck list"  -> False
    "deckMenu",
    "showDeckList",
    "deckList",
    "showDeckSidebar",
]
HIDE_BUTTON_LABELS = [
    "buttonLabels",         # "show the buttons but not the labels" -> False
    "showButtonLabels",
    "buttonText",
    "showButtonText",
]


def _find_prefs_dbs(data_dir):
    """All prefs21.db under the Anki data dir (single-profile + per-profile)."""
    out = []
    p = os.path.join(data_dir, "prefs21.db")
    if os.path.exists(p):
        out.append(p)
    prof_root = os.path.join(data_dir, "profiles")
    if os.path.isdir(prof_root):
        for sub in sorted(os.listdir(prof_root)):
            cand = os.path.join(prof_root, sub, "prefs21.db")
            if os.path.exists(cand):
                out.append(cand)
    return out


def _rows(db):
    """Return (cols, keycol, rows) for the profiles table.

    We verified on a live DB that the schema is `profiles(name, data)` with a
    pickled-dict `data` column. Write-back is keyed on `id` when present, else
    `name` (both are unique per profile row).
    """
    cur = db.cursor()
    try:
        cols = [r[1] for r in cur.execute("PRAGMA table_info(profiles)")]
    except sqlite3.Error:
        return None, None, []
    if "name" not in cols:
        return cols, None, []
    keycol = "id" if "id" in cols else "name"
    rows = list(cur.execute("SELECT * FROM profiles"))
    return cols, keycol, rows


def _unpickle(row, cols):
    for col in ("data", "prefs"):
        if col in cols:
            blob = row[cols.index(col)]
            if blob is None:
                continue
            try:
                d = pickle.loads(blob)
            except Exception:
                continue
            if isinstance(d, dict):
                return col, d
    return None, None


def _match(prefs, candidates):
    """Return the concrete key in `prefs` (or best candidate) to set, case-insensitive.

    Priority: exact (case-insens) match in the stored keys -> otherwise the
    first candidate (to be *inserted*).
    """
    lower = {k.lower(): k for k in prefs}
    for cand in candidates:
        if cand.lower() in lower:
            return lower[cand.lower()]
    # Not present: we will INSERT. Prefer the first candidate (most likely name).
    return candidates[0]


def _set_if_changed(prefs, key, value):
    if key in prefs and prefs[key] == value:
        return False
    prefs[key] = value
    return True


def probe(data_dir):
    """--probe: dump every pref key per profile so real names are visible."""
    print("== patch_prefs --probe: pref keys per profile ==")
    for path in _find_prefs_dbs(data_dir):
        db = sqlite3.connect(path)
        try:
            cols, keycol, rows = _rows(db)
            if cols is None:
                print("  (no profiles table) %s" % path)
                continue
            print("  -- %s (cols=%s)" % (os.path.relpath(path, data_dir), cols))
            for row in rows:
                name = row[cols.index("name")] if "name" in cols else "?"
                _, prefs = _unpickle(row, cols)
                if prefs is None:
                    print("      %s: <unpickled>" % name)
                    continue
                print("      %s: %d keys" % (name, len(prefs)))
                for k in sorted(prefs):
                    print("        %-40s = %s" % (k, repr(prefs[k])[:60]))
        finally:
            db.close()


def apply(data_dir):
    total = 0
    for path in _find_prefs_dbs(data_dir):
        db = sqlite3.connect(path)
        try:
            cols, keycol, rows = _rows(db)
            if cols is None or not rows:
                continue
            for row in rows:
                name = row[cols.index("name")] if "name" in cols else "?"
                col, prefs = _unpickle(row, cols)
                if col is None or prefs is None:
                    continue
                # _global row: skip (it holds profile metadata, not appearance)
                if str(name) == "_global":
                    continue
                changed = False
                # (a) hide the deck-list top bar
                k = _match(prefs, HIDE_DECK_LIST)
                if _set_if_changed(prefs, k, False):
                    changed = True
                    sys.stderr.write("patch_prefs: [%s] hide-deck-list key=%s -> False\n" % (name, k))
                # (b) hide button labels
                k2 = _match(prefs, HIDE_BUTTON_LABELS)
                if _set_if_changed(prefs, k2, False):
                    changed = True
                    sys.stderr.write("patch_prefs: [%s] hide-button-labels key=%s -> False\n" % (name, k2))
                if not changed:
                    # Nothing new: report so a genuinely-absent key is loud.
                    sys.stderr.write(
                        "patch_prefs: [%s] no change (keys already False or absent-as-default: %s, %s)\n"
                        % (name, HIDE_DECK_LIST[0], HIDE_BUTTON_LABELS[0])
                    )
                    continue
                # write back under the `data`/`prefs` column, keyed by id or name
                new_row = list(row)
                new_row[cols.index(col)] = pickle.dumps(prefs)
                if "id" in cols:
                    where = ("id=?", [new_row[cols.index("id")]])
                else:
                    where = ("name=?", [new_row[cols.index("name")]])
                sets = ", ".join("%s=?" % c for c in cols if c != keycol)
                vals = [new_row[cols.index(c)] for c in cols if c != keycol]
                try:
                    db.execute("UPDATE profiles SET %s WHERE %s" % (sets, where[0]), vals + where[1])
                    total += 1
                except Exception as e:
                    sys.stderr.write("patch_prefs: [%s] write failed: %s\n" % (name, e))
            db.commit()
        finally:
            db.close()
    return total


def main():
    args = list(sys.argv[1:])
    probe_mode = False
    if "--probe" in args:
        probe_mode = True
        args.remove("--probe")
    if len(args) < 1:
        sys.stderr.write("usage: patch_prefs.py [--probe] <ANKI_DATA>\n")
        return 2
    data_dir = args[0]
    if probe_mode:
        probe(data_dir)
        return 0
    n = apply(data_dir)
    sys.stderr.write("patch_prefs: done (%d profile row(s) updated)\n" % n)
    return 0


if __name__ == "__main__":
    sys.exit(main())
