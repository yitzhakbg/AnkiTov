#!/usr/bin/env python3
"""AnkiTov seed deck generator - genanki build per P0 content plan v0.2, section 5.

Reads versioned CSV sources from content/seed/sources/ (one CSV per slug+locale,
named <slug>-<locale>.csv), builds the shared AnkiTov note types from plan
section 3, writes content/seed/packages/<slug>-<locale>.apkg, and updates
content/seed/manifest.json (schema v2: entries carry rules_version, plan section 4 + spec §8c).

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
import yaml

SEED_DIR = Path(__file__).resolve().parent          # content/seed/
REPO_ROOT = SEED_DIR.parent.parent                  # monorepo root
SOURCES_DIR = SEED_DIR / "sources"
PACKAGES_DIR = SEED_DIR / "packages"
MANIFEST_PATH = SEED_DIR / "manifest.json"
RULES_PATH = REPO_ROOT / "specs" / "qa" / "bar1-rules.yaml"


def _rules_version() -> int:
    """BAR 1 rules_version stamped into every manifest entry (spec §8c control 5).

    The manifest records WHICH rules version a pack was last audited under;
    scripts/qa/grandfather_check.py re-runs the gate over packs stamped with an
    older version when the rules file bumps.
    """
    doc = yaml.safe_load(RULES_PATH.read_text(encoding="utf-8"))
    version = doc.get("rules_version")
    if not isinstance(version, int):
        raise RuntimeError(f"{RULES_PATH}: missing integer rules_version")
    return version

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


class SourceError(Exception):
    pass


# Per-deck configs live in content/seed/decks/<slug>.yaml (B-0040 Phase 1):
# the former hardcoded DECKS literal was externalized verbatim (deep-equality
# verified at conversion time). The YAML files are the single source of truth
# for subject tags, titles, deck ids, descriptions, and census targets.
DECKS_DIR = SEED_DIR / "decks"


def _load_decks() -> dict:
    decks = {}
    for path in sorted(DECKS_DIR.glob("*.yaml")):
        data = yaml.safe_load(path.read_text(encoding="utf-8"))
        slug = path.stem
        if data.get("slug") != slug:
            raise SourceError(f"{path.name}: slug {data.get('slug')!r} != filename {slug!r}")
        cfg = {
            "subject_tag": data["subject_tag"],
            "title": data["title"],
            "deck_id": {loc: int(v) for loc, v in data["deck_id"].items()},
            "description": data["description"],
            "expected_census": data["expected_census"],
        }
        if data.get("expected_census_by_locale"):
            cfg["expected_census_by_locale"] = data["expected_census_by_locale"]
        if data.get("directions_by_locale"):
            cfg["directions_by_locale"] = data["directions_by_locale"]
        if "directions" in data:
            cfg["directions"] = data["directions"]
        decks[slug] = cfg
    if not decks:
        raise SourceError(f"no deck configs under {DECKS_DIR}")
    return decks


DECKS = _load_decks()

# Fixed IDs so rebuilds are stable (plan section 5: deterministic, re-runnable).
# Per-locale model IDs: each .apkg ships its own self-contained note types, so
# two locales importing into one Anki collection cannot collide on note type.
MODEL_IDS = {
    "en": {"basic": 1607392318001, "cloze": 1607392318002, "type_in": 1607392318003},
    "he": {"basic": 1607392318101, "cloze": 1607392318102, "type_in": 1607392318103},
    "fr": {"basic": 1607392318201, "cloze": 1607392318202, "type_in": 1607392318203},
    "es": {"basic": 1607392318301, "cloze": 1607392318302, "type_in": 1607392318303},
    # Wave-L1 locales (2026-11-17): de/it/pt/ru/ko model-ID blocks
    "de": {"basic": 1607392318401, "cloze": 1607392318402, "type_in": 1607392318403},
    "it": {"basic": 1607392318501, "cloze": 1607392318502, "type_in": 1607392318503},
    "pt": {"basic": 1607392318601, "cloze": 1607392318602, "type_in": 1607392318603},
    "ru": {"basic": 1607392318701, "cloze": 1607392318702, "type_in": 1607392318703},
    "ko": {"basic": 1607392318801, "cloze": 1607392318802, "type_in": 1607392318803},
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
# Phase A grade band (spec 2026-11-08 §9 Q3, owner correction 2026-11-11): 7-12 default.
# K-2 kept as the recorded exception for public-domain sight-word lists (Dolch/Fry).
ALLOWED_GRADES = {str(g) for g in range(7, 13)} | {"K", "1", "2"}
CLOZE_TAG_RE = re.compile(r"\{\{c(\d+)::")



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
    # Grade band per spec 2026-11-08 §9 Q3 (owner correction 2026-11-11): the default is
    # the junior-high/high-school / bagrut-matriculation band, grades 7-12. K-2 is allowed
    # only as the recorded exception for public-domain sight-word lists (Dolch/Fry).
    if not grades or any(g not in ALLOWED_GRADES for g in grades):
        raise SourceError(f"{concept}: grades must be in {sorted(ALLOWED_GRADES)}, got {grades!r}")

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
            "schema_version": 2,
            "author_name": "AnkiTov Pilot Seed",
            "license": "TBC-OWNER-DECISION",
            "source_url": None,
            "decks": [],
        }
    # Manifest v2 (spec §8c control 5): every entry carries the rules_version
    # it was last audited under; schema_version rides along.
    doc["schema_version"] = 2
    doc["generated_at"] = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    decks = [d for d in doc["decks"]
             if not (d["locale"] == entry["locale"] and d["slug"] == entry["slug"])]
    decks.append(entry)
    decks.sort(key=lambda d: (d["locale"], d["slug"]))   # plan section 4: sorted by (locale, slug)
    doc["decks"] = decks
    MANIFEST_PATH.write_text(json.dumps(doc, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def build_deck(csv_path: Path, slug: str, locale: str, cfg: dict) -> tuple:
    """Build one .apkg from a source CSV; return (out, census, verify, entry, samples).

    Extracted from main() so tests can rebuild a deck deterministically and
    assert the GUID re-import contract (spec section 4.3) without touching
    the manifest.
    """
    models = build_models(locale)

    directions = cfg.get("directions_by_locale", {}).get(locale, cfg.get("directions", False))
    rows = read_rows(csv_path, directions)
    seen: set = set()
    deck = genanki.Deck(
        cfg["deck_id"][locale], cfg["title"][locale], description=cfg["description"][locale]
    )
    census: Counter = Counter()
    samples = {}
    for row in rows:
        nt = row["note_type"]
        validate_row(row, seen, locale, directions)
        direction = row.get("direction", "") if directions else ""
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
    expected = cfg.get("expected_census_by_locale", {}).get(
        locale, cfg["expected_census"])
    if actual != expected:
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
        "rules_version": _rules_version(),
    }
    return out, actual, v, entry, samples


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
        out, actual, v, entry, samples = build_deck(csv_path, slug, locale, cfg)
        update_manifest(entry)

        print(f"== {slug}/{locale} ==")
        print(f"  notes={v['notes']} cards={v['cards']} census={actual} (sum={sum(actual.values())})")
        print(f"  file: {out}")
        print(f"  sha256: {entry['sha256']}")
        for nt in ("basic", "cloze", "type_in"):
            s = samples.get(nt)
            if not s:
                continue
            tail = s["back"] or s["extra"]
            print(f"  sample {nt} [{s['concept_id']}]: front={s['front']!r} back={tail!r}")

    deck_count = len(json.loads(MANIFEST_PATH.read_text(encoding="utf-8"))["decks"])
    print(f"manifest: {MANIFEST_PATH} ({deck_count} deck entries, schema v2, rules v{_rules_version()})")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (SourceError, RuntimeError) as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        sys.exit(1)
