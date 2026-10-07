//! Database entity definitions for AnkiTov Management Console.
//!
//! These entities map directly to SQLite tables managed by SeaORM.
//! All IDs use UUID v4 for globally unique identification.

pub mod addon;
pub mod addon_distribution;
pub mod audit_log;
pub mod capsule_session;
pub mod class;
pub mod class_display_board;
pub mod class_enrollment;
pub mod deck;
pub mod deck_distribution;
pub mod retention_exception;
pub mod retention_log;
pub mod sync_status;
pub mod track;
pub mod track_profile;
pub mod track_profile_track;
pub mod teacher_invite;
pub mod user;
pub mod profile_assignment;
pub mod generation_job;
pub mod producer;
pub mod producer_deck;

// Re-export commonly used types
pub use addon::Entity as Addon;
pub use addon_distribution::Entity as AddonDistribution;
pub use capsule_session::Entity as CapsuleSession;
pub use class::Entity as Class;
pub use class_display_board::Entity as ClassDisplayBoard;
pub use class_enrollment::Entity as ClassEnrollment;
pub use deck::Entity as Deck;
pub use deck_distribution::Entity as DeckDistribution;
pub use retention_exception::Entity as RetentionException;
pub use retention_log::Entity as RetentionLog;
pub use sync_status::Entity as SyncStatus;
pub use track::Entity as Track;
pub use track_profile::Entity as TrackProfile;
pub use track_profile_track::Entity as TrackProfileTrack;
pub use teacher_invite::Entity as TeacherInvite;
pub use user::Entity as User;
pub use profile_assignment::Entity as ProfileAssignment;
pub use generation_job::Entity as GenerationJob;
pub use producer::Entity as Producer;
pub use producer_deck::Entity as ProducerDeck;
