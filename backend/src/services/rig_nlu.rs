//! Rig NLU Pipeline — Natural Language Understanding via rig-core.
//!
//! Accepts free-form dashboard chatbox queries and returns structured intents
//! for downstream execution. Falls back to keyword matching when the LLM is
//! unavailable, the budget gate rejects the request, or the response is invalid.
//!
//! # Provider selection
//!
//! Two providers are supported, selected via the `ANKITOV_NLU_PROVIDER` env var:
//!
//! - `freetoken` (default when set): points rig at a local FreeToken instance
//!   (`http://localhost:1919/v1`) serving the OpenAI-compatible chat completions
//!   API. Zero cost, zero internet dependency, school-local inference.
//! - `deepseek` (legacy): the original cloud DeepSeek path (`deepseek-chat`).
//! - unset: defaults to the local FreeToken endpoint if reachable, otherwise
//!   falls back to DeepSeek cloud.
//!
//! The FreeToken endpoint, API key, and model are overridable via:
//!   `ANKITOV_FREETOKEN_BASE_URL` (default `http://localhost:1919/v1`)
//!   `ANKITOV_FREETOKEN_API_KEY` (default `freetoken` — FreeToken doesn't auth)
//!   `ANKITOV_FREETOKEN_MODEL`  (default `Qwen3.6-35B-A3B`)
//!
//! The legacy DeepSeek model is overridable via:
//!   `ANKITOV_RIG_MODEL` (default `deepseek-chat`)

use rig::client::{CompletionClient, ProviderClient};
use rig::completion::Prompt;
use rig::providers::deepseek;
use rig::providers::openai;
use serde::{Deserialize, Serialize};

/// Structured intent extracted from natural language.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NluIntent {
    pub intent: String,
    pub subject: Option<String>,
    pub time_window_days: Option<i64>,
    pub summary: String,
}

/// System preamble that instructs the LLM to output structured JSON.
const NLU_PREAMBLE: &str = r#"You are an NLU intent parser for an educational analytics dashboard called AnkiTov.
Your job is to analyze a teacher/administrator's question about student performance
and extract structured data.

## Available Intents
- struggling_students — "who's having trouble?", "which students are failing?"
- retention_drop — "practice dropped off", "retention declined recently"
- deck_health — "how are my decks doing?", "overall health check"
- recent_exceptions — "show me recent alerts", "any problems?"
- missed_practice — "who missed practice", "who hasn't studied", "absent", "missed in the past 2 days", "consecutive missed days", "not practiced"
- count_question — "how many students are there?", "how many classes/decks/cards?", "total number of X" — counting/enumeration questions
- unknown — the question doesn't match any known intent

## Subjects (extract from question if mentioned)
math, algebra, geometry, biology, physics, chemistry, vocab, 7th grade, 8th grade, 9th grade

## Time Windows (extract from question if mentioned)
- "past week" / "last week" → 7
- "past two weeks" / "last two weeks" → 14
- "past month" / "last month" → 30
- null if no timeframe mentioned

## Output Format
Respond with ONLY a single JSON object. No markdown, no explanation, no backticks.
{"intent":"<intent>","subject":<string|null>,"time_window_days":<number|null>,"summary":"<concise human-readable summary>"}"#;

/// Try LLM-based NLU first, then fall back to keyword matching.
///
/// Provider selection (env `ANKITOV_NLU_PROVIDER`):
/// - `freetoken` → local FreeToken instance (no API key, no internet)
/// - `deepseek`  → cloud DeepSeek API (legacy, requires `DEEPSEEK_API_KEY`)
/// - unset       → try FreeToken first, fall back to DeepSeek, then keyword
pub async fn parse_query(query: &str) -> NluIntent {
    let provider = std::env::var("ANKITOV_NLU_PROVIDER").unwrap_or_default();

    // ── FreeToken path ──────────────────────────────────────────────────
    if provider == "freetoken" {
        match tokio::time::timeout(
            std::time::Duration::from_secs(3),
            try_freetoken_nlu(query),
        )
        .await
        {
            Ok(Ok(intent)) => {
                tracing::info!(
                    intent = %intent.intent,
                    subject = ?intent.subject,
                    "FreeToken NLU parsed query successfully"
                );
                return intent;
            }
            Ok(Err(e)) => {
                tracing::warn!("FreeToken NLU failed ({e}), falling back to keyword parser");
                return keyword_fallback(query);
            }
            Err(_) => {
                tracing::warn!("FreeToken NLU timed out (3s), falling back to keyword parser");
                return keyword_fallback(query);
            }
        }
    }

    if provider == "deepseek" {
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            try_rig_nlu(query),
        )
        .await
        {
            Ok(Ok(intent)) => {
                tracing::info!(
                    intent = %intent.intent,
                    subject = ?intent.subject,
                    "Rig NLU parsed query successfully"
                );
                return intent;
            }
            Ok(Err(e)) => {
                tracing::warn!("Rig NLU failed ({e}), falling back to keyword parser");
                return keyword_fallback(query);
            }
            Err(_) => {
                tracing::warn!("Rig NLU timed out (5s), falling back to keyword parser");
                return keyword_fallback(query);
            }
        }
    }

    // Default or unset or "keyword" → instant, deterministic zero-latency keyword fallback
    keyword_fallback(query)
}

/// Attempt to parse the query using a local FreeToken instance.
///
/// FreeToken serves an OpenAI-compatible chat completions API at a configurable
/// base URL. No API key is required for local inference.
///
/// Environment variables:
///   `ANKITOV_FREETOKEN_BASE_URL` — default `http://localhost:1919/v1`
///   `ANKITOV_FREETOKEN_MODEL`    — default `Qwen3.6-35B-A3B`
async fn try_freetoken_nlu(query: &str) -> Result<NluIntent, String> {
    let base_url = std::env::var("ANKITOV_FREETOKEN_BASE_URL")
        .unwrap_or_else(|_| "http://localhost:1919/v1".into());
    let model = std::env::var("ANKITOV_FREETOKEN_MODEL")
        .unwrap_or_else(|_| "Qwen3.6-35B-A3B".into());

    let client = openai::CompletionsClient::builder()
        .base_url(&base_url)
        .api_key("freetoken") // FreeToken doesn't validate API keys
        .build()
        .map_err(|e| format!("Failed to create FreeToken client: {e}"))?;

    let agent = client
        .agent(&model)
        .preamble(NLU_PREAMBLE)
        .temperature(0.0) // deterministic for intent parsing
        .build();

    let user_prompt = format!("User question: \"{query}\"");
    let response = agent
        .prompt(&user_prompt)
        .await
        .map_err(|e| format!("FreeToken call failed: {e}"))?;

    // Parse the JSON response — strip any accidental markdown fences
    let json_str = response
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    let intent: NluIntent = serde_json::from_str(json_str)
        .map_err(|e| format!("JSON parse error: {e}. Raw response: {json_str}"))?;

    // Basic validation
    let valid_intents = [
        "struggling_students",
        "retention_drop",
        "deck_health",
        "recent_exceptions",
        "missed_practice",
        "count_question",
        "unknown",
    ];
    if !valid_intents.contains(&intent.intent.as_str()) {
        return Err(format!("Unknown intent: {}", intent.intent));
    }

    Ok(intent)
}

/// Attempt to parse the query using rig-core + DeepSeek.
async fn try_rig_nlu(query: &str) -> Result<NluIntent, String> {
    // Allow overriding the model via env
    let model = std::env::var("ANKITOV_RIG_MODEL").unwrap_or_else(|_| "deepseek-chat".into());

    let client = deepseek::Client::from_env()
        .map_err(|e| format!("Failed to create DeepSeek client: {e}"))?;

    let agent = client
        .agent(&model)
        .preamble(NLU_PREAMBLE)
        .temperature(0.0) // deterministic for intent parsing
        .build();

    let user_prompt = format!("User question: \"{query}\"");
    let response = agent.prompt(&user_prompt).await.map_err(|e| format!("LLM call failed: {e}"))?;

    // Parse the JSON response — strip any accidental markdown fences
    let json_str = response.trim().trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();

    let intent: NluIntent =
        serde_json::from_str(json_str).map_err(|e| format!("JSON parse error: {e}. Raw response: {json_str}"))?;

    // Basic validation
    let valid_intents = ["struggling_students", "retention_drop", "deck_health", "recent_exceptions", "missed_practice", "count_question", "unknown"];
    if !valid_intents.contains(&intent.intent.as_str()) {
        return Err(format!("Unknown intent: {}", intent.intent));
    }

    Ok(intent)
}

/// Keyword-based fallback parser — identical logic to the original ask controller.
fn keyword_fallback(q: &str) -> NluIntent {
    let lower = q.to_lowercase();

    // Intent detection
    let trouble_words = ["trouble", "struggl", "failing", "behind", "low retention", "worst"];
    let drop_words = ["dropped off", "fell off", "decline", "decreased", "getting worse", "worse over", "past two weeks", "past week", "lately"];
    let health_words = ["deck health", "how are my decks", "decks doing", "overall health", "status of"];
    let exception_words = ["exceptions", "problems", "alerts", "warnings", "flagged"];
    let missed_words = ["miss", "practice", "haven't studied", "not studied", "studied", "absent", "consecutive", "haven't practiced", "not practiced", "lazy"];

    // Count/enumeration questions: "how many X are there?", "total number of X",
    // "number of X", "how many X do I have?". Must be checked BEFORE the
    // struggle/miss/deck-health words because "how many students are having
    // trouble?" is a count question, not a struggle question.
    let is_count = (lower.contains("how many") || lower.contains("how much")
        || lower.contains("total number") || lower.contains("total count")
        || lower.contains("number of") || lower.contains("count of")
        || lower.contains("how many") || lower.contains("give me the number"))
        && (lower.contains("student") || lower.contains("teacher")
            || lower.contains("class") || lower.contains("deck")
            || lower.contains("card") || lower.contains("module")
            || lower.contains("practice plan") || lower.contains("profile"));

    let intent = if is_count {
        "count_question"
    } else if trouble_words.iter().any(|w| lower.contains(w)) {
        "struggling_students"
    } else if missed_words.iter().any(|w| lower.contains(w)) {
        "missed_practice"
    } else if drop_words.iter().any(|w| lower.contains(w)) {
        "retention_drop"
    } else if health_words.iter().any(|w| lower.contains(w)) {
        "deck_health"
    } else if exception_words.iter().any(|w| lower.contains(w)) {
        "recent_exceptions"
    } else {
        "unknown"
    };

    // Subject extraction
    let subjects = [
        "math", "algebra", "geometry", "biology", "physics", "chemistry",
        "vocab", "vocabulary", "spanish", "french", "history", "science",
        "7th grade", "8th grade", "9th grade", "grade 7", "grade 8", "grade 9",
    ];
    let subject = subjects.iter().find(|s| lower.contains(*s)).map(|s| s.to_string());

    // Time window
    let mut time_window_days = if lower.contains("past two weeks") || lower.contains("last two weeks") || lower.contains("2 weeks") {
        Some(14)
    } else if lower.contains("past week") || lower.contains("last week") || lower.contains("1 week") {
        Some(7)
    } else if lower.contains("past month") || lower.contains("last month") {
        Some(30)
    } else if lower.contains("past 3 days") || lower.contains("last 3 days") {
        Some(3)
    } else {
        None
    };

    if intent == "missed_practice" && time_window_days.is_none() {
        if lower.contains("two consecutive") || lower.contains("2 consecutive") || lower.contains("two days") || lower.contains("2 days") {
            time_window_days = Some(2);
        } else if lower.contains("three consecutive") || lower.contains("3 consecutive") || lower.contains("three days") || lower.contains("3 days") {
            time_window_days = Some(3);
        } else if lower.contains("four consecutive") || lower.contains("4 consecutive") || lower.contains("four days") || lower.contains("4 days") {
            time_window_days = Some(4);
        } else {
            time_window_days = Some(2); // default missed practice query to 2 days
        }
    }

    let summary = format!("[keyword] Intent: {intent}, Subject: {:?}", subject);

    NluIntent {
        intent: intent.to_string(),
        subject,
        time_window_days,
        summary,
    }
}
