# AnkiTov Backend — Русский (Russian) locale
# Fluent Translation Template (Fluent = ftl)

# ── Generic ──
app-name = AnkiTov
app-tagline = Невидимая Инфраструктура Мастерства

# ── Errors ──
error-not-found = Ресурс не найден
error-unauthorized = Не авторизован — пожалуйста, войдите
error-internal = Внутренняя ошибка сервера — попробуйте снова
error-validation = Ошибка проверки: { $detail }
error-rate-limit = Слишком много запросов — подождите и попробуйте снова
error-class-not-found = Класс "{ $id }" не найден
error-student-not-found = Студент "{ $id }" не найден
error-track-not-found = Модуль "{ $id }" не найден
error-profile-not-found = План Практики "{ $id }" не найден
error-deck-not-found = Колода не найдена
error-upload-failed = Ошибка загрузки: { $reason }
error-sync-failed = Ошибка синхронизации: { $reason }
error-enroll-failed = Ошибка зачисления студента: { $reason }
error-transfer-failed = Ошибка перевода студента: { $reason }
error-capsule-generation-failed = Ошибка создания учебной сессии: { $reason }

# ── Success ──
success-created = { $entity } успешно создан
success-updated = { $entity } успешно обновлён
success-deleted = { $entity } успешно удалён
success-enrolled = Студент успешно зачислен
success-transferred = Студент успешно переведён
success-capsule-generated = Учебная капсула успешно создана
success-deck-uploaded = Колода "{ $name }" успешно загружена
success-deck-distributed = Колода распространена по { $count } цели(ям)
success-sync-started = Полная синхронизация запущена — это может занять несколько минут

# ── Notifications ──
notify-health-ok = 🟢 Система ОК
notify-health-unreachable = 🔴 Недоступно
notify-syncing = Синхронизация…
notify-loading = Загрузка…

# ── Audit log messages ──
audit-capsule-generated = Учебная сессия создана для { $student }
audit-n-value-changed = Значение-N изменено с { $old } на { $new } для { $target }
audit-profile-assigned = План Практики "{ $profile }" назначен { $student }
audit-profile-revoked = План Практики "{ $profile }" отозван у { $student }
audit-class-created = Класс "{ $name }" создан
audit-class-deleted = Класс "{ $name }" удалён
audit-deck-uploaded = Колода "{ $name }" загружена ({ $size } байт)
audit-deck-distributed = Колода "{ $name }" распространена на { $target_type } "{ $target }"


# ── Student Import from File ──
import-file-title = Импорт учеников из файла
import-file-description = Загрузите файл CSV, TSV или Excel со списками учеников
import-file-dropzone = Перетащите файл сюда или нажмите для выбора
import-file-accepted = Принимаемые форматы: CSV, TSV, Excel (.xlsx)
import-file-class = Целевой класс
import-file-submit = Импорт учеников
import-file-success = { $count } учеников успешно импортировано
import-file-skipped = { $count } пропущено (уже зачислены)
import-file-errors = { $count } строка(и) содержит ошибки
import-file-missing-column = Отсутствует обязательная колонка: { $column }
import-file-empty-file = Файл пуст
import-file-invalid-format = Неподдерживаемый формат файла. Используйте CSV, TSV или .xlsx
import-file-parse-error = Не удалось прочитать файл: { $reason }
import-file-preview = Предпросмотр ({ $count } строк)
import-file-download-template = Скачать шаблон

# ── Teacher Console UX Rewrite (2026-08-09) ──
error-practice-generation = Не удалось создать тренировку: { $reason }
error-batch-generate = Не удалось создать тренировку для { $count } учеников. Проверьте их данные и попробуйте снова.
error-no-students-selected = Пожалуйста, выберите хотя бы одного ученика для создания тренировки.
notify-practice-in-progress = Создание тренировки для { $count } учеников...
notify-practice-complete = Тренировки готовы для { $count } учеников

# Health status (for notification badge)
notify-health-attention = { $count } классов требуют внимания
