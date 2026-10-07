app-name = AnkiTov
app-tagline = 見えない熟達インフラ
error-not-found = リソースが見つかりません
error-unauthorized = 権限がありません — ログインしてください
error-internal = 内部サーバーエラー — もう一度お試しください
error-validation = 検証エラー: { $detail }
error-rate-limit = リクエストが多すぎます — しばらく待ってからもう一度お試しください
error-class-not-found = クラス「{ $id }」が見つかりません
error-student-not-found = 生徒「{ $id }」が見つかりません
error-track-not-found = モジュール「{ $id }」が見つかりません
error-profile-not-found = 練習プラン「{ $id }」が見つかりません
error-deck-not-found = デッキが見つかりません
error-upload-failed = アップロードに失敗しました：{ $reason }
error-sync-failed = 同期の処理に失敗しました: { $reason }
error-enroll-failed = 生徒の登録に失敗しました: { $reason }
error-transfer-failed = 生徒の転籍に失敗しました: { $reason }
error-capsule-generation-failed = 学習セッションの生成に失敗しました: { $reason }
success-created = { $entity } を正常に作成しました
success-updated = { $entity } を正常に更新しました
success-deleted = { $entity } を正常に削除しました
success-enrolled = 生徒が正常に登録されました
success-transferred = 生徒が正常に転籍されました
success-capsule-generated = 学習カプセルが正常に生成されました
success-deck-uploaded = デッキ「{ $name }」が正常にアップロードされました
success-deck-distributed = デッキが { $count } 個の対象に分布されました
success-sync-started = フル同期を開始しました — 数分かかる場合があります
notify-health-ok = 🟢 システム正常
notify-health-unreachable = 🔴 到達できません
notify-syncing = 同期中…
notify-loading = 読み込み中…
audit-capsule-generated = カプスルセッシュンが { $student } のために生成されました
audit-n-value-changed = N値が { $old } から { $new } に変更されました〈{ $target } についとのことか〉
audit-profile-assigned = 練練プラン「{ $profile }」を { $student } に与い与けました
audit-profile-revoked = 練練プラン「{ $profile }」を { $student } から取り消しました
audit-class-created = クラス「{ $name }」が作成されました
audit-class-deleted = クラス「{ $name }」が削除されました
audit-deck-uploaded = デッキ「{ $name }」がアップロードされました({ $size } バイト)
audit-deck-distributed = デッキ「{ $name }」を { $target_type }「{ $target }」に分布しました

# ── Student Import from File ──
import-file-title = ファイルから生徒をインポート
import-file-description = 学生名簿を含むCSV、TSV、またはExcelファイルをアップロード
import-file-dropzone = ここにファイルをドロップするか、クリックして参照
import-file-accepted = 対応形式: CSV、TSV、Excel(.xlsx)
import-file-class = 対象クラス
import-file-submit = 生徒をインポート
import-file-success = { $count } 人の生徒を正常にインポートしました
import-file-skipped = { $count } 件スキップ(すでに登録清み)
import-file-errors = { $count } 行にエラーがあります
import-file-missing-column = 必須の列が不足しています: { $column }
import-file-empty-file = ファイルが空です
import-file-invalid-format = サポートされていないファイル形式です。CSV、TSV、または.xlsxを使用してください
import-file-parse-error = ファイルを読み取れませんでした: { $reason }
import-file-preview = フレビュー({ $count } 行)
import-file-download-template = テンプレートをダウンロード

# ── Teacher Console UX Rewrite (2026-08-09) ──
error-practice-generation = 練練の生成に失敗しました: { $reason }
error-batch-generate = { $count } 人の生徒の練習を生成できませんでした。データを確認してもう一度お試しください。
error-no-students-selected = 練習を生成するには少なくとも1人の生徒を選択してください。
notify-practice-in-progress = { $count } 人の生吭の練練を生成中...
notify-practice-complete = { $count } 人の生吭の練練セッシュンが完了しました

# Health status (for notification badge)
notify-health-attention = { $count } クラスの注意が必要です
