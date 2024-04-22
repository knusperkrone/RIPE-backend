use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::pin::Pin;

pub trait FutBuilder: Send + Sync {
    fn build_future(
        &self,
        runtime: tokio::runtime::Handle,
    ) -> Pin<Box<dyn std::future::Future<Output = bool> + Send + Sync + 'static>>;
}

impl std::fmt::Debug for dyn FutBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FutBuilder").finish()
    }
}

#[derive(Debug)]
pub enum AgentMessage {
    Command(i32),
    OneshotTask(Box<dyn FutBuilder>),
    RepeatedTask(std::time::Duration, Box<dyn FutBuilder>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct SensorDataMessage {
    #[serde(default = "Utc::now")]
    pub timestamp: DateTime<Utc>,
    pub battery: Option<f64>,
    pub moisture: Option<f64>,
    pub temperature: Option<f64>,
    pub carbon: Option<i32>,
    pub conductivity: Option<i32>,
    pub light: Option<i32>,
}

impl std::default::Default for SensorDataMessage {
    fn default() -> Self {
        SensorDataMessage {
            timestamp: Utc::now(),
            temperature: None,
            light: None,
            moisture: None,
            conductivity: None,
            battery: None,
            carbon: None,
        }
    }
}
