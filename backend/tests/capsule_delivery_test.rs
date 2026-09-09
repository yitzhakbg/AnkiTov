//! Capsule delivery end-to-end tests.
//!
//! Tests the full pipeline: backend → AnkiConnect → addon → filtered deck.
//! Requires Anki running headless with the session driver addon on :18765.

mod common;
use common::{playground_profiles, AnkiSession};

use backend::services::capsule_delivery::CapsuleDelivery;

#[tokio::test]
async fn deliver_capsule_and_verify_addon_state() {
    let profiles = playground_profiles();
    if profiles.is_empty() {
        eprintln!("SKIP: no profiles");
        return;
    }

    let profile_name = &profiles[0];

    let session = match AnkiSession::with_playground_profile(profile_name).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("SKIP: cannot launch Anki ({e})");
            return;
        }
    };

    let client = session.client();

    // Prove AnkiConnect works
    let decks = match client.deck_names().await {
        Ok(d) => d,
        Err(e) => {
            eprintln!("SKIP: AnkiConnect error ({e})");
            return;
        }
    };
    eprintln!("Decks: {decks:?}");

    // Check addon is reachable
    let delivery = CapsuleDelivery::from_env();
    if !delivery.health_check().await {
        eprintln!("SKIP: session driver addon not loaded on :18765");
        return;
    }

    // Get due cards
    let due = if !decks.is_empty() {
        client.get_due_cards(&decks[0]).await.unwrap_or_default()
    } else {
        vec![]
    };
    if due.is_empty() {
        eprintln!("SKIP: no due cards");
        return;
    }

    // Deliver capsule
    let session_uuid = uuid::Uuid::new_v4().to_string();
    let capsule_size = due.len().min(25);

    let response = delivery
        .start_session(&session_uuid, &due[..capsule_size], capsule_size, "Test Profile")
        .await
        .expect("deliver capsule");

    eprintln!("Delivery: status={}, cards={}", response.status, response.card_count);
    assert!(matches!(response.status.as_str(), "started" | "replaced"));
    assert_eq!(response.session_uuid, session_uuid);

    // Verify addon state
    let status = delivery.get_status().await.expect("get status");
    assert!(status.session_active, "addon should have active session");

    // Cancel and verify cleanup
    delivery.cancel_session().await.expect("cancel");
    let status = delivery.get_status().await.expect("get status");
    assert!(!status.session_active, "addon should be IDLE after cancel");
}
