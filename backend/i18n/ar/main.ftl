# AnkiTov Backend — العربية (Arabic) locale
# Fluent Translation Template

# ── Generic ──
app-name = أنكيتوف
app-tagline = بنية الإتقان غير المرئية

# ── Errors ──
error-not-found = المورد غير موجود
error-unauthorized = غير مصرح — يرجى تسجيل الدخول
error-internal = خطأ داخلي في الخادم — يرجى المحاولة مرة أخرى
error-validation = خطأ في التحقق: { $detail }
error-rate-limit = طلبات كثيرة جدًا — يرجى الانتظار والمحاولة مرة أخرى
error-class-not-found = الفصل "{ $id }" غير موجود
error-student-not-found = الطالب "{ $id }" غير موجود
error-track-not-found = المسار "{ $id }" غير موجود
error-profile-not-found = خطة التدريب "{ $id }" غير موجودة
error-deck-not-found = المجموعة غير موجودة
error-upload-failed = فشل الرفع: { $reason }
error-sync-failed = فشلت عملية المزامنة: { $reason }
error-enroll-failed = فشل تسجيل الطالب: { $reason }
error-transfer-failed = فشل نقل الطالب: { $reason }
error-capsule-generation-failed = فشل إنشاء جلسة الدراسة: { $reason }

# ── Success ──
success-created = تم إنشاء { $entity } بنجاح
success-updated = تم تحديث { $entity } بنجاح
success-deleted = تم حذف { $entity } بنجاح
success-enrolled = تم تسجيل الطالب بنجاح
success-transferred = تم نقل الطالب بنجاح
success-capsule-generated = تم إنشاء مجموعة الدراسة بنجاح
success-deck-uploaded = تم رفع المجموعة "{ $name }" بنجاح
success-deck-distributed = تم توزيع المجموعة على { $count } هدف
success-sync-started = بدأت المزامنة الكاملة — قد تستغرق بضع دقائق

# ── Notifications ──
notify-health-ok = 🟢 النظام يعمل
notify-health-unreachable = 🔴 لا يمكن الوصول
notify-syncing = جاري المزامنة…
notify-loading = جاري التحميل…

# ── Audit log messages ──
audit-capsule-generated = تم إنشاء جلسة مراجعة لـ { $student }
audit-n-value-changed = تم تغيير قيمة N من { $old } إلى { $new } لـ { $target }
audit-profile-assigned = تم تعيين خطة التدريب "{ $profile }" لـ { $student }
audit-profile-revoked = تم إلغاء خطة التدريب "{ $profile }" من { $student }
audit-class-created = تم إنشاء الفصل "{ $name }"
audit-class-deleted = تم حذف الفصل "{ $name }"
audit-deck-uploaded = تم رفع المجموعة "{ $name }" ({ $size } بايت)
audit-deck-distributed = تم توزيع المجموعة "{ $name }" على { $target_type } "{ $target }"


# ── Student Import from File ──
import-file-title = استيراد الطلاب من ملف
import-file-description = حمّل ملف CSV أو TSV أو Excel بقوائم الطلاب
import-file-dropzone = أسقط الملف هنا أو انقر للتصفح
import-file-accepted = الصيغ المقبولة: CSV، TSV، Excel (.xlsx)
import-file-class = الفصل المستهدف
import-file-submit = استيراد الطلاب
import-file-success = تم استيراد { $count } طالب(ة) بنجاح
import-file-skipped = { $count } تم تخطيهم (مسجلون بالفعل)
import-file-errors = { $count } صف(وف) بها أخطاء
import-file-missing-column = العمود المطلوب مفقود: { $column }
import-file-empty-file = الملف فارغ
import-file-invalid-format = صيغة ملف غير مدعومة. استخدم CSV أو TSV أو .xlsx
import-file-parse-error = تعذرت قراءة الملف: { $reason }
import-file-preview = معاينة ({ $count } صف)
import-file-download-template = تنزيل النموذج

# ── Teacher Console UX Rewrite (2026-08-09) ──
error-practice-generation = فشل إنشاء الممارسة: { $reason }
error-batch-generate = تعذر إنشاء الممارسة لـ { $count } طالب. يرجى التحقق من بياناتهم والمحاولة مرة أخرى.
error-no-students-selected = يرجى اختيار طالب واحد على الأقل لإنشاء الممارسة.
notify-practice-in-progress = جارٍ إنشاء الممارسة لـ { $count } طالب...
notify-practice-complete = جلسات الممارسة جاهزة لـ { $count } طالب

# Health status (for notification badge)
notify-health-attention = { $count } فئة تحتاج إلى انتباه
