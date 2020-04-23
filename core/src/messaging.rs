use crate::agent::AgentState;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize, Serializer};

#[derive(Debug, Copy, Clone)]
pub enum AgentPayload {
    State(AgentState),
    Bool(bool),
    Int(i32),
}

impl Serialize for AgentPayload {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            AgentPayload::Bool(b) => serializer.serialize_bool(*b),
            AgentPayload::Int(i) => serializer.serialize_i32(*i),
            AgentPayload::State(_) => serializer.serialize_none(),
        }
    }
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
