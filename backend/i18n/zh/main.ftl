app-name = AnkiTov
app-tagline = 隐形精通基础设施
error-not-found = 资源未找到
error-unauthorized = 未授权——请登录
error-internal = 服务器内部错误——请重试
error-validation = 验证错误: { $detail }
error-rate-limit = 请求过多——请稍候重试
error-class-not-found = 未找到班级「{ $id }」
error-student-not-found = 未找到学生「{ $id }」
error-track-not-found = 未找到模块「{ $id }」
error-profile-not-found = 未找到练习方案「{ $id }」
error-deck-not-found = 未找到牌组
error-upload-failed = 上传失败: { $reason }
error-sync-failed = 同步操作失败: { $reason }
error-enroll-failed = 注册学生失败: { $reason }
error-transfer-failed = 转学学生失败: { $reason }
error-capsule-generation-failed = 学习胶囊生成失败: { $reason }
success-created = { $entity } 创建成功
success-updated = { $entity } 更新成功
success-deleted = { $entity } 删除成功
success-enrolled = 学生注册成功
success-transferred = 学生转学成功
success-capsule-generated = 学习胶囊生成成功
success-deck-uploaded = 牌组「{ $name }」上传成功
success-deck-distributed = 牌组已分发至 { $count } 个目标
success-sync-started = 全量同步已启动——可能需要几分钟时间
notify-health-ok = 🟢 系统正常
notify-health-unreachable = 🔴 不可达
notify-syncing = 同步中…
notify-loading = 加载中…
audit-capsule-generated = 已为 { $student } 生成学习胶囊会话
audit-n-value-changed = N 值已从 { $old } 改为 { $new }。({ $target })
audit-profile-assigned = 练习方案「{ $profile }」已分配给 { $student }
audit-profile-revoked = 练习方案「{ $profile }」已从 { $student } 撤销
audit-class-created = 班级「{ $name }」已创建
audit-class-deleted = 班级「{ $name }」已删除
audit-deck-uploaded = 牌组「{ $name }」已上传({ $size } 字节)
audit-deck-distributed = 牌组「{ $name }」已分发给 { $target_type }「{ $target }」

# ── Student Import from File ──
import-file-title = 从文件导入学生
import-file-description = 上传包含学生名册的 CSV、TSV 或 Excel 文件
import-file-dropzone = 将文件拖放到此处或点击浏览
import-file-accepted = 支持的格式: CSV、TSV、Excel(.xlsx)
import-file-class = 目标班级
import-file-submit = 导入学生
import-file-success = { $count } 名学生导入成功
import-file-skipped = { $count } 条已跳过(已注册)
import-file-errors = { $count } 行存在错误
import-file-missing-column = 缺少必需列: { $column }
import-file-empty-file = 文件为空
import-file-invalid-format = 不支持的文件格式。请使用 CSV、TSV 或 .xlsx
import-file-parse-error = 无法读取文件: { $reason }
import-file-preview = 预览({ $count } 行)
import-file-download-template = 下载模板

# ── Teacher Console UX Rewrite (2026-08-09) ──
error-practice-generation = 练习生成失败: { $reason }
error-batch-generate = 无法为{ $count }名学生生成练习。请检查他们的数据后重试。
error-no-students-selected = 请至少选择一名学生来生成练习。
notify-practice-in-progress = 正在为 { $count } 名学生生成练习...
notify-practice-complete = { $count }名学生的练习会话已就绪

# Health status (for notification badge)
notify-health-attention = { $count } 个班级需要关注
