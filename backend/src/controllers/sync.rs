//! Card-review sync WebSocket endpoint.
//!
//! Receives card review transactions from clients over a WebSocket and
//! rebroadcasts them on a shared `tokio::sync::broadcast` channel for
//! fan-out to other subscribers (e.g. live dashboards / the display wall).
//! Each accepted transaction is acknowledged to the sender with `SYNC_ACK`.

use axum::{extract::ws::{Message, WebSocket, WebSocketUpgrade}, response::IntoResponse, routing::get, Extension};
use loco_rs::prelude::*;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

/// One card review event as sent by a client over the sync WebSocket.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct CardReviewTransaction {
    /// Deck identifier the reviewed card belongs to.
    pub deck_id: String,
    /// Anki card identifier that was reviewed.
    pub card_id: String,
    /// Review grade (recall rating) assigned by the student.
    pub grade: u8,
    /// Unix timestamp in seconds (UTC) of the review event.
    pub timestamp: i64,
}

/// WebSocket upgrade for `/sync/stream`.
///
/// Accepts the upgrade and hands the socket to `handle_websocket_loop`,
/// forwarding the shared broadcast sender for rebroadcast.
pub async fn sync_stream_handler(
    ws: WebSocketUpgrade,
    Extension(tx): Extension<broadcast::Sender<String>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_websocket_loop(socket, tx))
}

/// Receive loop for one connected sync client.
///
/// Reads text frames, parses each as a [`CardReviewTransaction`], and:
/// 1. rebroadcasts the JSON payload on `tx` (subscribers get a copy), and
/// 2. replies with a bare `SYNC_ACK` text frame.
///
/// Frames that are not valid JSON are silently dropped. The loop ends when
/// the client disconnects or the ack write fails.
async fn handle_websocket_loop(mut socket: WebSocket, tx: broadcast::Sender<String>) {
    while let Some(Ok(frame)) = socket.recv().await {
        if let Message::Text(text_payload) = frame {
            if let Ok(transaction) = serde_json::from_str::<CardReviewTransaction>(&text_payload) {
                let _ = tx.send(serde_json::to_string(&transaction).unwrap());
                if socket.send(Message::Text("SYNC_ACK".into())).await.is_err() { break; }
            }
        }
    }
}

/// Routes for the card-review sync WebSocket.
pub fn routes() -> Routes {
    Routes::new().add("/sync/stream", get(sync_stream_handler))
}
