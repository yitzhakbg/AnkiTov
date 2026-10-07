#!/usr/bin/env python3
"""AnkiTov seed deck generator - genanki build per P0 content plan v0.2, section 5.

Reads versioned CSV sources from content/seed/sources/ (one CSV per slug+locale,
named <slug>-<locale>.csv), builds the shared AnkiTov note types from plan
section 3, writes content/seed/packages/<slug>-<locale>.apkg, and updates
content/seed/manifest.json (schema v1, plan section 4).

Note types (plan section 3):
    AnkiTovBasic  { Front, Back }        qfmt: {{Front}}
    AnkiTovCloze  { Text, Back Extra }   qfmt: {{cloze:Text}}  (exactly one c1 deletion)
    AnkiTovTypeIn { Front, Back }        qfmt: {{Front}}<br>{{type:Back}}

This slice (plan section 7, step 1) authors ONLY the pattern-setter deck
en/math-definitions. The script validates structure/limits, never content
quality - the gate and operator spot-check own that (plan section 5).

Usage: python3 generate_decks.py
"""
from __future__ import annotations

import csv
import hashlib
import json
import re
import sqlite3
import sys
import tempfile
import zipfile
from collections import Counter
from datetime import datetime, timezone
from pathlib import Path

import genanki

SEED_DIR = Path(__file__).resolve().parent          # content/seed/
SOURCES_DIR = SEED_DIR / "sources"
PACKAGES_DIR = SEED_DIR / "packages"
MANIFEST_PATH = SEED_DIR / "manifest.json"

# ---------------------------------------------------------------------------
# Deck registry (plan sections 1.1, 3.2, 4). Titles are locale-keyed; the
# note-type mix targets come from plan section 3.2 and are enforced exactly.
# Titles must equal the Anki deck name (plan section 1.1).
# ---------------------------------------------------------------------------
# Deck descriptions follow the positioning doctrine (2026-09-03): lead with
# pace-disengagement / gap-closure, keep the only permitted claim ("SRS works"),
# never frame practice as in-class / teacher-led activity.
def _desc(en: str, **by_locale: str) -> dict:
    out = {"en": en}
    out.update(by_locale)
    for known in ("en", "he", "fr"):
        out.setdefault(known, en)  # an unlisted locale reuses the en string
    return out


DECKS = {
    "math-definitions": {
        "subject_tag": "subject::math",
        "title": {
            "en": "AnkiTov Pilot — Math Definitions",
            "he": "AnkiTov Pilot — הגדרות מתמטיקה",
            "fr": "AnkiTov Pilot — Définitions de maths",
        },
        "deck_id": {"en": 1699412247001, "he": 1699412247002, "fr": 1699412247012},
        "description": _desc(
            "Grades 7-9 math definitions that run at their own pace, beside "
            "teaching, not inside it. Practice previously taught material to "
            "close gaps. SRS works. Self-authored Phase 0 seed; text only.",
            he="גדרות מתמטיקה לכיתות ז-ט שרצות בקצב עצמאי, לצד ההוראה ולא בתוכה. "
               "תרגול של חומר שהונהג קודם לכן כדי לסגור פערים. SRS works. "
               "פיוט פאזה 0 שנכתב על ידנו; טקסט בלבד.",
            fr="Définitions mathématiques pour les classes 7-9 qui suivent leur propre "
               "rythme, à côté de l'enseignement, pas dedans. Revoyez les concepts "
               "appris auparavant pour combler les lacunes. SRS works. Graine Phase 0 "
               "auto-écrite ; texte uniquement.",
        ),
        "expected_census": {"basic": 47, "cloze": 47, "type_in": 11},
    },
    "english-vocabulary": {
        "subject_tag": "subject::english",
        "directions": True,  # vocab deck: direction::en-he / direction::he-en tags (R4)
        "title": {
            "en": "AnkiTov Pilot — English Vocabulary",
            "he": "AnkiTov Pilot — אוצר מילים באנגלית",
            "fr": "AnkiTov Pilot — Vocabulaire anglais",
        },
        "deck_id": {"en": 1699412247003, "he": 1699412247004, "fr": 1699412247005},
        "description": _desc(
            "Grades 7-9 English vocabulary that runs at its own pace, beside "
            "teaching, not inside it. Revisit previously taught words to close "
            "gaps. SRS works. Self-authored Phase 0 seed; text only.",
            he="אוצר מילים באנגלית לכיתות ז-ט שרץ בקצב עצמאי, לצד ההוראה ולא בתוכה. "
               "חזרה על מילים שהונהגו קודם לכן כדי לסגור פערים. SRS works. "
               "פיוט פאזה 0 שנכתב על ידנו; טקסט בלבד.",
            fr="Vocabulaire anglais pour les classes 7-9 qui suit son propre rythme, "
               "à côté de l'enseignement, pas dedans. Revoyez les mots appris "
               "auparavant pour combler les lacunes. SRS works. Graine Phase 0 "
               "auto-écrite ; texte uniquement.",
        ),
        "expected_census": {"basic": 58, "cloze": 21, "type_in": 26},
    },
    "science-terms": {
        "subject_tag": "subject::science",
        "title": {
            "en": "AnkiTov Pilot — Science Terms",
            "he": "AnkiTov Pilot — מונחים במדעים",
            "fr": "AnkiTov Pilot — Termes scientifiques",
        },
        "deck_id": {"en": 1699412247006, "he": 1699412247007, "fr": 1699412247008},
        "description": _desc(
            "Grades 7-9 science terms that run at their own pace, beside "
            "teaching, not inside it. Revisit previously taught concepts to "
            "close gaps. SRS works. Self-authored Phase 0 seed; text only.",
            he="מונחים מדעיים לכיתות ז-ט שרצים בקצב עצמאי, לצד ההוראה ולא בתוכה. "
               "חזרה על מושגים שהונהגו קודם לכן כדי לסגור פערים. SRS works. "
               "פיוט פאזה 0 שנכתב על ידנו; טקסט בלבד.",
            fr="Termes scientifiques pour les classes 7-9 qui suivent leur propre "
               "rythme, à côté de l'enseignement, pas dedans. Revoyez les "
               "concepts appris auparavant pour combler les lacunes. SRS works. "
               "Graine Phase 0 auto-écrite ; texte uniquement.",
        ),
        "expected_census": {"basic": 63, "cloze": 32, "type_in": 10},
    },
    "history-dates-figures": {
        "subject_tag": "subject::history",
        "title": {
            "en": "AnkiTov Pilot — History: Dates & Figures",
            "he": "AnkiTov Pilot — היסטוריה: תאריכים ואישים",
            "fr": "AnkiTov Pilot — Histoire : dates et figures",
        },
        "deck_id": {"en": 1699412247009, "he": 1699412247010, "fr": 1699412247011},
        "description": _desc(
            "Grades 7-9 history dates and figures that run at their own pace, "
            "beside teaching, not inside it. Revisit previously taught "
            "milestones to close gaps. SRS works. Self-authored Phase 0 seed; "
            "text only.",
            he="תאריכים ואישים בהיסטוריה לכיתות ז-ט שרצים בקצב עצמאי, לצד ההוראה "
               "ולא בתוכה. חזרה על אבני דרך שהונהגו קודם לכן כדי לסגור פערים. "
               "SRS works. פיוט פאזה 0 שנכתב על ידנו; טקסט בלבד.",
            fr="Dates et figures d'histoire pour les classes 7-9 qui suivent leur "
               "propre rythme, à côté de l'enseignement, pas dedans. Revoyez les "
               "étapes clés apprises auparavant pour combler les lacunes. SRS works. "
               "Graine Phase 0 auto-écrite ; texte uniquement.",
        ),
        "expected_census": {"basic": 79, "cloze": 26, "type_in": 0},
    },
}

# Fixed IDs so rebuilds are stable (plan section 5: deterministic, re-runnable).
# Per-locale model IDs: each .apkg ships its own self-contained note types, so
# two locales importing into one Anki collection cannot collide on note type.
MODEL_IDS = {
    "en": {"basic": 1607392318001, "cloze": 1607392318002, "type_in": 1607392318003},
    "he": {"basic": 1607392318101, "cloze": 1607392318102, "type_in": 1607392318103},
    "fr": {"basic": 1607392318201, "cloze": 1607392318202, "type_in": 1607392318203},
}
NOTE_TYPE_NAMES = {"basic": "AnkiTovBasic", "cloze": "AnkiTovCloze", "type_in": "AnkiTovTypeIn"}

# Shared CSS. The .ankitov-he rules are the plan section 3.6 RTL scope: inert
# for en, required for he, so they ship in every package (R3 consistency).
SHARED_CSS = """.card {
  font-family: Arial, sans-serif;
  font-size: 20px;
  text-align: center;
  color: #1a1a1a;
  background-color: #fcfcfc;
}
hr#answer { border: 0; border-top: 1px solid #c8c8c8; margin: 16px 0; }
.cloze { font-weight: bold; color: #0b6bcb; }
/* AnkiTov RTL scope (plan section 3.6) */
.card.ankitov-he { direction: rtl; text-align: right; }
.ankitov-he { direction: rtl; text-align: right; }
.ankitov-he .ltr { direction: ltr; unicode-bidi: isolate; }
"""


def _wrap(fmt: str, locale: str) -> str:
    """Per-locale template wrapper (plan section 3.6).

    he: ``dir="rtl"`` + ``class="ankitov-he"`` so the CSS scope applies even
    before the .card class is attached; formula spans stay LTR-isolated.
    en (and other LTR locales): no wrapper — byte-identical to the existing
    committed model, so the en rebuild stays bit-for-bit reproducible.
    """
    if locale != "he":
        return fmt
    return '<div dir="rtl" class="ankitov-he">' + fmt + "</div>"


def build_models(locale: str) -> dict:
    """Build the locale's three note models with that locale's fixed IDs.

    The he package carries dir="rtl" template wrappers + the .ankitov-he
    CSS scope (plan section 3.6); en keeps the original LTR models verbatim.
    """
    ids = MODEL_IDS[locale]
    wrap = lambda fmt: _wrap(fmt, locale)
    basic = genanki.Model(
        ids["basic"],
        NOTE_TYPE_NAMES["basic"],
        fields=[{"name": "Front"}, {"name": "Back"}],
        templates=[{
            "name": "AnkiTov Basic",
            "qfmt": wrap("{{Front}}"),
            # Plan section 3.4: the rendered back repeats the full front question first.
            "afmt": wrap("{{Front}}<hr id=answer>{{Back}}"),
        }],
        css=SHARED_CSS,
    )
    cloze = genanki.Model(
        ids["cloze"],
        NOTE_TYPE_NAMES["cloze"],
        fields=[{"name": "Text"}, {"name": "Back Extra"}],
        templates=[{
            "name": "AnkiTov Cloze",
            "qfmt": wrap("{{cloze:Text}}"),
            "afmt": wrap("{{cloze:Text}}<hr id=answer>{{Back Extra}}"),
        }],
        css=SHARED_CSS,
        model_type=genanki.Model.CLOZE,
    )
    typein = genanki.Model(
        ids["type_in"],
        NOTE_TYPE_NAMES["type_in"],
        fields=[{"name": "Front"}, {"name": "Back"}],
        templates=[{
            # Plan section 3.8: typed-answer input + inline diff grading.
            "name": "AnkiTov TypeIn",
            "qfmt": wrap("{{Front}}<br>{{type:Back}}"),
            "afmt": wrap("{{Front}}<hr id=answer>{{type:Back}}"),
        }],
        css=SHARED_CSS,
    )
    return {"basic": basic, "cloze": cloze, "type_in": typein}


# ---------------------------------------------------------------------------
# Validation (plan sections 3.3-3.5, 3.8; R4, R5, R7). Fail loudly, never warn.
# ---------------------------------------------------------------------------
CONCEPT_RE = re.compile(r"^[a-z][a-z0-9-]*$")
CLOZE_TAG_RE = re.compile(r"\{\{c(\d+)::")


class SourceError(Exception):
    pass


def validate_row(row: dict, seen: set, locale: str, directions: bool = False) -> None:
    nt = row["note_type"]
    concept = row["concept_id"]
    unit = row["unit"]
    grades = row["grades"].split()

    if nt not in ("basic", "cloze", "type_in"):
        raise SourceError(f"{concept}: bad note_type {nt!r}")
    if not CONCEPT_RE.match(concept):
        raise SourceError(f"bad concept_id {concept!r}")
    if concept in seen:
        raise SourceError(f"duplicate concept_id {concept!r} (R5: one concept, one note)")
    seen.add(concept)
    if not re.match(r"^\d{2}$", unit):
        raise SourceError(f"{concept}: unit must be two digits, got {unit!r}")
    if not grades or any(g not in ("7", "8", "9") for g in grades):
        raise SourceError(f"{concept}: grades must be 7/8/9, got {grades!r}")

    for key in ("front", "back", "extra"):
        val = row[key]
        if "\n" in val or "\r" in val:
            raise SourceError(f"{concept}: newline in {key} (one-line rule)")
        if locale == "en" and not val.isascii():
            raise SourceError(f"{concept}: non-ASCII in {key} (en-US deck)")
        if "{{" in val and not (nt == "cloze" and key == "front"):
            raise SourceError(f"{concept}: template markup outside cloze Text field")

    front, back, extra = row["front"], row["back"], row["extra"]
    if not front:
        raise SourceError(f"{concept}: empty front")

    if nt == "basic":
        if len(front) > 240:
            raise SourceError(f"{concept}: basic front {len(front)} > 240 chars")
        if len(back) > 400:
            raise SourceError(f"{concept}: basic back {len(back)} > 400 chars")
        if not back:
            raise SourceError(f"{concept}: empty basic back")
    elif nt == "cloze":
        tags = CLOZE_TAG_RE.findall(front)
        if tags != ["1"]:
            # Exactly one deletion, and it must be c1 (plan section 3.5).
            raise SourceError(f"{concept}: expected exactly one {{c1::}} deletion, got {tags}")
        rendered = re.sub(r"\{\{c1::(.*?)\}\}", r"\1", front)
        if len(rendered) > 120:
            raise SourceError(f"{concept}: rendered cloze text {len(rendered)} > 120 chars")
        if len(front) > 240:
            raise SourceError(f"{concept}: cloze Text {len(front)} > 240 chars")
        if len(extra) > 400:
            raise SourceError(f"{concept}: cloze extra {len(extra)} > 400 chars")
    else:  # type_in - plan section 3.8 limits
        if len(front) > 120:
            raise SourceError(f"{concept}: type-in front {len(front)} > 120 chars")
        if not back or len(back) > 60:
            raise SourceError(f"{concept}: type-in back must be 1-60 chars, got {len(back)}")
        if extra:
            raise SourceError(f"{concept}: type-in cards carry no extra line")


def build_tags(concept: str, unit: str, grades: list, subject_tag: str,
               direction: str = "") -> list:
    tags = [subject_tag] + [f"grade::{g}" for g in grades] + [f"unit::{unit}"]
    if direction:
        if direction not in ("direction::en-he", "direction::he-en"):
            raise SourceError(f"{concept}: bad direction tag {direction!r} (R4)")
        tags.append(direction)
    for t in tags:
        if " " in t or not t.isascii():
            raise SourceError(f"{concept}: taxonomy tag violation {t!r} (R4)")
    return tags


def read_rows(csv_path: Path, directions: bool = False) -> list:
    with csv_path.open(newline="", encoding="utf-8") as fh:
        reader = csv.DictReader(fh)
        required = {"note_type", "concept_id", "unit", "grades", "front", "back", "extra"}
        if directions:
            required.add("direction")
        missing = required - set(reader.fieldnames or [])
        if missing:
            raise SourceError(f"{csv_path.name}: missing columns {sorted(missing)}")
        return [dict(r) for r in reader]


def verify_apkg(path: Path, expected_cards: int, expected_models: list, locale: str = "en") -> dict:
    """Independent check: reopen the .apkg and count what Anki would see."""
    with zipfile.ZipFile(path) as zf:
        db_bytes = zf.read("collection.anki2")
    tmp = Path(tempfile.mkdtemp()) / "collection.anki2"
    tmp.write_bytes(db_bytes)
    con = sqlite3.connect(str(tmp))
    notes = con.execute("SELECT count(*) FROM notes").fetchone()[0]
    cards = con.execute("SELECT count(*) FROM cards").fetchone()[0]
    (models_json,) = con.execute("SELECT models FROM col").fetchone()
    models_raw = json.loads(models_json)
    # genanki writes models as a JSON object keyed by model id (Anki schema);
    # accept a plain list too for safety.
    models_iter = models_raw.values() if isinstance(models_raw, dict) else models_raw
    model_names = sorted(m["name"] for m in models_iter)
    typein_qfmt = None
    for m in models_iter:
        if m["name"] == NOTE_TYPE_NAMES["type_in"]:
            typein_qfmt = m["tmpls"][0]["qfmt"]
    con.close()
    tmp.unlink()
    if model_names != sorted(expected_models):
        raise RuntimeError(f"{path.name}: models in apkg are {model_names}, expected {expected_models}")
    if NOTE_TYPE_NAMES["type_in"] in expected_models:
        expected_typein = _wrap("{{Front}}<br>{{type:Back}}", locale)
        if typein_qfmt != expected_typein:
            raise RuntimeError(f"{path.name}: AnkiTovTypeIn qfmt is {typein_qfmt!r}, expected {expected_typein!r} (plan sections 3.8/3.6)")
    if locale == "he":
        for m in models_iter:
            if '<div dir="rtl" class="ankitov-he">' not in m["tmpls"][0]["qfmt"]:
                raise RuntimeError(f"{path.name}: {m['name']} question template missing dir=rtl wrapper (plan section 3.6)")
        if not any(".ankitov-he" in m.get("css", "") for m in models_iter):
            raise RuntimeError(f"{path.name}: no .ankitov-he CSS scope in models (plan section 3.6)")
    if cards != expected_cards or notes != expected_cards:
        raise RuntimeError(f"{path.name}: apkg has notes={notes} cards={cards}, expected {expected_cards}")
    return {"notes": notes, "cards": cards}


def update_manifest(entry: dict) -> None:
    if MANIFEST_PATH.exists():
        doc = json.loads(MANIFEST_PATH.read_text(encoding="utf-8"))
    else:
        doc = {
            "schema_version": 1,
            "author_name": "AnkiTov Pilot Seed",
            "license": "TBC-OWNER-DECISION",
            "source_url": None,
            "decks": [],
        }
    doc["generated_at"] = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    decks = [d for d in doc["decks"]
             if not (d["locale"] == entry["locale"] and d["slug"] == entry["slug"])]
    decks.append(entry)
    decks.sort(key=lambda d: (d["locale"], d["slug"]))   # plan section 4: sorted by (locale, slug)
    doc["decks"] = decks
    MANIFEST_PATH.write_text(json.dumps(doc, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def main() -> int:
    csvs = sorted(SOURCES_DIR.glob("*.csv"))
    if not csvs:
        print("no source CSVs found under sources/", file=sys.stderr)
        return 1

    for csv_path in csvs:
        m = re.match(r"^([a-z0-9-]+)-([a-z]{2})\.csv$", csv_path.name)
        if not m:
            raise SourceError(f"unparseable source filename: {csv_path.name}")
        slug, locale = m.group(1), m.group(2)
        cfg = DECKS.get(slug)
        if cfg is None or locale not in cfg["title"]:
            raise SourceError(f"no deck config for {slug}/{locale}; add it to DECKS first")
        models = build_models(locale)

        rows = read_rows(csv_path, cfg.get("directions", False))
        seen: set = set()
        deck = genanki.Deck(
            cfg["deck_id"][locale], cfg["title"][locale], description=cfg["description"][locale]
        )
        census: Counter = Counter()
        samples = {}
        for row in rows:
            nt = row["note_type"]
            validate_row(row, seen, locale, cfg.get("directions", False))
            direction = row.get("direction", "") if cfg.get("directions", False) else ""
            if direction:
                direction = f"direction::{direction}"
            tags = build_tags(row["concept_id"], row["unit"], row["grades"].split(),
                              cfg["subject_tag"], direction)
            fields = [row["front"], row["extra"]] if nt == "cloze" else [row["front"], row["back"]]
            note = genanki.Note(
                model=models[nt],
                fields=fields,
                tags=tags,
                # Plan section 5: explicit GUID from concept_id + locale + slug
                # so rebuilds never duplicate notes on re-import.
                guid=genanki.guid_for(f"{row['concept_id']}|{locale}|{slug}"),
            )
            deck.add_note(note)
            census[nt] += 1
            samples.setdefault(nt, row)

        actual = {k: census.get(k, 0) for k in ("basic", "cloze", "type_in")}
        if actual != cfg["expected_census"]:
            raise SourceError(f"{slug}/{locale}: census {actual} != plan target {cfg['expected_census']}")
        if sum(actual.values()) != len(rows):
            raise SourceError(f"{slug}/{locale}: census does not sum to row count")

        out = PACKAGES_DIR / f"{slug}-{locale}.apkg"
        out.parent.mkdir(parents=True, exist_ok=True)
        genanki.Package(deck).write_to_file(str(out))

        expected_models = [NOTE_TYPE_NAMES[nt] for nt, count in actual.items() if count > 0]
        v = verify_apkg(out, len(rows), expected_models, locale)
        sha = hashlib.sha256(out.read_bytes()).hexdigest()
        entry = {
            "locale": locale,
            "slug": slug,
            "file": f"packages/{out.name}",
            "title": cfg["title"][locale],
            "card_count": v["cards"],
            "note_type_census": actual,
            "sha256": sha,
            "rtl": locale == "he",
        }
        update_manifest(entry)

        print(f"== {slug}/{locale} ==")
        print(f"  notes={v['notes']} cards={v['cards']} census={actual} (sum={sum(actual.values())})")
        print(f"  file: {out}")
        print(f"  sha256: {sha}")
        for nt in ("basic", "cloze", "type_in"):
            s = samples.get(nt)
            if not s:
                continue
            tail = s["back"] or s["extra"]
            print(f"  sample {nt} [{s['concept_id']}]: front={s['front']!r} back={tail!r}")

    deck_count = len(json.loads(MANIFEST_PATH.read_text(encoding="utf-8"))["decks"])
    print(f"manifest: {MANIFEST_PATH} ({deck_count} deck entries, schema v1)")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (SourceError, RuntimeError) as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        sys.exit(1)
