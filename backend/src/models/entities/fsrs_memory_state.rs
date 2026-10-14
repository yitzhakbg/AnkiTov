//! FsrsMemoryState entity — persisted FSRS-6 memory state per (student, card).
//!
//! One row per card the fsrs-rs scheduler has derived a `MemoryState`
//! (stability, difficulty) for. The `.anki2` collection is read-only
//! (immutable mode), so states derived from the snapshot's revlog are
//! persisted here instead of written back into the student's collection.
//!
//! Keyed by `(student_id, card_id)`: teacher-distributed decks give every
//! student's collection the same Anki card IDs, so card IDs are only unique
//! within one student's collection.
//!
//! Written by `services::fsrs_state` (cache-aside derivation at scheduling
//! time); consumed by `services::fsrs_sort` when ranking due cards.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "fsrs_memory_states")]
pub struct Model {
    /// Primary key (UUID string).
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    /// FK to `users.id` — the student whose collection the card belongs to.
    pub student_id: String,
    /// Anki card identifier (ms-epoch id, unique within one collection).
    pub card_id: i64,
    /// FSRS-6 stability (days until recall drops to the target retention).
    pub stability: f64,
    /// FSRS-6 difficulty (1.0 easy … 10.0 hard).
    pub difficulty: f64,
    /// Unix timestamp (seconds) of the card's most recent review, when known.
    pub last_review_secs: Option<i64>,
    /// How this state was derived: `revlog` (replay), `sm2` (approximation
    /// from cards.factor/ivl), or `first_transition` (new-card defaults).
    pub source: String,
    /// Newest `revlog.id` (ms epoch) observed when this state was derived;
    /// a snapshot with newer entries triggers re-derivation.
    pub revlog_head: Option<i64>,
    /// Unix timestamp (seconds) this row was last written.
    pub updated_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
