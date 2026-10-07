import threading, time, traceback
def _log(m):
    try:
        with open("/tmp/sc.log","a") as f: f.write(str(m)+"\n")
    except Exception: pass
_log("sitecustomize loaded")
def _worker():
    import sys
    for _ in range(180):
        time.sleep(1)
        aqt = sys.modules.get("aqt")
        mw = getattr(aqt, "mw", None) if aqt else None
        if mw is not None and getattr(mw, "col", None) is not None:
            _log("mw ready")
            def go():
                try:
                    did=None
                    for d in mw.col.decks.all_names_and_ids():
                        if "English Vocabulary" in d.name:
                            did=d.id; break
                    _log("deck=%s"%did)
                    if did is not None:
                        mw.col.decks.select(did)
                        mw.moveToState("review")
                        _log("moved to review")
                except Exception:
                    _log("go fail "+traceback.format_exc())
            try:
                mw.taskman.run_on_main(go)
                _log("scheduled via taskman")
            except Exception:
                _log("sched fail "+traceback.format_exc())
            return
threading.Thread(target=_worker, daemon=True).start()
