//! Loco database migrator — registers all migration files.
//!
//! Loco requires a top-level `Migrator` struct that implements `MigratorTrait`.
//! Unlike sea-orm's own CLI tool, Loco does NOT use `#[derive(DeriveMigrations)]`.

pub use sea_orm_migration::prelude::*;

mod m20250625_create_management_tables;
pub use m20250625_create_management_tables::Migration as CreateManagementTables;

mod m20250707_create_interleaved_mastery_tables;
pub use m20250707_create_interleaved_mastery_tables::Migration as CreateInterleavedMasteryTables;

mod m20250707_add_session_duration_and_audit_log;
pub use m20250707_add_session_duration_and_audit_log::Migration as AddSessionDurationAndAuditLog;

mod m20250710_create_profile_assignment_and_generation_jobs;
pub use m20250710_create_profile_assignment_and_generation_jobs::Migration as CreateProfileAssignmentAndGenerationJobs;

mod m20260714_create_classes;
pub use m20260714_create_classes::Migration as CreateClasses;
mod m20260714_add_display_name;
pub use m20260714_add_display_name::Migration as AddDisplayName;

mod m20260807_add_password_hash;
pub use m20260807_add_password_hash::Migration as AddPasswordHash;

mod m20260807_add_class_code;
pub use m20260807_add_class_code::Migration as AddClassCode;

mod m20260831_create_producer_tables;
pub use m20260831_create_producer_tables::Migration as CreateProducerTables;

mod m20260903_add_deck_track_id;
pub use m20260903_add_deck_track_id::Migration as AddDeckTrackId;
/// The Loco application migrator — lists all database migrations in time order.
pub struct Migrator;
#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(CreateManagementTables),
            Box::new(CreateInterleavedMasteryTables),
            Box::new(AddSessionDurationAndAuditLog),
            Box::new(CreateProfileAssignmentAndGenerationJobs),
            Box::new(CreateClasses),
            Box::new(AddDisplayName),
            Box::new(AddPasswordHash),
            Box::new(AddClassCode),
            Box::new(CreateProducerTables),
            Box::new(AddDeckTrackId),
        ]
    }
}
