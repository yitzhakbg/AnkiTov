use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CardStateFrame {
    pub card_id: String,
    pub deck_id: String,
    pub interval_seconds: u64,
}

pub fn calculate_retention_leakage(reviewed_cards: &[CardStateFrame], lapses_count: usize, threshold: f64) -> bool {
    if reviewed_cards.is_empty() { return false; }
    let leak_ratio = (lapses_count as f64) / (reviewed_cards.len() as f64);
    leak_ratio >= threshold
}
