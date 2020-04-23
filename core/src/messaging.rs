use crate::agent::AgentState;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::pin::Pin;

pub enum AgentMessage {
    State(AgentState),
    Bool(bool),
    Int(i32),
    Task(Pin<Box<dyn std::future::Future<Output = ()> + Send + Sync + 'static>>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SensorDataDto {
    #[serde(default = "Utc::now")]
    pub timestamp: DateTime<Utc>,
    pub refresh: Option<bool>,
    pub temperature: Option<f32>,
    pub light: Option<u32>,
    pub moisture: Option<u32>,
    pub conductivity: Option<u32>,
    pub battery: Option<u32>,
    pub carbon: Option<u32>,
}

impl std::default::Default for SensorDataDto {
    fn default() -> Self {
        SensorDataDto {
            timestamp: Utc::now(),
            refresh: None,
            temperature: None,
            light: None,
            moisture: None,
            conductivity: None,
            battery: None,
            carbon: None,
        }
    }
}
