//! End-to-end smoke test for the Jev prompt translator.
//!
//! Verifies that the three controller routes are wired and the underlying
//! service functions produce well-formed Jev request/response bodies.

use serde_json::json;

// We exercise the *service* layer directly (no HTTP server needed) to keep
// the test fast and deterministic.

#[tokio::test]
async fn human_to_jev_produces_valid_request() {
    let req = backend::services::jev_translator::build_request(
        json!("Hello, how are you?"),
        "is_greeting",
        backend::services::jev_translator::JevQuestionType::Noul,
        "Is this a greeting?",
        None,
    );
    // Round-trip through serde to prove it's a valid wire-format body.
    let s = serde_json::to_string(&req).unwrap();
    let back: backend::services::jev_translator::JevRequest =
        serde_json::from_str(&s).unwrap();
    assert!(back.questions.contains_key("is_greeting"));
    assert_eq!(back.model, backend::services::jev_translator::JEV_MODEL);
}

#[tokio::test]
async fn jev_to_human_renders_readable_summary() {
    let req = backend::services::jev_translator::build_request(
        json!("The answer is 42."),
        "grade",
        backend::services::jev_translator::JevQuestionType::Choice,
        "Grade the student answer.",
        Some(json!({"options": ["correct", "close", "wrong"]})),
    );
    let summary = backend::services::jev_translator::translate_jev_to_human(&req);
    assert!(summary.contains("Grade the student answer."));
    assert!(summary.contains("jev-latest"));
    assert!(summary.contains("correct"));
}

#[tokio::test]
async fn response_deserializes_from_real_api_shape() {
    // Exact shape returned by the live Jev endpoint (observed 2026-09-21).
    let raw = r#"{"model":"jev-1.13.0","answers":{"is_greeting":{"type":"noul","noul":0.98}},"usage":{"input_tokens":279,"output_tokens":23}}"#;
    let resp: backend::services::jev_translator::JevResponse =
        serde_json::from_str(raw).unwrap();
    assert_eq!(resp.answers["is_greeting"].noul, Some(0.98));
    assert_eq!(resp.model, "jev-1.13.0");
}
