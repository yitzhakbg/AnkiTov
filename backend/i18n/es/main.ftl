# AnkiTov Backend — Español (Spanish) locale
# Fluent Translation Template (Fluent = ftl)

# ── Generic ──
app-name = AnkiTov
app-tagline = Infraestructura Invisible de Maestría

# ── Errors ──
error-not-found = Recurso no encontrado
error-unauthorized = No autorizado — inicie sesión
error-internal = Error interno del servidor — intente de nuevo
error-validation = Error de validación: { $detail }
error-rate-limit = Demasiadas solicitudes — espere e intente de nuevo
error-class-not-found = Clase "{ $id }" no encontrada
error-student-not-found = Estudiante "{ $id }" no encontrado
error-track-not-found = Módulo "{ $id }" no encontrado
error-profile-not-found = Plan de Práctica "{ $id }" no encontrado
error-deck-not-found = Mazo no encontrado
error-upload-failed = Error al subir: { $reason }
error-sync-failed = Error de sincronización: { $reason }
error-enroll-failed = Error al matricular estudiante: { $reason }
error-transfer-failed = Error al transferir estudiante: { $reason }
error-capsule-generation-failed = Error al generar sesión de estudio: { $reason }

# ── Success ──
success-created = { $entity } creado exitosamente
success-updated = { $entity } actualizado exitosamente
success-deleted = { $entity } eliminado exitosamente
success-enrolled = Estudiante matriculado exitosamente
success-transferred = Estudiante transferido exitosamente
success-capsule-generated = Cápsula de estudio generada exitosamente
success-deck-uploaded = Mazo "{ $name }" subido exitosamente
success-deck-distributed = Mazo distribuido a { $count } destino(s)
success-sync-started = Sincronización completa iniciada — puede tardar unos minutos

# ── Notifications ──
notify-health-ok = 🟢 Sistema OK
notify-health-unreachable = 🔴 Inaccesible
notify-syncing = Sincronizando…
notify-loading = Cargando…

# ── Audit log messages ──
audit-capsule-generated = Sesión de estudio generada para { $student }
audit-n-value-changed = Valor-N cambiado de { $old } a { $new } para { $target }
audit-profile-assigned = Plan de Práctica "{ $profile }" asignado a { $student }
audit-profile-revoked = Plan de Práctica "{ $profile }" revocado de { $student }
audit-class-created = Clase "{ $name }" creada
audit-class-deleted = Clase "{ $name }" eliminada
audit-deck-uploaded = Mazo "{ $name }" subido ({ $size } bytes)
audit-deck-distributed = Mazo "{ $name }" distribuido a { $target_type } "{ $target }"


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
error-practice-generation = Fallo al generar la práctica: { $reason }
error-batch-generate = No se pudo generar la práctica para { $count } estudiantes. Compruebe sus datos e intente de nuevo.
error-no-students-selected = Seleccione al menos un estudiante para generar la práctica.
notify-practice-in-progress = Generando práctica para { $count } estudiantes...
notify-practice-complete = Sesiones de práctica listas para { $count } estudiantes

# Health status (for notification badge)
notify-health-attention = { $count } clases necesitan atención
