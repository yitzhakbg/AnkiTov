#!/usr/bin/env python3
"""Validate i18n completeness — exit code 0 = ready.

Checks:
1. Key completeness: all locales have the same keys as en-US.
2. HTML wiring: all keys are referenced in imp-console.html.
3. Non-Latin translation gate: for non-Latin-alphabet locales, scan
   display values for untranslated English words (exit code 2 if found).

Usage:
    python3 tools/validate_i18n.py
"""

import json
import re
import sys

# ── Configuration ──────────────────────────────────────────────────

ALL_CODES = ["en-US", "ar", "he", "fr", "es", "de", "it", "ru", "zh", "ko", "ja", "pt", "hi"]

# Non-Latin-alphabet locales — these MUST have no English display text
NON_LATIN_CODES = {"ar", "he", "ru", "zh", "ko", "ja", "hi"}

# Tokens that are allowed to appear in non-Latin locale values.
# These are NOT translated because they are brand names, file extensions,
# keyboard keys, common abbreviations, or HTML markup.
ALLOWED_ENGLISH_TOKENS = {
    # Brand / product names
    "AnkiTov", "Anki",
    # File extensions & format names
    "apkg", "xlsx", "csv", "tsv", "CSV", "TSV", "Excel",
    # Units
    "MB", "KB", "min",
    # Keyboard keys
    "Shift", "Enter", "Ctrl", "Alt", "Tab", "Esc", "Escape",
    # Common abbreviations that appear in UI
    "ID", "AI", "URL", "FAQ",
    # HTML tag names (these appear as part of markup)
    "h2", "h3", "p", "ol", "li", "ul", "strong", "em", "div", "span",
    "table", "tr", "th", "td", "thead", "tbody", "br", "hr", "img", "a",
    "amp", "nbsp", "lt", "gt", "quot", "apos",
    # Template variable names (Fluent: { $var } and JSON: {var} / ${var})
    "count", "name", "decks", "classes", "query", "created", "skipped",
    "enrolled", "column", "reason", "page", "days", "capsules", "entity",
    "detail", "id", "old", "new", "target", "profile", "student",
    "size", "target_type", "reason",
    # Example values used in filter placeholders (code-like, not display text)
    "tag", "math",
    # Single-letter or short patterns that are not real English words
    "N", "n", "s", "x", "S", "R", "T", "L", "A", "B", "C", "D", "E",
    "F", "G", "H", "I", "J", "K", "M", "O", "P", "Q", "U", "V", "W",
    "X", "Y", "Z",
}

# ── Helpers ────────────────────────────────────────────────────────


def load_json(path: str) -> dict:
    with open(path) as f:
        return json.load(f)


def load_ftl_keys(path: str) -> dict[str, str]:
    """Parse Fluent .ftl file and return {key: value} dict."""
    result = {}
    with open(path) as f:
        for line in f:
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            if "=" in line:
                parts = line.split("=", 1)
                key = parts[0].strip()
                value = parts[1].strip()
                result[key] = value
    return result


def strip_html(text: str) -> str:
    """Remove HTML tags but keep their content."""
    return re.sub(r"<[^>]+>", "", text)


def strip_template_vars(text: str) -> str:
    """Remove template variable placeholders like {var}, ${var}, { $var }."""
    text = re.sub(r"\$\{[^}]+\}", "", text)
    text = re.sub(r"\{[^}]+?\}", "", text)
    text = re.sub(r"\{\s*\$[^}]+\}", "", text)
    return text


def strip_html_entities(text: str) -> str:
    """Remove HTML entities like &amp; &nbsp; etc."""
    return re.sub(r"&[a-zA-Z]+;", "", text)


def extract_english_words(text: str) -> set[str]:
    """Extract all sequences of 2+ consecutive ASCII letters from text."""
    text = strip_html(text)
    text = strip_template_vars(text)
    text = strip_html_entities(text)
    # Remove emoji and other non-ASCII
    text = re.sub(r"[^\x00-\x7F]", " ", text)
    # Remove digits and punctuation
    text = re.sub(r"[0-9.,()%+\-–—/\\'\"!?@#$^&*~`|\[\]{}:;]", " ", text)
    words = set()
    for word in text.split():
        word = word.strip("'\"-.")
        if len(word) >= 2 and word.isascii() and word.isalpha():
            words.add(word)
    return words


def has_untranslated_english(value: str) -> set[str]:
    """Check if a value contains English words that should be translated.

    Returns the set of untranslated English words found.
    """
    english_words = extract_english_words(value)
    untranslated = english_words - ALLOWED_ENGLISH_TOKENS
    return {w for w in untranslated if len(w) >= 2}


# ── Checks ─────────────────────────────────────────────────────────


def check_key_completeness() -> tuple[set[str], int]:
    """Check that all locales have the same keys as en-US."""
    print("─── Key Completeness ───")
    errors = 0
    all_keys = None
    for code in ALL_CODES:
        path = f"backend/resources/dashboard/locales/{code}.json"
        d = load_json(path)
        keys = set(d.keys()) - {"_meta"}
        missing = set()
        extra = set()
        if all_keys is None:
            all_keys = keys
        else:
            missing = all_keys - keys
            extra = keys - all_keys
            if missing:
                print(f"  ❌ {code}: MISSING {len(missing)} keys")
                for k in sorted(missing):
                    print(f"       {k}")
                errors += 1
            if extra:
                print(f"  ❌ {code}: EXTRA {len(extra)} keys (not in en-US)")
                errors += 1
        status = "✅" if (not missing and not extra) else "⚠️"
        print(f"  {status} {code}: {len(keys)} keys")
    return all_keys, errors


def check_html_wiring(all_keys: set[str]) -> int:
    """Check that all locale keys are referenced in the HTML."""
    print("\n─── HTML Wiring ───")
    errors = 0
    html_path = "backend/resources/dashboard/imp-console.html"
    with open(html_path) as f:
        html = f.read()

    html_keys = set(re.findall(r'data-i18n[-\w]*=[\'"]([^\'"]+)[\'"]', html))
    html_keys |= set(re.findall(r"t\(['\"]([^'\"]+)['\"]\)", html))

    unwired = all_keys - html_keys if all_keys else set()
    if unwired:
        print(f"  ❌ {len(unwired)} keys in locale files but NOT in HTML:")
        for k in sorted(unwired)[:30]:
            print(f"       {k}")
        if len(unwired) > 30:
            print(f"       ... and {len(unwired) - 30} more")
        errors += 1
    else:
        print(f"  ✅ All keys wired in HTML.")

    print(f"  HTML/JS references: {len(html_keys)}")
    print(f"  Total locale keys:  {len(all_keys) if all_keys else 0}")
    return errors


def check_non_latin_translation() -> int:
    """Check non-Latin-alphabet locales for untranslated English text.

    Returns the number of locales with issues.
    """
    print("\n─── Non-Latin Translation Gate (HARD REQUIREMENT) ───")
    total_issues = 0

    # ── Frontend JSON files ──
    print("  ── Frontend (JSON) ──")
    for code in sorted(NON_LATIN_CODES):
        path = f"backend/resources/dashboard/locales/{code}.json"
        d = load_json(path)
        meta = d.get("_meta", {})
        locale_name = meta.get("nativeName", code)

        untranslated: dict[str, set[str]] = {}
        for key, value in d.items():
            if key == "_meta":
                continue
            if not isinstance(value, str):
                continue
            words = has_untranslated_english(value)
            if words:
                untranslated[key] = words

        if untranslated:
            total_issues += 1
            print(f"\n  ❌ {code} ({locale_name}): {len(untranslated)} keys untranslated")
            for k in sorted(untranslated.keys())[:10]:
                val = d[k]
                display_val = val[:80] + "..." if len(val) > 80 else val
                # Simplified tag: if the value is entirely ASCII letters/spaces, it's English
                stripped_val = strip_html(val)
                stripped_val = strip_template_vars(stripped_val)
                stripped_val = re.sub(r"[^\x00-\x7F]", "", stripped_val)
                stripped_val = re.sub(r"[0-9.,()%+\-–—/\\'\"!?@#$^&*~`|\[\]{}:;]", " ", stripped_val)
                stripped_val = stripped_val.strip()
                # If after stripping HTML/template vars, the value is all ASCII letters, it's English
                is_entirely_english = bool(stripped_val) and all(
                    w.isascii() and w.isalpha() for w in stripped_val.split()
                )
                tag = "ENTIRELY ENGLISH" if is_entirely_english else "UNTRANSLATED"
                print(f"       {k}: \"{display_val}\"")
                print(f"         → {tag}")
            if len(untranslated) > 10:
                print(f"       ... and {len(untranslated) - 10} more keys")
        else:
            print(f"  ✅ {code} ({locale_name}): fully translated")

    # ── Backend FTL files ──
    print("\n  ── Backend (FTL) ──")
    ftl_issues = 0
    for code in sorted(NON_LATIN_CODES):
        path = f"backend/i18n/{code}/main.ftl"
        try:
            messages = load_ftl_keys(path)
        except FileNotFoundError:
            print(f"  ⚠️  {code}.ftl: file not found")
            continue

        untranslated: dict[str, set[str]] = {}
        for key, value in messages.items():
            words = has_untranslated_english(value)
            if words:
                untranslated[key] = words

        if untranslated:
            ftl_issues += 1
            print(f"\n  ❌ {code}.ftl: {len(untranslated)} keys untranslated")
            for k in sorted(untranslated.keys())[:10]:
                val = messages[k][:80]
                print(f"       {k}: \"{val}\"")
            if len(untranslated) > 10:
                print(f"       ... and {len(untranslated) - 10} more")
        else:
            print(f"  ✅ {code}.ftl: clean")

    return total_issues + ftl_issues


# ── Main ───────────────────────────────────────────────────────────


def main():
    errors = 0

    # 1. Key completeness
    all_keys, key_errors = check_key_completeness()
    errors += key_errors

    # 2. HTML wiring
    if all_keys:
        html_errors = check_html_wiring(all_keys)
        errors += html_errors

    # 3. Non-Latin translation gate
    tl_errors = check_non_latin_translation()
    errors += tl_errors

    # Summary
    print("\n" + "=" * 60)
    if errors == 0:
        print("✅ All checks passed — i18n is clean.")
        sys.exit(0)
    else:
        print(f"❌ {errors} check(s) failed.")
        if tl_errors > 0:
            print()
            print("  ⚠️  HARD REQUIREMENT: Non-Latin-alphabet locales must have NO English display text.")
            print("     Every display field in ar, he, ru, zh, ko, ja must be translated.")
            print("     Run translation workflow before committing.")
        sys.exit(1 if key_errors or html_errors else 2)


if __name__ == "__main__":
    main()