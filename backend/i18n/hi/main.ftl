# AnkiTov Backend — हिन्दी (Hindi) locale
# Fluent Translation Template

# ── Generic ──
app-name = AnkiTov
app-tagline = अदृश्य निपुणता अवसंरचना

# ── Errors ──
error-not-found = संसाधन नहीं मिला
error-unauthorized = अधिकृत नहीं — कृपया लॉग इन करें
error-internal = आंतरिक सर्वर त्रुटि — कृपया पुनः प्रयास करें
error-validation = मान्यकरण त्रुटि: { $detail }
error-rate-limit = बहुत अधिक अनुरोध — कृपया प्रतीक्षा करें और पुनः प्रयास करें
error-class-not-found = कक्षा "{ $id }" नहीं मिली
error-student-not-found = छात्र "{ $id }" नहीं मिला
error-track-not-found = ट्रैक "{ $id }" नहीं मिला
error-profile-not-found = अभ्यास योजना "{ $id }" नहीं मिली
error-deck-not-found = डेक नहीं मिला
error-upload-failed = Upload failed: { $reason }
error-sync-failed = Sync operation failed: { $reason }
error-enroll-failed = छात्र नामांकन विफल: { $reason }
error-transfer-failed = छात्र स्थानांतरण विफल: { $reason }
error-capsule-generation-failed = कैप्सूल निर्माण विफल: { $reason }

# ── Success ──
success-created = { $entity } सफलतापूर्वक बनाया गया
success-updated = { $entity } सफलतापूर्वक अद्यतन किया गया
success-deleted = { $entity } सफलतापूर्वक हटाया गया
success-enrolled = छात्र सफलतापूर्वक नामांकित हुआ
success-transferred = छात्र सफलतापूर्वक स्थानांतरित हुआ
success-capsule-generated = अध्ययन डेक सफलतापूर्वक तैयार हुआ
success-deck-uploaded = डेक "{ $name }" सफलतापूर्वक अपलोड हुआ
success-deck-distributed = डेक { $count } लक्ष्यों को वितरित किया गया
success-sync-started = पूर्ण सिंक शुरू हुआ — इसमें कुछ मिनट लग सकते हैं

# ── Notifications ──
notify-health-ok = 🟢 सिस्टम ठीक है
notify-health-unreachable = 🔴 पहुंच योग्य नहीं
notify-syncing = सिंक हो रहा है…
notify-loading = लोड हो रहा है…

# ── Audit log messages ──
audit-capsule-generated = { $student } के लिए कैप्सूल सत्र तैयार हुआ
audit-n-value-changed = N-मूल्य { $old } से { $new } में बदला, { $target } के लिये
audit-profile-assigned = Practice Plan "{ $profile }" assigned to { $student }
audit-profile-revoked = Practice Plan "{ $profile }" revoked from { $student }
audit-class-created = Class "{ $name }" created
audit-class-deleted = Class "{ $name }" deleted
audit-deck-uploaded = Deck "{ $name }" uploaded ({ $size } bytes)
audit-deck-distributed = Deck "{ $name }" distributed to { $target_type } "{ $target }"


# ── Student Import from File ──
import-file-title = फ़ाइल से छात्र आयात करें
import-file-description = छात्र रोस्टर के साथ CSV, TSV या Excel फ़ाइल अपलोड करें
import-file-dropzone = फ़ाइल यहाँ छोड़ें या ब्राउज़ करने के लिए क्लिक करें
import-file-accepted = Accepted formats: CSV, TSV, Excel (.xlsx)
import-file-class = लक्ष्य कक्षा
import-file-submit = छात्र आयात करें
import-file-success = { $count } छात्र सफलतापूर्वक आयात हुए
import-file-skipped = { $count } छोड़े गए (पहले से नामांकित)
import-file-errors = { $count } पंक्ति(याँ) में त्रुटियाँ हैं
import-file-missing-column = आवश्यक स्तंभ गायब: { $column }
import-file-empty-file = फ़ाइल खाली है
import-file-invalid-format = असमर्थित फ़ाइल प्रारूप। CSV, TSV या .xlsx का उपयोग करें
import-file-parse-error = फ़ाइल पढ़ी नहीं जा सकी: { $reason }
import-file-preview = पूर्वावलोकन ({ $count } पंक्तियाँ)
import-file-download-template = टेम्पलेट डाउनलोड करें

# ── Teacher Console UX Rewrite (2026-08-09) ──

# Friendlier API responses for batch operations
error-practice-generation = अभ्यास निर्माण विफल: { $reason }
error-batch-generate = { $count } छात्रों के लिए अभ्यास नहीं बनाया जा सका। उनका डेटा जाँचें और पुनः प्रयास करें।
error-no-students-selected = कृपया अभ्यास बनाने के लिए कम से कम एक छात्र चुनें।
notify-practice-in-progress = { $count } छात्रों के लिए अभ्यास तैयार हो रहा है...
notify-practice-complete = { $count } छात्रों के लिए अभ्यास सत्र तैयार हैं

# Health status (for notification badge)
notify-health-attention = { $count } कक्षाओं को ध्यान देने की आवश्यकता है