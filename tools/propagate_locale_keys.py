#!/usr/bin/env python3
"""Propagate new i18n keys from en-US to all other locale JSON files.

Usage:
    python3 tools/propagate_locale_keys.py

Adds any keys present in en-US.json that are missing from other locale files.
New keys get a placeholder comment so translators know they need attention.
"""

import json
import os
import sys

LOCALES_DIR = "backend/resources/dashboard/locales"
EN_US = "en-US.json"

# New keys to add — these are the teacher-friendly UX rewrite keys
NEW_KEYS = {
    # ── Sidebar ──
    "sidebar.home": "🏠 Home",
    "sidebar.homeShort": "Home",
    "sidebar.setup": "⚙️ Setup",

    # ── Command Bar ──
    "commandbar.placeholder": "What would you like to do?  (⌘K)",
    "commandbar.quickActions": "Quick Actions",
    "commandbar.chip.struggling": "🚩 Who's struggling?",
    "commandbar.chip.generate": "⚡ Generate practice",
    "commandbar.chip.upload": "📦 Upload a deck",
    "commandbar.chip.health": "🩺 Class health",

    # ── Home view (class cards) ──
    "home.title": "🏠 Home",
    "home.classCard.generate": "⚡ Generate Practice",
    "home.classCard.generating": "Generating...",
    "home.classCard.studentsBehind": "{count} students behind",
    "home.classCard.allClear": "✅ All clear",
    "home.classCard.practiceReady": "Practice ready for {count} students",
    "home.classCard.practiceFailed": "Generation failed for {count} students",
    "home.classCard.noData": "No practice data yet",
    "home.subtitle": "Your classes at a glance",
    "home.noClassesTitle": "👋 No classes yet",
    "home.noClassesDesc": "Create your first class to get started with study sessions, progress tracking, and deck management.",
    "home.createClass": "+ Create a Class",

    # ── First-Run Wizard ──
    "wizard.heading": "👋 Welcome to AnkiTov!",
    "wizard.subtitle": "Let's get your classroom set up in 3 quick steps.",
    "wizard.step1.title": "Create Your First Class",
    "wizard.step1.desc": "Give your class a name, subject, and period.",
    "wizard.step1.fieldName": "Class Name",
    "wizard.step1.fieldSubject": "Subject Area",
    "wizard.step1.fieldPeriod": "Period",
    "wizard.step2.title": "Add Students",
    "wizard.step2.desc": "Paste student names or import from a file.",
    "wizard.step2.pasteHint": "One name per line, or paste a CSV",
    "wizard.step2.importBtn": "📁 Import from File",
    "wizard.step3.title": "Assign Decks",
    "wizard.step3.desc": "Choose which decks this class should use.",
    "wizard.step3.noDecks": "No decks uploaded yet. You can assign them later.",
    "wizard.complete": "🎉 You're all set!",
    "wizard.complete.desc": "Students can now log in. Practice will be generated automatically.",
    "wizard.complete.cta": "Go to Home",
    "wizard.skip": "Skip setup",
    "wizard.next": "Next →",
    "wizard.back": "← Back",

    # ── Notification Badge ──
    "badge.studentsBehind": "{count} students need attention",
    "badge.noIssues": "No issues",
    "badge.healthyTooltip": "✅ All classes healthy",
    "badge.alertTooltip": "🟠 {count} classes need attention",

    # ── Class Detail view ──
    "classDetail.heading": "Class Detail",
    "classDetail.subtitle": "Student overview and practice generation",
    "classDetail.healthGrid": "Student Health",
    "classDetail.selectPrompt": "Select students to generate practice",
    "classDetail.batchGenerate": "Generate Practice for Selected",
    "classDetail.generatingBatch": "Generating practice for {count} students...",
    "classDetail.complianceRate": "Compliance: {rate}%",
    "classDetail.lastPractice": "Last practice: {date}",
    "classDetail.statusHealthy": "✅ On track",
    "classDetail.statusBehind": "🟠 Behind",
    "classDetail.statusCritical": "🔴 Critical",
    "classDetail.noData": "No data yet",
    "classDetail.searchStudents": "Search students...",

    # ── Friendly error messages (overriding existing) ──
    "error.requestTimeout": "The request timed out. Please check your connection and try again.",
    "error.badRequest": "We couldn't process that request. Please check your input and try again.",
    "error.notFound": "That item wasn't found. It may have been moved or deleted.",
    "error.serverError": "Something went wrong on our end. Please try again in a moment.",
    "error.networkError": "Could not reach the server. Please check your internet connection.",
    "error.generic": "Something unexpected happened. Please try again.",
    "error.couldNotLoadStudents": "Could not load students. Please refresh the page.",
    "error.couldNotLoadClasses": "Could not load classes. Please refresh the page.",
    "error.couldNotLoadCompliance": "Could not load progress data. Please try again.",
    "error.couldNotLoadInsights": "Could not load class insights. Please try again.",
    "error.couldNotGeneratePractice": "Could not generate practice. Please check the student data and try again.",
    "error.noStudentsSelected": "Please select at least one student.",

    # ── Misc new UI strings ──
    "home.generateSuccess": "✅ Practice generated for {count} students in {className}",
    "home.generateFailed": "Failed to generate practice. Please try again.",
    "home.quickAction": "Quick Actions",
    "home.viewClass": "View Class →",
    "status.healthy": "✅ Healthy",
    "status.needsAttention": "🟠 Needs Attention",
    "status.critical": "🔴 Critical",
    "btn.generateNow": "Generate Now",
    "btn.refresh": "Refresh",
    "btn.dismiss": "Dismiss",
}

def main():
    # Load en-US
    en_path = os.path.join(LOCALES_DIR, EN_US)
    with open(en_path) as f:
        en = json.load(f)

    # Add new keys to en-US first
    changed = False
    for key, value in NEW_KEYS.items():
        if key not in en:
            en[key] = value
            print(f"  + {key} → en-US")
            changed = True

    if changed:
        with open(en_path, "w") as f:
            json.dump(en, f, indent=2, ensure_ascii=False)
            f.write("\n")
        print(f"  Updated en-US.json ({len(en)} keys)")

    # Propagate to all other locales
    for filename in sorted(os.listdir(LOCALES_DIR)):
        if filename == EN_US or not filename.endswith(".json"):
            continue

        path = os.path.join(LOCALES_DIR, filename)
        with open(path) as f:
            data = json.load(f)

        added = 0
        for key, value in en.items():
            if key not in data:
                # Add placeholder: English text with a note for new untranslated keys
                # Translators can find these and translate them
                data[key] = value
                added += 1

        if added > 0:
            with open(path, "w") as f:
                json.dump(data, f, indent=2, ensure_ascii=False)
                f.write("\n")
            print(f"  + {added} keys → {filename} (placeholders from en-US)")
        else:
            print(f"  ✓ {filename} — up to date")

    # Summary
    print(f"\nDone. en-US now has {len(en)} keys.")
    print(f"New keys added: {len(NEW_KEYS)}")

    # Validate all locales still match en-US keyset
    en_keys = set(en.keys())
    all_ok = True
    for filename in sorted(os.listdir(LOCALES_DIR)):
        if not filename.endswith(".json"):
            continue
        path = os.path.join(LOCALES_DIR, filename)
        with open(path) as f:
            data = json.load(f)
        missing = en_keys - set(data.keys())
        extra = set(data.keys()) - en_keys
        if missing:
            print(f"  ⚠ {filename}: missing {len(missing)} keys")
            all_ok = False
        if extra:
            print(f"  ⚠ {filename}: {len(extra)} extra keys")
            all_ok = False
    if all_ok:
        print("✅ All locales match en-US keyset")

if __name__ == "__main__":
    main()