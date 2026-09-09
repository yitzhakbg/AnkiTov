# AnkiTov 백엔드 — ko 로케일
# Fluent 번역 템플릿 (Fluent = ftl)

# ── 일반 ──
app-name = AnkiTov
app-tagline = 보이지 않는 숙달 인프라

# ── 오류 ──
error-not-found = 리소스를 찾을 수 없습니다
error-unauthorized = 권한 없음 — 로그인해 주세요
error-internal = 내부 서버 오류 — 다시 시도해 주세요
error-validation = 유효성 검사 오류: { $detail }
error-rate-limit = 요청이 너무 많습니다 — 잠시 기다렸다가 다시 시도해 주세요
error-class-not-found = 수업 "{ $id }"을(를) 찾을 수 없습니다
error-student-not-found = 학생 "{ $id }"을(를) 찾을 수 없습니다
error-track-not-found = 트랙 "{ $id }"을(를) 찾을 수 없습니다
error-profile-not-found = 학습 계획 "{ $id }"을(를) 찾을 수 없습니다
error-deck-not-found = 덱을 찾을 수 없습니다
error-upload-failed = 업로드 실패: { $reason }
error-sync-failed = 동기화 작업 실패: { $reason }
error-enroll-failed = 학생 등록 실패: { $reason }
error-transfer-failed = 학생 전학 실패: { $reason }
error-capsule-generation-failed = 학습 세션 생성 실패: { $reason }

# ── 성공 ──
success-created = { $entity }(이)가 성공적으로 생성되었습니다
success-updated = { $entity }(이)가 성공적으로 업데이트되었습니다
success-deleted = { $entity }(이)가 성공적으로 삭제되었습니다
success-enrolled = 학생이 성공적으로 등록되었습니다
success-transferred = 학생이 성공적으로 전학 처리되었습니다
success-capsule-generated = 학습 덱이 성공적으로 생성되었습니다
success-deck-uploaded = 덱 "{ $name }"이(가) 성공적으로 업로드되었습니다
success-deck-distributed = 덱이 { $count }개 대상에 배포되었습니다
success-sync-started = 전체 동기화가 시작되었습니다 — 몇 분 정도 걸릴 수 있습니다

# ── 알림 ──
notify-health-ok = 🟢 시스템 정상
notify-health-unreachable = 🔴 연결 불가
notify-syncing = 동기화 중…
notify-loading = 로딩 중…

# ── 감사 로그 메시지 ──
audit-capsule-generated = { $student }님을 위한 학습 세션이 생성되었습니다
audit-n-value-changed = { $target }의 N-값이 { $old }에서 { $new }(으)로 변경되었습니다
audit-profile-assigned = 학습 계획 "{ $profile }"이(가) { $student }님에게 할당되었습니다
audit-profile-revoked = 학습 계획 "{ $profile }"이(가) { $student }님에게서 철회되었습니다
audit-class-created = 수업 "{ $name }"이(가) 생성되었습니다
audit-class-deleted = 수업 "{ $name }"이(가) 삭제되었습니다
audit-deck-uploaded = 덱 "{ $name }"이(가) 업로드되었습니다 ({ $size }바이트)
audit-deck-distributed = 덱 "{ $name }"이(가) { $target_type } "{ $target }"에 배포되었습니다

# ── Student Import from File ──
import-file-title = 파일에서 학생 가져오기
import-file-description = 학생 명단이 포함된 CSV, TSV 또는 Excel 파일 업로드
import-file-dropzone = 파일을 여기에 놓거나 클릭하여 찾아보기
import-file-accepted = 허용 형식: CSV, TSV, Excel (.xlsx)
import-file-class = 대상 수업
import-file-submit = 학생 가져오기
import-file-success = { $count }명의 학생을 성공적으로 가져왔습니다
import-file-skipped = { $count }개 건너뜀 (이미 등록됨)
import-file-errors = { $count }개 행에 오류가 있습니다
import-file-missing-column = 필수 열이 누락되었습니다: { $column }
import-file-empty-file = 파일이 비어 있습니다
import-file-invalid-format = 지원되지 않는 파일 형식입니다. CSV, TSV 또는 .xlsx를 사용하세요
import-file-parse-error = 파일을 읽을 수 없습니다: { $reason }
import-file-preview = 미리보기 ({ $count }개 행)
import-file-download-template = 템플릿 다운로드

# ── Teacher Console UX Rewrite (2026-08-09) ──
error-practice-generation = 연습 생성 실패: { $reason }
error-batch-generate = { $count }명의 학생에 대한 연습을 생성할 수 없습니다. 데이터를 확인하고 다시 시도하세요.
error-no-students-selected = 연습을 생성하려면 최소 한 명의 학생을 선택하세요.
notify-practice-in-progress = { $count }명의 학생에 대한 연습 생성 중...
notify-practice-complete = { $count }명의 학생 연습 세션이 준비되었습니다

# Health status (for notification badge)
notify-health-attention = { $count }개 학급에 주의가 필요합니다
