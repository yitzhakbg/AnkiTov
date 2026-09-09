# AnkiTov Backend — Italiano (Italian) locale
# Fluent Translation Template (Fluent = ftl)

# ── Generic ──
app-name = AnkiTov
app-tagline = Infrastruttura Invisibile di Padronanza

# ── Errors ──
error-not-found = Risorsa non trovata
error-unauthorized = Non autorizzato — accedi
error-internal = Errore interno del server — riprova
error-validation = Errore di validazione: { $detail }
error-rate-limit = Troppe richieste — attendi e riprova
error-class-not-found = Classe "{ $id }" non trovata
error-student-not-found = Studente "{ $id }" non trovato
error-track-not-found = Modulo "{ $id }" non trovato
error-profile-not-found = Piano di Pratica "{ $id }" non trovato
error-deck-not-found = Mazzo non trovato
error-upload-failed = Caricamento fallito: { $reason }
error-sync-failed = Operazione di sincronizzazione fallita: { $reason }
error-enroll-failed = Iscrizione studente fallita: { $reason }
error-transfer-failed = Trasferimento studente fallito: { $reason }
error-capsule-generation-failed = Generazione sessione di studio fallita: { $reason }

# ── Success ──
success-created = { $entity } creato con successo
success-updated = { $entity } aggiornato con successo
success-deleted = { $entity } eliminato con successo
success-enrolled = Studente iscritto con successo
success-transferred = Studente trasferito con successo
success-capsule-generated = Capsula di studio generata con successo
success-deck-uploaded = Mazzo "{ $name }" caricato con successo
success-deck-distributed = Mazzo distribuito a { $count } destinatario(i)
success-sync-started = Sincronizzazione completa avviata — potrebbe richiedere alcuni minuti

# ── Notifications ──
notify-health-ok = 🟢 Sistema OK
notify-health-unreachable = 🔴 Irraggiungibile
notify-syncing = Sincronizzazione…
notify-loading = Caricamento…

# ── Audit log messages ──
audit-capsule-generated = Sessione di studio generata per { $student }
audit-n-value-changed = Valore-N cambiato da { $old } a { $new } per { $target }
audit-profile-assigned = Piano di Pratica "{ $profile }" assegnato a { $student }
audit-profile-revoked = Piano di Pratica "{ $profile }" revocato da { $student }
audit-class-created = Classe "{ $name }" creata
audit-class-deleted = Classe "{ $name }" eliminata
audit-deck-uploaded = Mazzo "{ $name }" caricato ({ $size } byte)
audit-deck-distributed = Mazzo "{ $name }" distribuito a { $target_type } "{ $target }"


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
error-practice-generation = Generazione della pratica non riuscita: { $reason }
error-batch-generate = Impossibile generare la pratica per { $count } studenti. Controlla i loro dati e riprova.
error-no-students-selected = Seleziona almeno uno studente per generare la pratica.
notify-practice-in-progress = Generazione della pratica per { $count } studenti...
notify-practice-complete = Sessioni di pratica pronte per { $count } studenti

# Health status (for notification badge)
notify-health-attention = { $count } classi richiedono attenzione
