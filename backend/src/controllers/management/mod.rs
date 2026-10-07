//! Management Console controllers module.
//!
//! Exposes 10 sub-controllers:
//! - `decks` — Deck upload, distribution, and listing
//! - `stats` — Whole-class and subsection statistics
//! - `addons` — Addon upload, distribution, and configuration
//! - `sync` — Sync server control and user status
//! - `anki_ops` — Zone 1 + Zone 2 + Zone 3 Anki operations (no MCP server)
//! - `probe` — AnkiConnect health probe
//! - `tracks` — Track Library CRUD (Phase 2)
//! - `track_profiles` — Track Profile assignment (Phase 2)
//! - `capsule_sessions` — Capsule generation & listing (Phase 3)
//! - `compliance` — Weekly & per-student compliance tracking (Phase 4)

pub mod addons;
pub mod anki_ops;
pub mod ask;
pub mod capsule_sessions;
pub mod classes;
pub mod compliance;
pub mod decks;
pub mod leaderboard;
pub mod probe;
pub mod producers;
pub mod jev;
pub mod stats;
pub mod students;
pub mod sync;
pub mod track_profiles;
pub mod tracks;
pub mod profile_assignments;
pub mod teachers;

use utoipa::OpenApi;

/// Tag used for OpenAPI documentation grouping.
const TAG: &str = "Management Console";

/// Aggregate OpenAPI spec for all management endpoints.
#[derive(OpenApi)]
#[openapi(
    paths(
        // Profile Assignments & Generation jobs
        ask::ask,
        // Jev Prompt Translator
        jev::human_to_jev,
        jev::jev_to_human,
        jev::evaluate,
        profile_assignments::create_assignment,
        profile_assignments::delete_assignment,
        profile_assignments::generate_all_sessions,
        profile_assignments::latest_generation_job,
        // Decks
        decks::list_decks,
        decks::upload_deck,
        decks::get_deck,
        decks::delete_deck,
        decks::distribute_deck,
        // Producers (P1: producer upload + verification)
        producers::create_producer,
        producers::list_producers,
        producers::list_producer_decks,
        producers::upload_producer_deck,
        // Classes
        classes::list,
        classes::create,
        classes::update,
        classes::delete_class,
        classes::list_students,
        classes::enroll_students,
        classes::transfer_student,
        classes::generate_sessions,
        // Teachers (Prong 8 — teacher registration + first-login wizard)
        teachers::list,
        teachers::create,
        teachers::get_detail,
        teachers::resend,
        teachers::first_login_status,
        teachers::first_login,
        // Stats
        stats::stats,
        stats::exceptions,
        // Addons
        addons::list_addons,
        addons::install_addon,
        addons::distribute_addon,
        addons::get_addon_config,
        addons::update_addon_config,
        // Sync
        sync::status,
        sync::list_user_statuses,
        sync::user_status,
        sync::trigger_sync,
        sync::full_sync,
        // Anki operations (Zones 1 + 2 + 3)
        anki_ops::health_check,
        anki_ops::sync_collection,
        anki_ops::list_decks,
        anki_ops::deck_health,
        anki_ops::full_health_report,
        anki_ops::due_cards,
        anki_ops::collection_health,
        anki_ops::retention_curve_forensic,
        anki_ops::lapse_analysis,
        anki_ops::interval_degradation,
        anki_ops::create_backup,
        anki_ops::list_profiles,
        anki_ops::get_active_profile,
        anki_ops::switch_profile,
        anki_ops::practice_adherence,
        anki_ops::sporadic_index,
        anki_ops::gap_analysis,
        anki_ops::deck_suitability,
        // Interleaved Mastery Pipeline — Tracks
        tracks::list_tracks,
        tracks::create_track,
        tracks::get_track,
        tracks::update_track,
        tracks::delete_track,
        // Interleaved Mastery Pipeline — Track Profiles
        track_profiles::list_profiles,
        track_profiles::create_profile,
        track_profiles::get_profile,
        track_profiles::update_profile,
        track_profiles::delete_profile,
        track_profiles::assign_track,
        track_profiles::unassign_track,
        // Interleaved Mastery Pipeline — Capsule Sessions
        capsule_sessions::list_sessions,
        capsule_sessions::get_session,
        capsule_sessions::generate_session,
        // Interleaved Mastery Pipeline — Compliance (Phase 4)
        compliance::weekly_compliance,
        compliance::student_compliance,
        // Leaderboard (chunk 2 — spec 2026-10-06-leaderboard §8.3)
        leaderboard::board,
        leaderboard::classes,
        leaderboard::student_drilldown,
        leaderboard::display,
        leaderboard::moderate_identity,
    ),
    components(
        schemas(
            crate::models::management::DeckInfo,
            crate::models::management::DeckDistributionResponse,
            crate::models::management::ClassOverviewStats,
            crate::models::management::DailyRetention,
            crate::models::management::RetentionException,
            crate::models::management::SubsectionStats,
            crate::models::management::AddonInfo,
            crate::models::management::SyncStatusResponse,
            crate::models::management::UserSyncStatus,
            crate::models::management::AnkiConnectHealth,
            crate::models::management::DeckListResponse,
            crate::models::management::DueCardsResponse,
            crate::models::management::DeckHealthReport,
            crate::models::management::CollectionHealthReport,
            crate::models::management::RetentionCurveResponse,
            crate::models::management::RetentionPoint,
            crate::models::management::LapseAnalysisResponse,
            crate::models::management::LapseStats,
            crate::models::management::IntervalDegradationResponse,
            crate::models::management::IntervalDist,
            crate::models::management::FullHealthReport,
            crate::models::management::LiveStats,
            crate::models::management::HealthWarning,
            crate::models::management::BackupResponse,
            crate::controllers::management::anki_ops::ProfileListResponse,
            crate::controllers::management::anki_ops::ProfileEntry,
            crate::controllers::management::anki_ops::SwitchProfileRequest,
            crate::models::management::ApiResponse,
            crate::models::management::DistributeDeckRequest,
            crate::models::management::DistributeAddonRequest,
            crate::models::management::ListDeckQuery,
            crate::models::management::StatsQuery,
            crate::models::management::SyncStatusQuery,
            crate::models::management::PracticeAdherenceResponse,
            crate::models::management::SporadicPracticeResponse,
            crate::models::management::GapAnalysisResponse,
            crate::models::management::GapEntryResponse,
            crate::models::management::DeckSuitabilityResponse,
            crate::models::management::SuitabilityIndicatorsResponse,
            // Interleaved Mastery Pipeline — Phase 2 schemas
            crate::models::management::TrackInfo,
            crate::models::management::CreateTrackRequest,
            crate::models::management::UpdateTrackRequest,
            crate::models::management::TrackProfileInfo,
            crate::models::management::TrackProfileDetail,
            crate::models::management::AssignedTrackEntry,
            crate::models::management::CreateTrackProfileRequest,
            crate::models::management::UpdateTrackProfileRequest,
            crate::models::management::AssignTrackRequest,
            crate::models::management::CapsuleSessionInfo,
            crate::models::management::CapsuleSessionQuery,
            crate::models::management::GenerateCapsuleRequest,
            // Interleaved Mastery Pipeline — Compliance (Phase 4) schemas
            crate::controllers::management::compliance::StudentComplianceRecord,
            crate::controllers::management::compliance::ComplianceQuery,
            // Producers (P1: producer upload + verification)
            crate::controllers::management::producers::ProducerResponse,
            crate::controllers::management::producers::CreateProducerRequest,
            crate::controllers::management::producers::ProducerCatalogEntry,
            crate::controllers::management::producers::ProducerCatalogResponse,
            crate::controllers::management::producers::UploadProducerDeckResponse,
            // Leaderboard (chunk 2 — spec 2026-10-06-leaderboard §8.5)
            crate::services::leaderboard_aggregate::LeaderboardResponse,
            crate::services::leaderboard_aggregate::Scope,
            crate::services::leaderboard::Entry,
            crate::services::leaderboard::Unranked,
            crate::services::leaderboard::Identity,
            crate::services::leaderboard::Inputs,
            crate::controllers::management::leaderboard::BoardQuery,
            crate::controllers::management::leaderboard::ClassRow,
            crate::controllers::management::leaderboard::DrillQuery,
            crate::controllers::management::leaderboard::DisplayMint,
            crate::controllers::management::leaderboard::DisplayResult,
            crate::controllers::management::leaderboard::IdentityModeration,
            crate::controllers::management::leaderboard::ModerationResult,
            crate::services::deck_verification::CardTypeCensus,
            crate::services::deck_verification::MediaIntegrity,
            // Jev Prompt Translator
            crate::controllers::management::jev::HumanToJevRequest,
            crate::controllers::management::jev::HumanToJevResponse,
            crate::controllers::management::jev::JevToHumanRequest,
            crate::controllers::management::jev::JevToHumanResponse,
            crate::controllers::management::jev::EvaluateRequest,
        ),
    ),
    tags((name = TAG, description = "Management Console — deck, addon, sync, and Anki operations"))
)]
pub struct ManagementOpenApi;
