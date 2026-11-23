#!/usr/bin/env python3
"""Set the sample sentence (final example part) of language-pack answers on its own line.

Owner request 2026-11-23: decks on https://ankitov.org/deck-packs/languages/ whose
answer ends with a sample sentence must render that sentence on a NEW line. The
backs were compiled as `definition + " " + example` on one line; Anki renders
fields as HTML and collapses whitespace, so the fix inserts a literal `<br>`
before the example part. GUIDs are concept_id|locale|slug (generate_decks.py
section 5), so field edits update already-imported collections in place.

Rules per pack family (all in the storefront Languages category):
  - sentence packs (ela-*, *-core-vocabulary, it/ko/pt-orthography-rules):
    back = definition + sample sentence -> <br> before the LAST guarded sentence
    boundary. A boundary is rejected when the next char is lowercase (pág. para),
    the preceding token is a single letter (z. B.) or digits (3. Person), or a
    known abbreviation (Mr., vs., usw., ...).
  - de-orthography-rules: example parts are marked ("Weitere Beispiele:",
    "Weiteres Beispiel:", "Weitere Paare:", "Weitere Varianten:"). Split before
    the marker ONLY when the marker part runs to the end of the back (the
    3 mid-back marker cards keep their trailing rule sentence and stay inline).
  - english-bagrut-band1 (he): back = Hebrew definition + English sample
    sentence -> <br> at the Hebrew->Latin script transition.
Cloze extras and type-in backs are passed through the same guards and no-op
(their extras are definitions/questions, not sample sentences).

Idempotent: backs already containing <br> are skipped. Files keep their CRLF
line endings and canonical QUOTE_MINIMAL quoting (byte-identical round-trip
for unchanged lines).
"""
from __future__ import annotations

import csv
import io
import re
import sys
from pathlib import Path

SRC = Path(__file__).resolve().parent.parent / "sources"

SENT_PACKS = [
    "ela-content-words-en", "ela-high-frequency-en",
    "de-core-vocabulary-de", "it-core-vocabulary-it", "it-orthography-rules-it",
    "ko-core-vocabulary-ko", "ko-orthography-rules-ko",
    "pt-core-vocabulary-pt", "pt-orthography-rules-pt", "ru-core-vocabulary-ru",
    "english-vocabulary-en", "english-vocabulary-he", "english-vocabulary-fr",
    "english-vocabulary-es",
]
MARKER_PACKS = ["de-orthography-rules-de"]
BAGRUT_PACKS = ["english-bagrut-band1-he"]

SENT_CAND = re.compile(r"(?<=[.!?…])\s+")
MARK = re.compile(
    r"(?:Weitere (?:Beispiele(?: ohne h)?|Paare|Varianten)|Weiteres Beispiel|Beispiele?):"
)
TAIL_END = re.compile(r"""[.!?…]["'»“”’)\]]?$""")
ABB_FRAGMENT = re.compile(r"^[A-Za-zА-Яа-яЁё]\.$")
HEB = re.compile(r"[\u0590-\u05FF]")
LAT = re.compile(r"[A-Za-z]")
ABBREV = {
    "mr", "mrs", "ms", "dr", "st", "vs", "etc", "ca", "pág", "pags", "nr",
    "usw", "zw", "vgl", "bzw", "resp", "insbes", "approx", "prof", "jr", "sr",
    "fig", "tab", "z.b", "d.h", "u.a", "i.e", "e.g",
}


def _candidates(b: str):
    """Sentence-boundary candidates that pass the abbreviation guards."""
    out = []
    for m in SENT_CAND.finditer(b):
        tok_m = re.search(r"(\S+)$", b[: m.start() - 1])
        tok = tok_m.group(1) if tok_m else ""
        nxt = b[m.end(): m.end() + 1]
        next_tok_m = re.match(r"\S+", b[m.end():])
        next_tok = next_tok_m.group(0) if next_tok_m else ""
        if nxt.islower():
            continue
        if tok.isdigit():
            continue
        if tok.lower().strip(".,;:") in ABBREV:
            continue
        # Single-letter token: only an abbreviation fragment when the NEXT token
        # is also a single letter + period ("z. B.", "u. a."). Single-letter
        # WORDS are legitimate sentence enders (pt "vogal i", it "è", ko 길).
        if len(tok) == 1 and tok.isalpha() and ABB_FRAGMENT.match(next_tok):
            continue
        out.append(m)
    return out


def split_sentence(b: str) -> str | None:
    """<br> before the last guarded sentence boundary, or None."""
    if "<br>" in b:
        return None
    cands = _candidates(b)
    if not cands:
        return None
    m = cands[-1]
    head, tail = b[: m.start()], b[m.end():]
    if len(tail) < 12 or tail[:1].islower():
        return None
    if not TAIL_END.search(tail):
        return None
    if len(head) < 10 or head.rstrip().endswith(":"):
        return None
    return b[: m.start()] + "<br>" + tail


def split_marker(b: str) -> str | None:
    """<br> before a trailing 'Weitere *:' example marker, or None."""
    if "<br>" in b:
        return None
    m = MARK.search(b)
    if not m:
        return None
    cands = _candidates(b)
    if cands and m.start() < cands[-1].start():
        return None  # a sentence boundary follows the marker -> example is mid-back
    if not cands:
        return None
    return b[: m.start()] + "<br>" + b[m.start():]


def split_bagrut(b: str) -> str | None:
    """<br> at the Hebrew definition -> English sample-sentence transition."""
    if "<br>" in b:
        return None
    heb_last = max((m.start() for m in HEB.finditer(b)), default=-1)
    lat_first = next((m.start() for m in LAT.finditer(b) if m.start() > heb_last), None)
    if lat_first is None:
        return None
    cut = b.rfind(". ", 0, lat_first)
    if cut == -1:
        return None
    return b[: cut + 1] + "<br>" + b[cut + 2:]


def transform(pack: str, note_type: str, back: str, extra: str) -> tuple[str, str]:
    if pack in MARKER_PACKS:
        if note_type == "basic":
            new = split_marker(back)
            if new:
                return new, extra
        else:
            new = split_sentence(extra)
            if new:
                return back, new
        return back, extra
    if pack in BAGRUT_PACKS:
        if note_type == "basic":
            new = split_bagrut(back)
            if new:
                return new, extra
        return back, extra
    field = back if note_type != "cloze" else extra
    if field:
        new = split_sentence(field)
        if new:
            return (new, extra) if note_type != "cloze" else (back, new)
    return back, extra


def encode_line(fields: list[str]) -> str:
    buf = io.StringIO()
    csv.writer(buf, lineterminator="").writerow(fields)
    return buf.getvalue()


def main() -> None:
    report: list[str] = []
    counts: dict[str, int] = {}
    for pack in SENT_PACKS + MARKER_PACKS + BAGRUT_PACKS:
        path = SRC / f"{pack}.csv"
        raw = path.read_bytes()
        text = raw.decode("utf-8")
        lines = text.split("\r\n")
        if lines and lines[-1] == "":
            lines = lines[:-1]
        out_lines = []
        changed = 0
        for ln in lines:
            fields = next(csv.reader([ln]))
            rec = dict(zip(next(csv.reader([lines[0]])), fields))
            nb, ne = transform(pack, rec["note_type"], rec["back"], rec["extra"])
            if nb != rec["back"] or ne != rec["extra"]:
                changed += 1
                old_f = rec["back"] if nb != rec["back"] else rec["extra"]
                new_f = nb if nb != rec["back"] else ne
                cut = new_f.find("<br>")
                report.append(
                    f"{pack} [{rec['note_type']}] {rec['concept_id']}: "
                    f"...{old_f[max(0, cut - 28):cut]}⟦BR⟧{new_f[cut + 4:cut + 44]}..."
                )
                fields = [
                    nb if k == "back" else (ne if k == "extra" else v)
                    for k, v in rec.items()
                ]
            out_lines.append(encode_line(fields))
        new_raw = ("\r\n".join(out_lines) + "\r\n").encode("utf-8")
        # unchanged lines must round-trip byte-identically
        assert new_raw.count(b"\r\n") == raw.count(b"\r\n"), pack
        if changed:
            path.write_bytes(new_raw)
        counts[pack] = changed
    Path("/tmp/br-report.txt").write_text("\n".join(report) + "\n", encoding="utf-8")
    total = sum(counts.values())
    for pack, n in counts.items():
        print(f"{pack}: {n} backs updated")
    print(f"TOTAL: {total} backs updated (full report: /tmp/br-report.txt)")


if __name__ == "__main__":
    main()
