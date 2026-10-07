//! Rig AI Chatbox — Natural Language Query Controller
//!
//! Accepts free-form questions from the dashboard chatbox and returns
//! structured results. Intent parsing uses keyword extraction (Rig-style
//! state transforms). Future: feed through `rig-core` pipeline for semantic
//! NLU with entity extraction.
//!
//! # Supported Intents
//! - `struggling_students` — "who's having trouble"
//! - `retention_drop` — "practice dropped off", "retention fell"
//! - `deck_health` — "how are my decks doing", "deck health"
//! - `recent_exceptions` — "show exceptions", "problems detected"

use crate::models::entities::retention_exception as ret_exception_entity;
use crate::models::entities::{class, deck, user};
use crate::models::entities::sync_status as sync_entity;
use crate::models::management::{AskQuery, AskResponse, AskRow};
use crate::services::rig_nlu;
use axum::extract::State;
use axum::Json;
use loco_rs::{app::AppContext, prelude::*};
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect};

const TAG: &str = "Management Console";

/// Routes for the Rig AI chatbox.
pub fn routes() -> Routes {
    Routes::new()
        .prefix("/management")
        .add("/ask", post(ask))
}

/// Accept a natural language query and return structured results.
#[utoipa::path(
    post,
    path = "/management/ask",
    request_body = AskQuery,
    responses(
        (status = 200, description = "Structured answer from intent parser", body = AskResponse)
    ),
    tag = TAG
)]
pub async fn ask(
    State(ctx): State<AppContext>,
    Json(payload): Json<AskQuery>,
) -> Result<Response> {
    tracing::info!("Rig chatbox query: {}", payload.query);

    // ── Intent Detection (Rig NLU → Keyword Fallback) ──────────────
    let nlu = rig_nlu::parse_query(&payload.query).await;
    let q_lower = payload.query.to_lowercase();

    // ── Query Execution ────────────────────────────────────────────
    let mut response = match nlu.intent.as_str() {
        "struggling_students" => struggling_students(&ctx, &q_lower).await?,
        "missed_practice" => missed_practice(&ctx, nlu.time_window_days.unwrap_or(2)).await?,
        "retention_drop" => retention_drop(&ctx, &q_lower).await?,
        "deck_health" => deck_health(&ctx).await?,
        "recent_exceptions" => recent_exceptions(&ctx).await?,
        "count_question" => count_question(&ctx, &q_lower).await?,
        "decks" => unknown_intent(&payload.query),
        _ => unknown_intent(&payload.query),
    };

    // Override summary with the NLU-generated one if available (LLM is more fluent)
    if !nlu.summary.starts_with("[keyword]") {
        response.summary = nlu.summary;
    }

    format::json(response)
}

// ── Intent Detection ────────────────────────────────────────────────

fn detect_intent(q: &str) -> String {
    // Priority-ordered keyword matches
    let trouble_words = ["trouble", "struggl", "failing", "behind", "low retention", "worst"];
    let drop_words = ["dropped off", "fell off", "decline", "decreased", "getting worse", "worse over", "past two weeks", "past week", "lately"];
    let health_words = ["deck health", "how are my decks", "decks doing", "overall health", "status of"];
    let exception_words = ["exceptions", "problems", "alerts", "warnings", "flagged"];

    if trouble_words.iter().any(|w| q.contains(w)) {
        "struggling_students".to_string()
    } else if drop_words.iter().any(|w| q.contains(w)) {
        "retention_drop".to_string()
    } else if health_words.iter().any(|w| q.contains(w)) {
        "deck_health".to_string()
    } else if exception_words.iter().any(|w| q.contains(w)) {
        "recent_exceptions".to_string()
    } else {
        "unknown".to_string()
    }
}

// ── Intent Handlers ─────────────────────────────────────────────────

/// Students with active retention exceptions — "who's having trouble?"
async fn struggling_students(ctx: &AppContext, q: &str) -> Result<AskResponse> {
    // Extract optional subject filter from query
    let subject_filter = extract_subject(q);

    let mut query = ret_exception_entity::Entity::find()
        .filter(ret_exception_entity::Column::ResolvedAt.is_null())
        .order_by_asc(ret_exception_entity::Column::RetentionRate)
        .all(&ctx.db)
        .await?;

    // Filter by subject if mentioned
    if let Some(ref subject) = subject_filter {
        query.retain(|e| e.deck_id.to_lowercase().contains(&subject.to_lowercase()));
    }

    let rows: Vec<AskRow> = query
        .into_iter()
        .map(|e| {
            let status = if e.retention_rate < 0.40 {
                "danger"
            } else if e.retention_rate < 0.60 {
                "warn"
            } else {
                "ok"
            };
            AskRow {
                label: format!("Student {}", e.user_id),
                value: format!("{:.0}% retention", e.retention_rate * 100.0),
                status: Some(status.to_string()),
                detail: Some(format!("Deck: {} — threshold {:.0}%", e.deck_id, e.threshold * 100.0)),
            }
        })
        .collect();

    let subject_text = subject_filter
        .map(|s| format!(" in {}", s))
        .unwrap_or_default();

    Ok(AskResponse {
        summary: format!(
            "Found {} student{} with active retention flags{}. {}",
            rows.len(),
            if rows.len() == 1 { "" } else { "s" },
            subject_text,
            if rows.is_empty() {
                "Everyone looks good right now! 🎉"
            } else if rows.iter().any(|r| r.status == Some("danger".into())) {
                "⚠ Some students need immediate attention."
            } else {
                "Keep an eye on flagged students."
            }
        ),
        intent: "struggling_students".into(),
        navigate_to: Some("stats".into()),
        rows,
    })
}

/// Students who have missed practice for at least N consecutive days.
async fn missed_practice(ctx: &AppContext, time_days: i64) -> Result<AskResponse> {
    tracing::info!("Querying students who missed practice for >= {} days", time_days);

    // Query all sync statuses
    let syncs = sync_entity::Entity::find()
        .all(&ctx.db)
        .await
        .map_err(|e| {
            tracing::error!("Failed to query sync statuses: {:?}", e);
            loco_rs::Error::InternalServerError
        })?;

    let now = chrono::Utc::now().timestamp();
    let threshold_seconds = time_days * 24 * 3600;

    let mut rows: Vec<AskRow> = Vec::new();

    for s in syncs {
        let is_missed = match s.last_sync {
            None => true, // never synced is counted as missed
            Some(last) => (now - last) >= threshold_seconds,
        };

        if is_missed {
            let detail = match s.last_sync {
                None => "Never synced with Anki Cloud".to_string(),
                Some(last) => {
                    let diff_hours = (now - last) / 3600;
                    let diff_days = diff_hours / 24;
                    if diff_days > 0 {
                        format!("Last synced {} day{} ago", diff_days, if diff_days == 1 { "" } else { "s" })
                    } else {
                        format!("Last synced {} hour{} ago", diff_hours, if diff_hours == 1 { "" } else { "s" })
                    }
                }
            };

            let status = if time_days >= 3 {
                "danger"
            } else {
                "warn"
            };

            rows.push(AskRow {
                label: format!("Student {}", s.user_id),
                value: format!("Missed (>= {} days)", time_days),
                status: Some(status.to_string()),
                detail: Some(detail),
            });
        }
    }

    // Sort: most days missed first, or alphabetically
    rows.sort_by(|a, b| {
        let extract_days = |detail: &Option<String>| -> i64 {
            if let Some(ref d) = detail {
                if d.contains("Never") {
                    return 999;
                }
                // Extract number of days from "Last synced X days ago"
                if let Some(first_word) = d.split_whitespace().nth(2) {
                    if let Ok(n) = first_word.parse::<i64>() {
                        return n;
                    }
                }
            }
            0
        };
        extract_days(&b.detail).cmp(&extract_days(&a.detail))
    });

    Ok(AskResponse {
        summary: format!(
            "Found {} student{} who haven't completed a study session for at least {} consecutive days.",
            rows.len(),
            if rows.len() == 1 { "" } else { "s" },
            time_days
        ),
        intent: "missed_practice".into(),
        navigate_to: Some("stats".into()),
        rows,
    })
}

/// Students whose retention dropped significantly — "practice fell off"
async fn retention_drop(ctx: &AppContext, q: &str) -> Result<AskResponse> {
    let subject_filter = extract_subject(q);
    let time_filter = extract_time_window(q); // days

    // Query all unresolved exceptions, sorted by most recent
    let mut exceptions = ret_exception_entity::Entity::find()
        .filter(ret_exception_entity::Column::ResolvedAt.is_null())
        .order_by_desc(ret_exception_entity::Column::DetectedAt)
        .all(&ctx.db)
        .await?;

    // Subject filter
    if let Some(ref subject) = subject_filter {
        exceptions.retain(|e| e.deck_id.to_lowercase().contains(&subject.to_lowercase()));
    }

    // Time filter: only recent exceptions
    let now = chrono::Utc::now().timestamp();
    let cutoff = now - (time_filter * 24 * 3600);
    exceptions.retain(|e| e.detected_at >= cutoff);

    let rows: Vec<AskRow> = exceptions
        .into_iter()
        .map(|e| {
            let days_ago = (now - e.detected_at) / 86400;
            AskRow {
                label: format!("Student {}", e.user_id),
                value: format!("{:.0}% → {:.0}% (threshold)", e.retention_rate * 100.0, e.threshold * 100.0),
                status: Some("warn".into()),
                detail: Some(format!(
                    "Deck: {} · detected {} day{} ago",
                    e.deck_id,
                    days_ago,
                    if days_ago == 1 { "" } else { "s" }
                )),
            }
        })
        .collect();

    let subject_text = subject_filter
        .map(|s| format!(" for {}", s))
        .unwrap_or_default();

    Ok(AskResponse {
        summary: format!(
            "{} student{} showed retention drops{} in the past {} day{}. {}",
            rows.len(),
            if rows.len() == 1 { "" } else { "s" },
            subject_text,
            time_filter,
            if time_filter == 1 { "" } else { "s" },
            if rows.is_empty() {
                "No recent drops detected — retention is holding steady. 👍"
            } else {
                "Consider reviewing their study patterns or adjusting difficulty."
            }
        ),
        intent: "retention_drop".into(),
        navigate_to: Some("stats".into()),
        rows,
    })
}

/// Deck-level health summary.
async fn deck_health(ctx: &AppContext) -> Result<AskResponse> {
    let exceptions = ret_exception_entity::Entity::find()
        .filter(ret_exception_entity::Column::ResolvedAt.is_null())
        .all(&ctx.db)
        .await?;

    // Group by deck_id
    let mut deck_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for e in &exceptions {
        *deck_counts.entry(e.deck_id.clone()).or_insert(0) += 1;
    }

    let mut rows: Vec<AskRow> = deck_counts
        .into_iter()
        .map(|(deck, count)| {
            let status = if count >= 3 {
                "danger"
            } else if count >= 1 {
                "warn"
            } else {
                "ok"
            };
            AskRow {
                label: deck,
                value: format!("{} flagged student{}", count, if count == 1 { "" } else { "s" }),
                status: Some(status.to_string()),
                detail: None,
            }
        })
        .collect();

    rows.sort_by(|a, b| b.status.cmp(&a.status)); // danger first

    Ok(AskResponse {
        summary: format!(
            "{} deck{} with active flags. {}",
            rows.len(),
            if rows.len() == 1 { "" } else { "s" },
            if rows.is_empty() {
                "All decks are healthy! 🎉"
            } else {
                "Decks marked red need immediate attention."
            }
        ),
        intent: "deck_health".into(),
        navigate_to: Some("capsules".into()),
        rows,
    })
}

/// Recent unresolved exceptions.
async fn recent_exceptions(ctx: &AppContext) -> Result<AskResponse> {
    let exceptions = ret_exception_entity::Entity::find()
        .filter(ret_exception_entity::Column::ResolvedAt.is_null())
        .order_by_desc(ret_exception_entity::Column::DetectedAt)
        .limit(20)
        .all(&ctx.db)
        .await?;

    let rows: Vec<AskRow> = exceptions
        .into_iter()
        .map(|e| AskRow {
            label: format!("{} / {}", e.user_id, e.deck_id),
            value: format!("{:.0}% (limit {:.0}%)", e.retention_rate * 100.0, e.threshold * 100.0),
            status: Some(if e.retention_rate < 0.40 { "danger" } else { "warn" }.into()),
            detail: Some(format!("Type: {}", e.exception_type)),
        })
        .collect();

    Ok(AskResponse {
        summary: format!(
            "{} active exception{} in the system. {}",
            rows.len(),
            if rows.len() == 1 { "" } else { "s" },
            if rows.is_empty() {
                "System is clean — no active alerts. 🎉"
            } else {
                "Review the flagged items above."
            }
        ),
        intent: "recent_exceptions".into(),
        navigate_to: Some("capsules".into()),
        rows,
    })
}

/// Answer counting/enumeration questions with live DB totals:
/// "how many students?", "how many classes?", "how many decks/cards?", etc.
async fn count_question(ctx: &AppContext, q: &str) -> Result<AskResponse> {
    let now = chrono::Utc::now().timestamp();
    let month_ago = now - 30 * 24 * 3600;

    // Live counts from the database
    let students_total = user::Entity::find()
        .filter(user::Column::Role.eq("student"))
        .count(&ctx.db).await? as u64;
    let teachers_total = user::Entity::find()
        .filter(user::Column::Role.eq("teacher"))
        .count(&ctx.db).await? as u64;
    let classes_total = class::Entity::find().count(&ctx.db).await? as u64;
    let decks_total = deck::Entity::find().count(&ctx.db).await? as u64;

    let cards_total = deck::Entity::find()
        .all(&ctx.db).await?
        .iter().map(|d| d.card_count as u64).sum::<u64>();

    // "this month" approximations — created within last 30 days
    let students_new = user::Entity::find()
        .filter(user::Column::Role.eq("student"))
        .filter(user::Column::CreatedAt.gte(month_ago))
        .count(&ctx.db).await? as u64;
    let teachers_new = user::Entity::find()
        .filter(user::Column::Role.eq("teacher"))
        .filter(user::Column::CreatedAt.gte(month_ago))
        .count(&ctx.db).await? as u64;
    let classes_new = class::Entity::find()
        .filter(class::Column::CreatedAt.gte(month_ago))
        .count(&ctx.db).await? as u64;
    let decks_new = deck::Entity::find()
        .filter(deck::Column::CreatedAt.gte(month_ago))
        .count(&ctx.db).await? as u64;

    let this_month = q.contains("this month") || q.contains("last month") || q.contains("recently");

    // Decide which metric(s) the user is asking about.
    let want_students = q.contains("student");
    let want_teachers = q.contains("teacher");
    let want_classes = q.contains("class");
    let want_decks = q.contains("deck");
    let want_cards = q.contains("card");
    let want_any = want_students || want_teachers || want_classes || want_decks || want_cards;

    let pick = |total: u64, new: u64| -> u64 { if this_month { new } else { total } };

    let mut parts: Vec<String> = Vec::new();
    // Parts are plain "N unit" phrases; the scope ("currently" / "this month")
    // is added by the summary sentence so it is never repeated in the parts.
    if want_students {
        parts.push(format!("{} students", pick(students_total, students_new)));
    }
    if want_teachers {
        parts.push(format!("{} teachers", pick(teachers_total, teachers_new)));
    }
    if want_classes {
        parts.push(format!("{} classes", pick(classes_total, classes_new)));
    }
    if want_decks {
        parts.push(format!("{} decks", pick(decks_total, decks_new)));
    }
    if want_cards {
        parts.push(format!("{} cards", cards_total));
    }

    // Generic "how many" with no specific noun → give the full overview.
    let summary = if parts.is_empty() {
        format!(
            "Here's your current overview: {} students, {} teachers, {} classes, {} decks, and {} total cards.",
            students_total, teachers_total, classes_total, decks_total, cards_total
        )
    } else if this_month {
        // e.g. "how many students this month?" → "This month: 4 students."
        format!("This month: {}.", parts.join(", "))
    } else {
        // e.g. "how many students are there?" → "There are 4 students currently."
        format!("There are {} currently.", parts.join(", "))
    };

    // Structured rows for the chatbox UI
    let rows: Vec<AskRow> = vec![
        AskRow { label: "Students".into(), value: students_total.to_string(), status: Some("ok".into()), detail: None },
        AskRow { label: "Teachers".into(), value: teachers_total.to_string(), status: Some("ok".into()), detail: None },
        AskRow { label: "Classes".into(), value: classes_total.to_string(), status: Some("ok".into()), detail: None },
        AskRow { label: "Decks".into(), value: decks_total.to_string(), status: Some("ok".into()), detail: None },
        AskRow { label: "Cards".into(), value: cards_total.to_string(), status: Some("ok".into()), detail: None },
    ];

    Ok(AskResponse {
        summary,
        intent: "count_question".into(),
        navigate_to: Some("stats".into()),
        rows,
    })
}

/// Fallback when the NLU couldn't classify the query's **intent**.
///
/// We deliberately do *not* scan the raw query for broad words like "class"/
/// "practice"/"study" to decide navigation — that would misroute almost any
/// ordinary sentence (e.g. "order pizza for the class party" → Classes screen).
///
/// We *do* navigate to the Decks screen, but only for a narrow, high-precision
/// signal: the query mentions a deck **and** an action verb (upload / import /
/// distribute). Deck/upload is an explicit "I want to do X" intent, unlike
/// "class health" (a question) which must stay a query.
///
/// Otherwise we stay put and return a helpful "I'm best at: …" suggestion list
/// rather than a bare "Sorry, I could not answer that."
/// (Frontend command chips that navigate do so via `Router.navigate`, not here,
/// so they are unaffected by this logic.)
fn unknown_intent(q: &str) -> AskResponse {
    // Deck/upload action → route to the Decks screen.
    // The NLU only returns this intent when the query clearly asks to
    // upload / import / distribute a deck, so navigating is correct.
    let ql = q.to_lowercase();
    let navigate_to: Option<String> = if ql.contains("deck")
        && (ql.contains("upload") || ql.contains("import") || ql.contains("distribut"))
    {
        Some("decks".into())
    } else {
        None
    };

    let summary = if navigate_to.is_some() {
        format!(
            "Sure — that lives on the {} screen. I've taken you there.",
            match navigate_to.as_deref() {
                Some("decks") => "Decks",
                _ => "",
            }
        )
    } else {
        format!(
            "I'm not sure what you're asking about. I'm best at:\n\
             • \"Which students are having trouble?\"\n\
             • \"Who hasn't practiced in the past two weeks?\"\n\
             • \"How are my decks doing?\"\n\
             • \"How many students / classes / decks do I have?\"\n\
             • \"Show me recent exceptions\"\n\n\
             You asked: \"{}\"",
            q
        )
    };

    AskResponse {
        summary,
        intent: "unknown".into(),
        navigate_to,
        rows: vec![],
    }
}

// ── Helpers ─────────────────────────────────────────────────────────

/// Extract a subject/deck name from the query (e.g., "7th Grade math").
fn extract_subject(q: &str) -> Option<String> {
    let subjects = [
        "math", "algebra", "geometry", "biology", "physics", "chemistry",
        "vocab", "vocabulary", "spanish", "french", "history", "science",
        "7th grade", "8th grade", "9th grade", "grade 7", "grade 8", "grade 9",
    ];
    for s in &subjects {
        if q.contains(s) {
            return Some(s.to_string());
        }
    }
    None
}

/// Extract time window from query (default 14 days).
fn extract_time_window(q: &str) -> i64 {
    if q.contains("past two weeks") || q.contains("last two weeks") || q.contains("2 weeks") {
        14
    } else if q.contains("past week") || q.contains("last week") || q.contains("1 week") {
        7
    } else if q.contains("past month") || q.contains("last month") {
        30
    } else if q.contains("past 3 days") || q.contains("last 3 days") {
        3
    } else {
        14 // default
    }
}