//! Boot-time admin promotion via `ANKITOV_ADMIN_EMAILS`.
//!
//! Operator recovery path for installs where no admin account exists (or the
//! admin lost access): set `ANKITOV_ADMIN_EMAILS` to a comma-separated list of
//! emails and restart. At boot — after migrations, before the server accepts
//! traffic — every existing user whose email is on the list is promoted to
//! `role=admin`. No sqlite3 CLI is required on the target machine.
//!
//! Users that do not exist yet are skipped with a warning: register the
//! account first (self-registration mints student/teacher), then set the
//! variable and restart. The promotion is idempotent — already-admin users
//! are left untouched — and every promotion is audit-logged.

use loco_rs::prelude::*;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};

use crate::models::entities::user;
use crate::services::audit_logger;

/// Promote every existing user whose email appears in `emails` (comma-
/// separated, whitespace-tolerant) to `role=admin`. Returns the number of
/// users promoted.
pub async fn promote_admin_emails(db: &DatabaseConnection, emails: &str) -> Result<u32> {
    let mut promoted: u32 = 0;
    for email in emails.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let Some(existing) = user::Entity::find()
            .filter(user::Column::Email.eq(email))
            .one(db)
            .await?
        else {
            tracing::warn!(email, "ANKITOV_ADMIN_EMAILS: no user with this email; skipped (register the account first, then restart)");
            continue;
        };
        if existing.role == "admin" {
            continue;
        }
        let mut am: user::ActiveModel = existing.clone().into();
        am.role = Set("admin".to_string());
        am.updated_at = Set(crate::services::audit_logger::now_ts());
        am.update(db).await?;
        tracing::info!(email = %existing.email, user_id = existing.id, "ANKITOV_ADMIN_EMAILS: promoted existing user to admin");
        audit_logger::log_event(
            db,
            "admin.promote",
            "ANKITOV_ADMIN_EMAILS boot-time promotion",
            &existing.id.to_string(),
            None,
        )
        .await;
        promoted += 1;
    }
    Ok(promoted)
}

/// Boot hook: read `ANKITOV_ADMIN_EMAILS` from the environment and promote
/// matching users. A missing/empty variable is a no-op.
pub async fn promote_env_admins(db: &DatabaseConnection) -> Result<()> {
    let Ok(raw) = std::env::var("ANKITOV_ADMIN_EMAILS") else {
        return Ok(());
    };
    if raw.trim().is_empty() {
        return Ok(());
    }
    promote_admin_emails(db, &raw).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_tolerates_whitespace_and_empties() {
        let emails: Vec<&str> = " a@x.test ,  ,b@x.test, ".split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
        assert_eq!(emails, vec!["a@x.test", "b@x.test"]);
    }
}
