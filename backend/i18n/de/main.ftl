# AnkiTov Backend — Deutsch (German) locale
# Fluent Translation Template (Fluent = ftl)

# ── Generic ──
app-name = AnkiTov
app-tagline = Unsichtbare Meisterschafts-Infrastruktur

# ── Errors ──
error-not-found = Ressource nicht gefunden
error-unauthorized = Nicht autorisiert — bitte anmelden
error-internal = Interner Serverfehler — bitte versuchen Sie es erneut
error-validation = Validierungsfehler: { $detail }
error-rate-limit = Zu viele Anfragen — bitte warten und erneut versuchen
error-class-not-found = Klasse "{ $id }" nicht gefunden
error-student-not-found = Schüler "{ $id }" nicht gefunden
error-track-not-found = Modul "{ $id }" nicht gefunden
error-profile-not-found = Übungsplan "{ $id }" nicht gefunden
error-deck-not-found = Deck nicht gefunden
error-upload-failed = Hochladen fehlgeschlagen: { $reason }
error-sync-failed = Sync-Vorgang fehlgeschlagen: { $reason }
error-enroll-failed = Schüleranmeldung fehlgeschlagen: { $reason }
error-transfer-failed = Schülerversetzung fehlgeschlagen: { $reason }
error-capsule-generation-failed = Generierung der Lernsitzung fehlgeschlagen: { $reason }

# ── Success ──
success-created = { $entity } erfolgreich erstellt
success-updated = { $entity } erfolgreich aktualisiert
success-deleted = { $entity } erfolgreich gelöscht
success-enrolled = Schüler erfolgreich angemeldet
success-transferred = Schüler erfolgreich versetzt
success-capsule-generated = Lernkapsel erfolgreich generiert
success-deck-uploaded = Deck "{ $name }" erfolgreich hochgeladen
success-deck-distributed = Deck an { $count } Ziel(e) verteilt
success-sync-started = Vollständige Synchronisation gestartet — dies kann einige Minuten dauern

# ── Notifications ──
notify-health-ok = 🟢 System OK
notify-health-unreachable = 🔴 Nicht erreichbar
notify-syncing = Synchronisiere…
notify-loading = Lade…

# ── Audit log messages ──
audit-capsule-generated = Lernsitzung für { $student } generiert
audit-n-value-changed = N-Wert von { $old } auf { $new } für { $target } geändert
audit-profile-assigned = Übungsplan "{ $profile }" { $student } zugewiesen
audit-profile-revoked = Übungsplan "{ $profile }" von { $student } entzogen
audit-class-created = Klasse "{ $name }" erstellt
audit-class-deleted = Klasse "{ $name }" gelöscht
audit-deck-uploaded = Deck "{ $name }" hochgeladen ({ $size } Bytes)
audit-deck-distributed = Deck "{ $name }" an { $target_type } "{ $target }" verteilt


# ── Student Import from File ──
import-file-title = Import Students from File
import-file-description = Upload a CSV, TSV, or Excel file with student rosters
import-file-dropzone = Drop file here or click to browse
import-file-accepted = Accepted formats: CSV, TSV, Excel (.xlsx)
import-file-class = Target Class
import-file-submit = Import Students
import-file-success = { $count } students imported successfully
import-file-skipped = { $count } skipped (already enrolled)
import-file-errors = { $count } row(s) had errors
import-file-missing-column = Missing required column: { $column }
import-file-empty-file = File is empty
import-file-invalid-format = Unsupported file format. Use CSV, TSV, or .xlsx
import-file-parse-error = Could not read file: { $reason }
import-file-preview = Preview ({ $count } rows)
import-file-download-template = Download template

# ── Teacher Console UX Rewrite (2026-08-09) ──
error-practice-generation = Praxis-Generierung fehlgeschlagen: { $reason }
error-batch-generate = Praxis konnte für { $count } Schüler nicht generiert werden. Überprüfen Sie die Daten und versuchen Sie es erneut.
error-no-students-selected = Bitte wählen Sie mindestens einen Schüler für die Praxis-Generierung aus.
notify-practice-in-progress = Praxis wird für { $count } Schüler generiert...
notify-practice-complete = Praxis-Sitzungen bereit für { $count } Schüler

# Health status (for notification badge)
notify-health-attention = { $count } Klassen benötigen Aufmerksamkeit
