# AnkiTov kiosk launcher.
# /opt/anki/app/anki is a tiny launcher shim (regular package) that shadows the
# real packed 'anki' namespace package in app_packages, hiding anki.lang. Drop
# app/ from sys.path so 'anki' resolves to app_packages, then skip the first-run
# modal language picker and run Anki.
import sys

for p in ("/opt/anki/app", "/opt/anki/app_packages"):
    while p in sys.path:
        sys.path.remove(p)
sys.path.insert(0, "/opt/anki/app_packages")

import aqt
import aqt.profiles

def _set_default_lang(self, idx):
    try:
        self.setLang("en")
    except Exception:
        pass

aqt.profiles.ProfileManager.setDefaultLang = _set_default_lang

aqt.run()
