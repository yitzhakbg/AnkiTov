use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct DiagnosticFrame {
    pub tenant_namespace: String,
    pub event_identifier: String,
    pub event_payload: Vec<u8>,
}

#[async_trait]
pub trait TelemetryStreamer: Send + Sync {
    async fn stream_diagnostics(&self) -> broadcast::Receiver<DiagnosticFrame>;
}
