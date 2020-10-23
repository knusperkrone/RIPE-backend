use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::pin::Pin;

pub enum AgentMessage {
    Command(i32),
    OneshotTask(Pin<Box<dyn std::future::Future<Output = ()> + Send + Sync + 'static>>),
    RepeatedTask(
        std::time::Duration,
        Pin<Box<dyn std::future::Future<Output = bool> + Send + Sync + 'static>>,
    ),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
