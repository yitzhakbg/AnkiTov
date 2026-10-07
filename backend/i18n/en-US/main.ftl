# AnkiTov Backend — en-US locale
# Fluent Translation Template (Fluent = ftl)

# ── Generic ──
app-name = AnkiTov
app-tagline = Invisible Mastery Infrastructure

# ── Errors ──
error-not-found = Resource not found
error-unauthorized = Unauthorized — please log in
error-internal = Internal server error — please try again
error-validation = Validation error: { $detail }
error-rate-limit = Too many requests — please wait and try again
error-class-not-found = Class "{ $id }" not found
error-student-not-found = Student "{ $id }" not found
error-track-not-found = Track "{ $id }" not found
error-profile-not-found = Practice Plan "{ $id }" not found
error-deck-not-found = Deck not found
error-upload-failed = Upload failed: { $reason }
error-sync-failed = Sync operation failed: { $reason }
error-enroll-failed = Failed to enroll student: { $reason }
error-transfer-failed = Failed to transfer student: { $reason }
error-capsule-generation-failed = Capsule generation failed: { $reason }

# ── Success ──
success-created = { $entity } created successfully
success-updated = { $entity } updated successfully
success-deleted = { $entity } deleted successfully
success-enrolled = Student enrolled successfully
success-transferred = Student transferred successfully
success-capsule-generated = Study deck generated successfully
success-deck-uploaded = Deck "{ $name }" uploaded successfully
success-deck-distributed = Deck distributed to { $count } targets
success-sync-started = Full sync initiated — this may take a few minutes

# ── Notifications ──
notify-health-ok = 🟢 System OK
notify-health-unreachable = 🔴 Unreachable
notify-syncing = Syncing…
notify-loading = Loading…

# ── Audit log messages ──
audit-capsule-generated = Capsule session generated for { $student }
audit-n-value-changed = N-Value changed from { $old } to { $new } for { $target }
audit-profile-assigned = Practice Plan "{ $profile }" assigned to { $student }
audit-profile-revoked = Practice Plan "{ $profile }" revoked from { $student }
audit-class-created = Class "{ $name }" created
audit-class-deleted = Class "{ $name }" deleted
audit-deck-uploaded = Deck "{ $name }" uploaded ({ $size } bytes)
audit-deck-distributed = Deck "{ $name }" distributed to { $target_type } "{ $target }"


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

# Friendlier API responses for batch operations
error-practice-generation = Practice generation failed: { $reason }
error-batch-generate = Could not generate practice for { $count } students. Check their data and try again.
error-no-students-selected = Please select at least one student to generate practice.
notify-practice-in-progress = Generating practice for { $count } students...
notify-practice-complete = Practice sessions ready for { $count } students

# Health status (for notification badge)
notify-health-attention = { $count } classes need attention

# ── Teacher enrollment (Prong 8) ──
teacher-enroll-title            = Register a teacher
teacher-enroll-description     = Enroll a new teacher into AnkiTov. You will receive an invite link to share.
teacher-enroll-field-name      = Full name
teacher-enroll-field-email     = Email (login)
teacher-enroll-field-school    = School / community
teacher-enroll-field-role      = Role or class
teacher-enroll-field-locale    = Language
teacher-enroll-err-dupe-email  = A teacher with that email is already enrolled
teacher-enroll-err-dupe-name   = A teacher with that name is already enrolled
teacher-enroll-err-locale      = Unsupported locale "{ $locale }"
teacher-enroll-ok              = { $name } registered. Share this invite link: { $url }
teacher-invite-expired         = This invite link has expired. Ask your administrator for a new one.
teacher-invite-revoked         = This invite link was revoked.
teacher-firstlogin-title       = Set your AnkiTov password
teacher-firstlogin-consent     = I confirm I am a teacher at { $school } and I consent to AnkiTov processing my teaching data (see privacy notice).
teacher-firstlogin-err-policy  = Password must be at least 8 characters and contain a digit.

# ── i18n test probe (present ONLY in en-US — exercises the fallback chain) ──
i18n-test-probe = AnkiTov fallback probe
