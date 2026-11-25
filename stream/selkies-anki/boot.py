# AnkiTov kiosk launcher.
# /opt/anki/app/anki is a tiny launcher shim (regular package) that shadows the
# real packed 'anki' namespace package in app_packages, hiding anki.lang. Drop
# app/ from sys.path so 'anki' resolves to app_packages, then skip the first-run
# modal language picker and run Anki.
#
# Language: AnkiTov must launch in the *currently chosen* language, so this
# launcher must NOT pin a default. It honours, in order: an explicit platform
# override (ANKITOV_ANKI_LANG), the language the profile already chose, the
# container locale, and only then English.
import os
import pickle
import sys

for p in ("/opt/anki/app", "/opt/anki/app_packages"):
    while p in sys.path:
        sys.path.remove(p)
sys.path.insert(0, "/opt/anki/app_packages")

import aqt
import aqt.profiles


def _stored_lang(pm):
    """The language the profile last chose, or None on a fresh install."""
    try:
        row = pm.db.execute(
            "select val from profiles where name = '_global'"
        ).fetchone()
        if not row:
            return None
        g = row[0]
        if isinstance(g, (bytes, bytearray)):
            g = pickle.loads(g)
        lang = (g or {}).get("defaultLang")
        if isinstance(lang, bytes):
            lang = lang.decode("utf-8", "replace")
        return lang or None
    except Exception:
        return None


def _system_lang():
    for var in ("LC_ALL", "LC_MESSAGES", "LANG"):
        v = os.environ.get(var, "")
        code = v.split(".")[0].split("_")[0].strip()
        if code and code not in ("C", "POSIX"):
            return code
    return None


def _chosen_lang(pm):
    # Precedence: explicit platform override -> the profile's chosen language
    # -> the container locale -> English.
    return (
        os.environ.get("ANKITOV_ANKI_LANG", "").strip()
        or _stored_lang(pm)
        or _system_lang()
        or "en"
    )


def _set_default_lang(self, idx):
    # Called by Anki when it needs a language for a not-yet-configured profile.
    # Honour the chosen language instead of forcing English.
    try:
        self.setLang(_chosen_lang(self))
    except Exception:
        try:
            self.setLang("en")
        except Exception:
            pass


aqt.profiles.ProfileManager.setDefaultLang = _set_default_lang

aqt.run()
