# AnkiTov Backend — עברית (Hebrew) locale
# Fluent Translation Template

# ── Generic ──
app-name = אנקיטוב
app-tagline = תשתית שליטה בלתי נראית

# ── Errors ──
error-not-found = המשאב לא נמצא
error-unauthorized = אין הרשאה — יש להתחבר
error-internal = שגיאת שרת פנימית — נסה שוב
error-validation = שגיאת אימות: { $detail }
error-rate-limit = יותר מדי בקשות — המתן ונסה שוב
error-class-not-found = הכיתה "{ $id }" לא נמצאה
error-student-not-found = התלמיד "{ $id }" לא נמצא
error-track-not-found = המסלול "{ $id }" לא נמצא
error-profile-not-found = תוכנית האימון "{ $id }" לא נמצאה
error-deck-not-found = החפיסה לא נמצאה
error-upload-failed = ההעלה נכשלה: { $reason }
error-sync-failed = פעולת הסנכרון נכשלה: { $reason }
error-enroll-failed = רישום התלמיד נכשל: { $reason }
error-transfer-failed = העברת התלמיד נכשלה: { $reason }
error-capsule-generation-failed = יצירת חבילת הלימוד נכשלה: { $reason }

# ── Success ──
success-created = { $entity } נוצר בהצלחה
success-updated = { $entity } עודכן בהצלחה
success-deleted = { $entity } נמחק בהצלחה
success-enrolled = התלמיד נרשם בהצלחה
success-transferred = התלמיד הועבר בהצלחה
success-capsule-generated = חבילת הלימוד נוצרה בהצלחה
success-deck-uploaded = החפיסה "{ $name }" הועלתה בהצלחה
success-deck-distributed = החפיסה הופצה ל־{ $count } יעדים
success-sync-started = סנכרון מלא התחיל — עשוי להימשך מספר דקות

# ── Notifications ──
notify-health-ok = 🟢 המערכת תקינה
notify-health-unreachable = 🔴 אין תקשורת
notify-syncing = מסנכרן…
notify-loading = טוען…

# ── Audit log messages ──
audit-capsule-generated = חבילת לימוד נוצרה עבור { $student }
audit-n-value-changed = ערך N שונה מ־{ $old } ל־{ $new } עבור { $target }
audit-profile-assigned = תוכנית אימון "{ $profile }" שויכת ל־{ $student }
audit-profile-revoked = תוכנית אימון "{ $profile }" שושפעה מ־{ $student }
audit-class-created = הכיתה "{ $name }" נוצרה
audit-class-deleted = הכיתה "{ $name }" נמחקה
audit-deck-uploaded = החפיסה "{ $name }" הועלתה ({ $size } בתים)
audit-deck-distributed = החפיסה "{ $name }" הופצה אל { $target_type } "{ $target }"


# ── Student Import from File ──
import-file-title = ייבא תלמידים מקובץ
import-file-description = העלה קובץ CSV, TSV או Excel עם רשימות תלמידים
import-file-dropzone = השלך קובץ כאן או לחץ כדי לעיין
import-file-accepted = פורמטים נתמכים: CSV, TSV, Excel (.xlsx)
import-file-class = כיתת יעד
import-file-submit = ייבא תלמידים
import-file-success = { $count } תלמידים יובאו בהצלחה
import-file-skipped = { $count } בילו (נרשמו כבר)
import-file-errors = { $count } שורה(ות) הכילה שגיאות
import-file-missing-column = טור חובה חסר: { $column }
import-file-empty-file = הקובץ ריק
import-file-invalid-format = פורמט קובץ לא נתמך. השתמש ב-CSV, TSV או .xlsx
import-file-parse-error = לא ניתן לקרוא את הקובץ: { $reason }
import-file-preview = תצוגה מקדימה ({ $count } שורות)
import-file-download-template = הורד תבנית

# ── Teacher Console UX Rewrite (2026-08-09) ──
error-practice-generation = יצירת התרגול נכשלה: { $reason }
error-batch-generate = לא ניתן ליצור תרגול עבור { $count } תלמידים. בדוק את הנתונים שלהם ונסה שוב.
error-no-students-selected = אנא בחר לפחות תלמיד אחד ליצירת תרגול.
notify-practice-in-progress = מייצר תרגול עבור { $count } תלמידים...
notify-practice-complete = התרגולים מוכנים עבור { $count } תלמידים

# Health status (for notification badge)
notify-health-attention = { $count } כיתות זקוקות לתשומת לב
