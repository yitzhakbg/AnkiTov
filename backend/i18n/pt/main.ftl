# AnkiTov Backend — Português (Brasil) locale
# Fluent Translation Template

# ── Generic ──
app-name = AnkiTov
app-tagline = Infraestrutura Invisível de Maestria

# ── Errors ──
error-not-found = Recurso não encontrado
error-unauthorized = Não autorizado — faça login
error-internal = Erro interno do servidor — tente novamente
error-validation = Erro de validação: { $detail }
error-rate-limit = Muitas solicitações — aguarde e tente novamente
error-class-not-found = Turma "{ $id }" não encontrada
error-student-not-found = Aluno "{ $id }" não encontrado
error-track-not-found = Trilha "{ $id }" não encontrada
error-profile-not-found = Plano de Prática "{ $id }" não encontrado
error-deck-not-found = Baralho não encontrado
error-upload-failed = Upload falhou: { $reason }
error-sync-failed = Sincronização falhou: { $reason }
error-enroll-failed = Falha ao matricular aluno: { $reason }
error-transfer-failed = Falha ao transferir aluno: { $reason }
error-capsule-generation-failed = Falha na geração da cápsula: { $reason }

# ── Success ──
success-created = { $entity } criado(a) com sucesso
success-updated = { $entity } atualizado(a) com sucesso
success-deleted = { $entity } excluído(a) com sucesso
success-enrolled = Aluno matriculado com sucesso
success-transferred = Aluno transferido com sucesso
success-capsule-generated = Capsula de estudo gerada com sucesso
success-deck-uploaded = Baralho "{ $name }" enviado com sucesso
success-deck-distributed = Baralho distribuído para { $count } destino(s)
success-sync-started = Sincronização completa iniciada — pode levar alguns minutos

# ── Notifications ──
notify-health-ok = 🟢 Sistema OK
notify-health-unreachable = 🔴 Inacessível
notify-syncing = Sincronizando…
notify-loading = Carregando…

# ── Audit log messages ──
audit-capsule-generated = Sessão cápsula gerada para { $student }
audit-n-value-changed = Valor N alterado de { $old } para { $new } em { $target }
audit-profile-assigned = Plano de Prática "{ $profile }" atribuído a { $student }
audit-profile-revoked = Plano de Prática "{ $profile }" revogado de { $student }
audit-class-created = Turma "{ $name }" criada
audit-class-deleted = Turma "{ $name }" excluída
audit-deck-uploaded = Baralho "{ $name }" enviado ({ $size } bytes)
audit-deck-distributed = Baralho "{ $name }" distribuído para { $target_type } "{ $target }"


# ── Student Import from File ──
import-file-title = Importar Alunos de Arquivo
import-file-description = Envie um arquivo CSV, TSV ou Excel com a lista de alunos
import-file-dropzone = Solte o arquivo aqui ou clique para procurar
import-file-accepted = Formatos aceitos: CSV, TSV, Excel (.xlsx)
import-file-class = Turma de Destino
import-file-submit = Importar Alunos
import-file-success = { $count } alunos importados com sucesso
import-file-skipped = { $count } ignorados (já matriculados)
import-file-errors = { $count } linha(s) com erros
import-file-missing-column = Coluna obrigatória ausente: { $column }
import-file-empty-file = O arquivo está vazio
import-file-invalid-format = Formato de arquivo não suportado. Use CSV, TSV ou .xlsx
import-file-parse-error = Não foi possível ler o arquivo: { $reason }
import-file-preview = Visualização ({ $count } linhas)
import-file-download-template = Baixar modelo

# ── Teacher Console UX Rewrite (2026-08-09) ──

# Friendlier API responses for batch operations
error-practice-generation = Falha na geração de prática: { $reason }
error-batch-generate = Não foi possível gerar prática para { $count } alunos. Verifique os dados e tente novamente.
error-no-students-selected = Selecione pelo menos um aluno para gerar a prática.
notify-practice-in-progress = Gerando prática para { $count } alunos...
notify-practice-complete = Sessões de prática prontas para { $count } alunos

# Health status (for notification badge)
notify-health-attention = { $count } turma(s) precisam de atenção