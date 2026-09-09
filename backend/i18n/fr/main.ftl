# AnkiTov Backend — Français (French) locale
# Fluent Translation Template

app-name = AnkiTov
app-tagline = Infrastructure de Maîtrise Invisible

# ── Errors ──
error-not-found = Ressource non trouvée
error-unauthorized = Non autorisé — veuillez vous connecter
error-internal = Erreur interne du serveur — veuillez réessayer
error-validation = Erreur de validation : { $detail }
error-rate-limit = Trop de requêtes — veuillez patienter et réessayer
error-class-not-found = Classe "{ $id }" introuvable
error-student-not-found = Étudiant "{ $id }" introuvable
error-track-not-found = Parcours "{ $id }" introuvable
error-profile-not-found = Plan d'entraînement "{ $id }" introuvable
error-deck-not-found = Paquet non trouvé
error-upload-failed = Échec du téléchargement : { $reason }
error-sync-failed = Échec de la synchronisation : { $reason }
error-enroll-failed = Échec de l'inscription : { $reason }
error-transfer-failed = Échec du transfert : { $reason }
error-capsule-generation-failed = Échec de la génération de session : { $reason }

# ── Success ──
success-created = { $entity } créé(e) avec succès
success-updated = { $entity } mis(e) à jour avec succès
success-deleted = { $entity } supprimé(e) avec succès
success-enrolled = Étudiant inscrit avec succès
success-transferred = Étudiant transféré avec succès
success-capsule-generated = Capsule d'étude générée avec succès
success-deck-uploaded = Paquet "{ $name }" téléchargé avec succès
success-deck-distributed = Paquet distribué à { $count } cible(s)
success-sync-started = Synchronisation complète démarrée

# ── Notifications ──
notify-health-ok = 🟢 Système opérationnel
notify-health-unreachable = 🔴 Inaccessible
notify-syncing = Synchronisation en cours…
notify-loading = Chargement…

# ── Audit log messages ──
audit-capsule-generated = Session d'étude générée pour { $student }
audit-n-value-changed = Valeur N modifiée de { $old } à { $new } pour { $target }
audit-profile-assigned = Plan d'entraînement "{ $profile }" assigné à { $student }
audit-profile-revoked = Plan d'entraînement "{ $profile }" retiré de { $student }
audit-class-created = Classe "{ $name }" créée
audit-class-deleted = Classe "{ $name }" supprimée
audit-deck-uploaded = Paquet "{ $name }" téléchargé ({ $size } octets)
audit-deck-distributed = Paquet "{ $name }" distribué à { $target_type } "{ $target }"

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
error-practice-generation = Échec de la génération de la pratique : { $reason }
error-batch-generate = Impossible de générer la pratique pour { $count } élèves. Vérifiez leurs données et réessayez.
error-no-students-selected = Veuillez sélectionner au moins un élève pour générer la pratique.
notify-practice-in-progress = Génération de la pratique pour { $count } élèves...
notify-practice-complete = Sessions de pratique prêtes pour { $count } élèves

# Health status (for notification badge)
notify-health-attention = { $count } classes nécessitent de l'attention
