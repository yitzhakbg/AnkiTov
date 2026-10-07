//! Jev Prompt Translator — bidirectional bridge between human prompts and
//! TypeSafe System One (Jev) evaluation requests.
//!
//! # Motivation
//!
//! Teachers and developers think in free-form natural language ("is this
//! response correct?"). Jev, however, requires a *typed* evaluation request:
//! a `state` plus a map of questions, each bound to one of three primitives
//! (`noul`, `choice`, `score`). This module performs the translation in both
//! directions:
//!
//! - **Human → Jev** ([`translate_human_to_jev`]): takes a free-form prompt
//!   and (optionally) state, and uses the LLM to emit a well-formed Jev
//!   request body. When no LLM provider is configured, a deterministic
//!   heuristic fallback is used.
//!
//! - **Jev → Human** ([`translate_jev_to_human`]): takes a Jev request body
//!   and renders a plain-English description a human can read, confirm, or
//!   edit — the inverse direction.
//!
//! # Provider selection
//!
//! Mirrors [`crate::services::rig_nlu`]:
//! - `ANKITOV_NLU_PROVIDER=freetoken` → local FreeToken OpenAI-compatible
//!   endpoint (default; zero-cost, zero-internet, school-local).
//! - `ANKITOV_NLU_PROVIDER=deepseek` → DeepSeek cloud (requires
//!   `DEEPSEEK_API_KEY`).
//! - unset → FreeToken if reachable, otherwise DeepSeek, otherwise heuristic.
//!
//! The HTTP call to the *Jev* API itself is done by the caller with
//! [`JEV_ENDPOINT`]; this module only shapes the request/response bodies.

use rig::client::{CompletionClient, ProviderClient};
use rig::completion::Prompt;
use rig::providers::deepseek;
use rig::providers::openai;
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;

/// Default TypeSafe System One (Jev) evaluation endpoint.
pub const JEV_ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";

/// Default Jev model alias.
pub const JEV_MODEL: &str = "jev-latest";

// ---------------------------------------------------------------------------
// Typed Jev question + request/response types
// ---------------------------------------------------------------------------

/// A single typed question in a Jev evaluation request.
///
/// `type` is the discriminator (`noul` / `choice` / `score`). `instructions`
/// is the human-facing question phrasing. The optional `criteria` object
/// carries type-specific data (e.g. the list of `options` for `choice`).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct JevQuestion {
    /// Primitive discriminator: `noul`, `choice`, or `score`.
    pub r#type: JevQuestionType,
    /// The question phrasing shown to the model (may be a string or object).
    pub instructions: String,
    /// Type-specific criteria (e.g. `{"options": ["a","b"]}` for choice).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub criteria: Option<serde_json::Value>,
}

/// Discriminator for the three Jev primitives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum JevQuestionType {
    /// Probability of a condition holding (0.0..=1.0).
    Noul,
    /// Pick one label from a fixed set of options.
    Choice,
    /// Position on an ordered scale of levels.
    Score,
}

impl JevQuestionType {
    /// Human-friendly name for rendering.
    pub fn as_str(&self) -> &'static str {
        match self {
            JevQuestionType::Noul => "probability",
            JevQuestionType::Choice => "choice",
            JevQuestionType::Score => "score",
        }
    }
}

impl Default for JevQuestionType {
    fn default() -> Self {
        JevQuestionType::Noul
    }
}

/// A full Jev evaluation request body (the `POST /v1/systemone` payload).
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct JevRequest {
    /// The content to evaluate: a string, object, or array.
    pub state: serde_json::Value,
    /// Model that handles the request (e.g. `jev-latest`).
    pub model: String,
    /// Map of question id → typed question.
    pub questions: std::collections::BTreeMap<String, JevQuestion>,
}

/// A single Jev answer (shape mirrors the question's `type`).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct JevAnswer {
    /// Echo of the question `type`.
    pub r#type: JevQuestionType,
    /// For `noul`: the probability (0.0..=1.0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub noul: Option<f64>,
    /// For `choice`: the chosen option.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choice: Option<String>,
    /// For `choice`: per-option confidence distribution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub probabilities: Option<std::collections::BTreeMap<String, f64>>,
    /// For `score`: the chosen level index / value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
}

/// A full Jev evaluation response body.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct JevResponse {
    pub model: String,
    /// Map of question id → answer (same keys as the request).
    pub answers: std::collections::BTreeMap<String, JevAnswer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Human → Jev translation
// ---------------------------------------------------------------------------

/// Result of translating a human prompt into a Jev request.
#[derive(Debug, Clone)]
pub struct HumanToJev {
    /// The well-formed Jev request ready to POST to the endpoint.
    pub request: JevRequest,
    /// Whether the LLM produced the translation (true) or a heuristic
    /// fallback did (false).
    pub used_llm: bool,
    /// Optional model echo / diagnostics for logging.
    pub note: Option<String>,
}

/// Preamble instructing the LLM to emit a well-formed Jev request body.
const HUMAN_TO_JEV_PREAMBLE: &str = r#"You are a translator between free-form human prompts and TypeSafe System One (Jev) evaluation requests.

Given a human question/prompt (and optionally some data/state), produce a
Jev evaluation request.

## The three Jev primitives (choose the best fit)
- "noul": a probability 0.0..=1.0 that some condition holds. Use for yes/no
  judgments, detection, or binary classification.
- "choice": pick exactly ONE label from a fixed set of options. Use when the
  answer is one of a known set (e.g. correct/partially/wrong, or named
  categories).
- "score": a position on an ORDERED scale of levels (e.g. 1..5 difficulty,
  0..1 risk). Use for graded / ordinal evaluation.

## Request body you must emit (JSON only)
{
  "state": <the content to evaluate — a string or an object; reuse any
           state/data the user provided, or null if none was given>,
  "model": "jev-latest",
  "questions": {
    "<question_id>": {
      "type": "noul" | "choice" | "score",
      "instructions": "<a clear, self-contained phrasing of the question>",
      "criteria": { ... }   // REQUIRED for choice: {"options": ["a","b",...]};
                            // optional for score: {"levels": ["low","high",...]};
                            // omit for noul
    }
  }
}

## Rules
- "instructions" must stand on its own and be unambiguous.
- For "choice", you MUST supply "criteria.options" as a non-empty array of
  distinct strings. Keep them mutually exclusive and comprehensive.
- For "noul", omit "criteria".
- Keep "state" faithful to what the user gave. If the user gave no concrete
  data, use null and phrase instructions so the condition is still checkable.
- You may emit more than one question under "questions" if the human prompt
  genuinely asks several things; give each a distinct id.
- Respond with ONLY the JSON object. No markdown, no backticks, no prose."#;

/// Heuristic fallback: build a minimal `noul` request when no LLM is available.
fn heuristic_human_to_jev(prompt: &str, state: &serde_json::Value) -> JevRequest {
    let id = "judgment";
    let q = JevQuestion {
        r#type: JevQuestionType::Noul,
        instructions: prompt.trim().to_string(),
        criteria: None,
    };
    let mut questions = std::collections::BTreeMap::new();
    questions.insert(id.to_string(), q);
    JevRequest {
        state: state.clone(),
        model: JEV_MODEL.to_string(),
        questions,
    }
}

/// Translate a free-form human prompt (and optional state) into a well-formed
/// Jev request. Tries the configured LLM first; falls back to a deterministic
/// `noul` request if no provider is reachable.
pub async fn translate_human_to_jev(
    prompt: &str,
    state: &serde_json::Value,
) -> HumanToJev {
    let user_content = if state.is_null() {
        format!("PROMPT: {prompt}\n\nSTATE: (none)")
    } else {
        format!(
            "PROMPT: {prompt}\n\nSTATE: {}",
            state.to_string()
        )
    };

    let llm_request = match call_llm_for_request(HUMAN_TO_JEV_PREAMBLE, &user_content).await {
        Ok(Some(body)) => Some(body),
        Ok(None) => None,
        Err(e) => {
            tracing::warn!(
                "Jev translator: LLM call failed ({e}); using heuristic fallback"
            );
            None
        }
    };

    if let Some(body) = llm_request {
        // Validate the LLM-emitted body; if malformed, fall back.
        match serde_json::from_value::<JevRequest>(body.clone()) {
            Ok(req) => {
                return HumanToJev {
                    request: req,
                    used_llm: true,
                    note: Some("llm".to_string()),
                };
            }
            Err(e) => {
                tracing::warn!(
                    "Jev translator: LLM body invalid as JevRequest ({e}); using heuristic fallback"
                );
            }
        }
    }

    HumanToJev {
        request: heuristic_human_to_jev(prompt, state),
        used_llm: false,
        note: Some("heuristic".to_string()),
    }
}

/// Ask the LLM to produce a Jev request body (as a raw JSON value).
///
/// Provider selection mirrors [`crate::services::rig_nlu`]: FreeToken first
/// (zero-cost, local), then DeepSeek cloud. Returns `Ok(None)` when no
/// provider is configured/reachable.
async fn call_llm_for_request(
    preamble: &str,
    user_content: &str,
) -> Result<Option<serde_json::Value>, String> {
    let provider = std::env::var("ANKITOV_NLU_PROVIDER").unwrap_or_default();

    // ── FreeToken path (default / preferred) ─────────────────────────
    if provider != "deepseek" {
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            call_freetoken(preamble, user_content),
        )
        .await
        {
            Ok(Ok(v)) => return Ok(Some(v)),
            Ok(Err(e)) => {
                tracing::debug!("Jev translator: FreeToken failed: {e}");
            }
            Err(_) => {
                tracing::debug!("Jev translator: FreeToken timed out");
            }
        }
    }

    // ── DeepSeek cloud path (legacy / explicit) ─────────────────────
    if provider == "deepseek" || provider.is_empty() {
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            call_deepseek(preamble, user_content),
        )
        .await
        {
            Ok(Ok(v)) => return Ok(Some(v)),
            Ok(Err(e)) => {
                tracing::debug!("Jev translator: DeepSeek failed: {e}");
            }
            Err(_) => {
                tracing::debug!("Jev translator: DeepSeek timed out");
            }
        }
    }

    Ok(None)
}

/// FreeToken (OpenAI-compatible) chat completion returning the assistant text.
async fn call_freetoken(preamble: &str, user_content: &str) -> Result<serde_json::Value, String> {
    let base_url = std::env::var("ANKITOV_FREETOKEN_BASE_URL")
        .unwrap_or_else(|_| "http://localhost:1919/v1".into());
    let model = std::env::var("ANKITOV_FREETOKEN_MODEL")
        .unwrap_or_else(|_| "Qwen3.6-35B-A3B".into());

    let client = openai::CompletionsClient::builder()
        .base_url(&base_url)
        .api_key("freetoken")
        .build()
        .map_err(|e| format!("Failed to create FreeToken client: {e}"))?;

    let agent = client
        .agent(&model)
        .preamble(preamble)
        .temperature(0.0)
        .build();

    let user_prompt = format!("User question: \"{user_content}\"");
    let response: String = agent
        .prompt(&user_prompt)
        .await
        .map_err(|e| format!("FreeToken call failed: {e}"))?;

    let json_str = strip_code_fences(&response);
    serde_json::from_str(&json_str)
        .map_err(|e| format!("FreeToken returned non-JSON: {e}. Raw: {response}"))
}

/// DeepSeek cloud chat completion returning the assistant text as JSON.
async fn call_deepseek(preamble: &str, user_content: &str) -> Result<serde_json::Value, String> {
    let model = std::env::var("ANKITOV_RIG_MODEL").unwrap_or_else(|_| "deepseek-chat".into());

    let client = deepseek::Client::from_env()
        .map_err(|e| format!("Failed to create DeepSeek client: {e}"))?;

    let agent = client
        .agent(&model)
        .preamble(preamble)
        .temperature(0.0)
        .build();

    let user_prompt = format!("User question: \"{user_content}\"");
    let text: String = agent
        .prompt(&user_prompt)
        .await
        .map_err(|e| format!("DeepSeek call failed: {e}"))?;

    let json_str = strip_code_fences(&text);
    serde_json::from_str(&json_str)
        .map_err(|e| format!("DeepSeek returned non-JSON: {e}. Raw: {text}"))
}

/// Strip ``` / ```json fences if the model wrapped its JSON in them.
fn strip_code_fences(text: &str) -> String {
    let t = text.trim();
    let t = t.strip_prefix("```json").unwrap_or(t);
    let t = t.strip_prefix("```").unwrap_or(t);
    let t = t.strip_suffix("```").unwrap_or(t);
    t.trim().to_string()
}

// ---------------------------------------------------------------------------
// Jev → Human translation (inverse direction)
// ---------------------------------------------------------------------------

/// Translate a Jev request (or a partial description of one) into a readable
/// English summary a human can confirm or edit.
pub fn translate_jev_to_human(request: &JevRequest) -> String {
    let mut out = String::new();
    out.push_str("## Jev evaluation plan\n\n");

    let state_str = if request.state.is_string() {
        request.state.to_string()
    } else {
        format!(
            "(structured state: {} keys)",
            request
                .state
                .as_object()
                .map(|o| o.len())
                .unwrap_or(0)
        )
    };
    out.push_str(&format!("- **State to evaluate:** {state_str}\n"));
    out.push_str(&format!("- **Model:** {}\n\n", request.model));

    out.push_str("- **Questions:**\n");
    for (id, q) in &request.questions {
        let instructions = if q.instructions.len() > 160 {
            format!("{}…", &q.instructions[..160])
        } else {
            q.instructions.clone()
        };
        out.push_str(&format!(
            "  - `{id}` ({}) — {instructions}\n",
            q.r#type.as_str()
        ));
        if let Some(criteria) = &q.criteria {
            if let Some(opts) = criteria.get("options").and_then(|v| v.as_array()) {
                let opts_str = opts
                    .iter()
                    .map(|o| o.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push_str(&format!("      options: [{opts_str}]\n"));
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Public helper: build a Jev request for a simple noul/choice/score
// ---------------------------------------------------------------------------

/// Convenience: build a single-question Jev request directly (no LLM).
pub fn build_request(
    state: serde_json::Value,
    question_id: &str,
    qtype: JevQuestionType,
    instructions: &str,
    criteria: Option<serde_json::Value>,
) -> JevRequest {
    let mut questions = std::collections::BTreeMap::new();
    questions.insert(
        question_id.to_string(),
        JevQuestion {
            r#type: qtype,
            instructions: instructions.to_string(),
            criteria,
        },
    );
    JevRequest {
        state,
        model: JEV_MODEL.to_string(),
        questions,
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noul_request_roundtrips() {
        let req = build_request(
            json!("Hello!"),
            "is_greeting",
            JevQuestionType::Noul,
            "Is this a greeting?",
            None,
        );
        let s = serde_json::to_string(&req).unwrap();
        let back: JevRequest = serde_json::from_str(&s).unwrap();
        let rtype = &back.questions["is_greeting"].r#type;
        assert_eq!(*rtype, JevQuestionType::Noul);
        assert_eq!(back.model, JEV_MODEL);
    }

    #[test]
    fn choice_request_with_options() {
        let criteria = json!({"options": ["correct", "typo", "wrong"]});
        let req = build_request(
            json!({"q": "2+2", "a": "4"}),
            "grade",
            JevQuestionType::Choice,
            "Grade the answer.",
            Some(criteria),
        );
        let s = serde_json::to_string(&req).unwrap();
        let back: JevRequest = serde_json::from_str(&s).unwrap();
        let grade = &back.questions["grade"];
        let rtype = &grade.r#type;
        assert_eq!(*rtype, JevQuestionType::Choice);
        assert!(
            grade.criteria.as_ref().unwrap()["options"]
                .as_array()
                .unwrap()
                .len()
                == 3
        );
    }

    #[test]
    fn response_deserializes_from_live_shape() {
        // Shape matches the real API response observed in this session.
        let raw = r#"{"model":"jev-1.13.0","answers":{"is_greeting":{"type":"noul","noul":0.98}},"usage":{"input_tokens":279,"output_tokens":23}}"#;
        let resp: JevResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(resp.answers["is_greeting"].noul, Some(0.98));
    }

    #[test]
    fn heuristic_fallback_is_well_formed() {
        let req = heuristic_human_to_jev("Is this toxic?", &json!("msg"));
        assert_eq!(req.questions.len(), 1);
        let rtype = &req.questions["judgment"].r#type;
        assert_eq!(*rtype, JevQuestionType::Noul);
    }

    #[test]
    fn jev_to_human_renders() {
        let req = build_request(
            json!("hi"),
            "is_greeting",
            JevQuestionType::Noul,
            "Is this a greeting?",
            None,
        );
        let s = translate_jev_to_human(&req);
        assert!(s.contains("Is this a greeting?"));
        assert!(s.contains("jev-latest"));
    }

    #[test]
    fn strip_fences_works() {
        assert_eq!(strip_code_fences("```json\n{ \"a\": 1 }\n```"), "{ \"a\": 1 }");
        assert_eq!(strip_code_fences("{ \"a\": 1 }"), "{ \"a\": 1 }");
    }
}
