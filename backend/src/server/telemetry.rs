use async_trait::async_trait;
use tokio::sync::broadcast;
use serde::{Serialize, Deserialize};

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
