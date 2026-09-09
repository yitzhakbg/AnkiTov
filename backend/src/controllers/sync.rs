use axum::{extract::ws::{Message, WebSocket, WebSocketUpgrade}, response::IntoResponse, routing::get, Extension};
use loco_rs::prelude::*;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct CardReviewTransaction {
    pub deck_id: String,
    pub card_id: String,
    pub grade: u8,
    pub timestamp: i64,
}

pub async fn sync_stream_handler(
    ws: WebSocketUpgrade,
    Extension(tx): Extension<broadcast::Sender<String>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_websocket_loop(socket, tx))
}

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

pub fn routes() -> Routes {
    Routes::new().add("/sync/stream", get(sync_stream_handler))
}
