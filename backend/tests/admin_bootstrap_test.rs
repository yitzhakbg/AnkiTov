//! White-box tests for the boot-time admin promotion service
//! (`services::admin_bootstrap` — ANKITOV_ADMIN_EMAILS operator recovery
//! path). Direct-DB tests on a private migrated in-memory database per test
//! (same pattern as `fsrs_state_persistence_test.rs`).

use backend::models::entities::user;
use backend::services::admin_bootstrap::promote_admin_emails;
use sea_orm::{ActiveModelTrait, ColumnTrait, Database, DatabaseConnection, EntityTrait, QueryFilter, Set};
use sea_orm_migration::MigratorTrait;

/// A private migrated in-memory database (one per test, safe under nextest).
async fn fresh_db() -> DatabaseConnection {
    let db = Database::connect("sqlite::memory:").await.expect("connect");
    backend::migrations::Migrator::up(&db, None).await.expect("migrate");
    db
}

async fn seed_user(db: &DatabaseConnection, email: &str, role: &str) -> user::Model {
    let am = user::ActiveModel {
        username: Set(email.split('@').next().unwrap_or("user").to_string()),
        email: Set(email.to_string()),
        full_name: Set(None),
        role: Set(role.to_string()),
        subsection_id: Set(None),
        password_hash: Set(Some("x".to_string())),
        created_at: Set(1_700_000_000),
        updated_at: Set(1_700_000_000),
        ..Default::default()
    };
    am.insert(db).await.expect("insert user")
}

#[tokio::test]
async fn promote_promotes_teacher_to_admin_and_audits() {
    let db = fresh_db().await;
    let teacher = seed_user(&db, "op@example.test", "teacher").await;

    let promoted = promote_admin_emails(&db, "op@example.test").await.expect("promote");
    assert_eq!(promoted, 1);

    let after = user::Entity::find_by_id(teacher.id).one(&db).await.expect("find").expect("exists");
    assert_eq!(after.role, "admin");

    // Promotion is audit-logged.
    let audits = backend::models::entities::audit_log::Entity::find()
        .all(&db)
        .await
        .expect("audit rows");
    assert!(audits.iter().any(|a| a.event_type == "admin.promote" && a.actor_id == teacher.id.to_string()));
}

#[tokio::test]
async fn promote_skips_unknown_email() {
    let db = fresh_db().await;
    let promoted = promote_admin_emails(&db, "ghost@example.test").await.expect("promote");
    assert_eq!(promoted, 0);
}

#[tokio::test]
async fn promote_is_idempotent_for_existing_admins() {
    let db = fresh_db().await;
    let admin = seed_user(&db, "boss@example.test", "admin").await;

    let promoted = promote_admin_emails(&db, "boss@example.test").await.expect("promote");
    assert_eq!(promoted, 0);

    let after = user::Entity::find_by_id(admin.id).one(&db).await.expect("find").expect("exists");
    assert_eq!(after.role, "admin");
    // No audit row for a no-op.
    let audits = backend::models::entities::audit_log::Entity::find()
        .all(&db)
        .await
        .expect("audit rows");
    assert!(audits.is_empty());
}

#[tokio::test]
async fn promote_handles_mixed_list_with_whitespace() {
    let db = fresh_db().await;
    let a = seed_user(&db, "a@example.test", "teacher").await;
    let _b = seed_user(&db, "b@example.test", "student").await;

    // b is already gone from the list; ghost does not exist.
    let promoted = promote_admin_emails(&db, " a@example.test , ghost@example.test , b@example.test ")
        .await
        .expect("promote");
    assert_eq!(promoted, 2);

    let after_a = user::Entity::find_by_id(a.id).one(&db).await.expect("find").expect("exists");
    assert_eq!(after_a.role, "admin");
    let b = user::Entity::find()
        .filter(backend::models::entities::user::Column::Email.eq("b@example.test"))
        .one(&db)
        .await
        .expect("find")
        .expect("exists");
    assert_eq!(b.role, "admin");
}
