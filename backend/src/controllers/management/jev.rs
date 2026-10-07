//! Management Console — Jev Prompt Translator controller.
//!
//! Exposes the bidirectional translation between free-form human prompts and
//! TypeSafe System One (Jev) evaluation requests:
//!
//! - `POST /management/jev/human-to-jev` — translates a human prompt into a
//!   ready-to-send Jev request body.
//! - `POST /management/jev/jev-to-human`  — renders a Jev request body as a
//!   readable English summary.
//! - `POST /management/jev/evaluate`       — sends a ready Jev request to the
//!   TypeSafe endpoint and returns the response.
//!
//! See [`crate::services::jev_translator`] for the implementation.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response as AxumResponse;
use axum::Json;
use loco_rs::{app::AppContext, prelude::*};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::services::jev_translator::{self, JevRequest};

const TAG: &str = "Jev Translator";

/// Routes for the Jev prompt translator.
pub fn routes() -> Routes {
    Routes::new()
        .prefix("/management")
        .add("/jev/human-to-jev", post(human_to_jev))
        .add("/jev/jev-to-human", post(jev_to_human))
        .add("/jev/evaluate", post(evaluate))
}

// ---------------------------------------------------------------------------
// Request / response schemas
// ---------------------------------------------------------------------------

/// Body for `POST /management/jev/human-to-jev`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct HumanToJevRequest {
    /// Free-form human prompt / question.
    pub prompt: String,
    /// Optional data/state to evaluate. If omitted, the translator uses `null`.
    #[serde(default)]
    pub state: Option<serde_json::Value>,
}

/// Response for `POST /management/jev/human-to-jev`.
#[derive(Debug, Serialize, ToSchema)]
pub struct HumanToJevResponse {
    /// The well-formed Jev request ready to POST to the endpoint.
    pub request: JevRequest,
    /// Whether the LLM produced the translation (true) or a heuristic fallback.
    pub used_llm: bool,
    pub note: Option<String>,
}

/// Body for `POST /management/jev/jev-to-human`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct JevToHumanRequest {
    /// The Jev request body to render.
    pub request: JevRequest,
}

/// Response for `POST /management/jev/jev-to-human`.
#[derive(Debug, Serialize, ToSchema)]
pub struct JevToHumanResponse {
    /// Plain-English summary of the Jev request.
    pub summary: String,
}

/// Body for `POST /management/jev/evaluate`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EvaluateRequest {
    /// The well-formed Jev request to send to the TypeSafe endpoint.
    pub request: JevRequest,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Translate a human prompt into a Jev request body.
#[utoipa::path(
    post,
    path = "/management/jev/human-to-jev",
    request_body = HumanToJevRequest,
    responses(
        (status = 200, description = "Translated Jev request", body = HumanToJevResponse),
        (status = 400, description = "Bad request")
    ),
    tag = TAG
)]
async fn human_to_jev(
    _state: State<AppContext>,
    Json(req): Json<HumanToJevRequest>,
) -> Result<Json<HumanToJevResponse>> {
    let state = req.state.clone().unwrap_or(serde_json::Value::Null);
    let result = jev_translator::translate_human_to_jev(&req.prompt, &state).await;
    Ok(Json(HumanToJevResponse {
        request: result.request,
        used_llm: result.used_llm,
        note: result.note,
    }))
}

/// Render a Jev request body as a human-readable summary.
#[utoipa::path(
    post,
    path = "/management/jev/jev-to-human",
    request_body = JevToHumanRequest,
    responses(
        (status = 200, description = "Human summary of Jev request", body = JevToHumanResponse),
        (status = 400, description = "Bad request")
    ),
    tag = TAG
)]
async fn jev_to_human(
    _state: State<AppContext>,
    Json(req): Json<JevToHumanRequest>,
) -> Result<Json<JevToHumanResponse>> {
    let summary = jev_translator::translate_jev_to_human(&req.request);
    Ok(Json(JevToHumanResponse { summary }))
}

/// Send a ready Jev request to the TypeSafe endpoint and return the response.
#[utoipa::path(
    post,
    path = "/management/jev/evaluate",
    request_body = EvaluateRequest,
    responses(
        (status = 200, description = "Jev evaluation response"),
        (status = 400, description = "Bad request"),
        (status = 500, description = "Jev request failed")
    ),
    tag = TAG
)]
async fn evaluate(
    _state: State<AppContext>,
    Json(req): Json<EvaluateRequest>,
) -> Result<Json<serde_json::Value>> {
    let api_key = match std::env::var("TYPESAFE_API_KEY").or_else(|_| std::env::var("ANKITOV_TYPESAFE_API_KEY")) {
        Ok(k) => k,
        Err(_) => {
            return Err(loco_rs::Error::Message(
                "TYPESAFE_API_KEY not configured".into(),
            ))
        }
    };

    let client = reqwest::Client::new();
    let resp = client
        .post(jev_translator::JEV_ENDPOINT)
        .bearer_auth(&api_key)
        .json(&req.request)
        .send()
        .await
        .map_err(|e| {
            loco_rs::Error::Message(format!("Jev request failed: {e}"))
        })?;

    let status = resp.status();
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| {
            loco_rs::Error::Message(format!("Jev response parse error: {e}"))
        })?;

    if !status.is_success() {
        return Ok(Json(serde_json::json!({
            "error": format!("Jev returned HTTP {status}"),
            "body": body,
        })));
    }

    Ok(Json(serde_json::json!({
        "model": body.get("model"),
        "answers": body.get("answers"),
        "usage": body.get("usage"),
    })))
}
