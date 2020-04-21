use crate::agent::{plugin::AgentFactory, Agent, AgentPayload};
use crate::models::{AgentConfigDao, SensorDao};
use chrono::{DateTime, Utc};
use plugins_core::{error::AgentError, SensorData};
use serde::{Deserialize, Serialize};
use std::vec::Vec;

pub struct SensorHandle {
    pub dao: SensorDao,
    pub agents: Vec<Agent>,
}

impl SensorHandle {
    pub fn from(
        sensor: SensorDao,
        actions: &Vec<AgentConfigDao>,
        factory: &AgentFactory,
    ) -> Result<SensorHandle, AgentError> {
        // TODO: Filter invalid!
        let agents: Vec<Agent> = actions
            .into_iter()
            .map(|config| factory.restore_agent(config))
            .filter_map(Result::ok)
            .collect();

        Ok(SensorHandle {
            dao: sensor,
            agents,
        })
    }

    pub fn on_data(&mut self, data: &SensorData) -> Vec<SensorMessage> {
        let id = self.id();
        self.agents
            .iter_mut()
            .filter_map(|agent| {
                if let Some(payload) = agent.on_data(data) {
                    Some(SensorMessage {
                        payload: payload.into(),
                        sensor_id: id,
                    })
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn id(&self) -> i32 {
        self.dao.id
    }
}

#[derive(Deserialize, Serialize)]
pub struct SensorMessage {
    pub sensor_id: i32,
    pub payload: AgentPayload,
}

pub struct SensorHandleData {
    pub sensor_id: i32,
    pub data: SensorData,
}

impl SensorHandleData {
    pub fn new(sensor_id: i32, dto: SensorData) -> Self {
        SensorHandleData {
            sensor_id: sensor_id,
            data: dto,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct SensorDataDto {
    #[serde(default = "Utc::now", skip_deserializing)]
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

impl From<SensorDataDto> for SensorData {
    fn from(other: SensorDataDto) -> Self {
        SensorData {
            timestamp: other.timestamp,
            refresh: other.refresh,
            temperature: other.temperature,
            light: other.light,
            moisture: other.moisture,
            conductivity: other.conductivity,
            battery: other.battery,
            carbon: other.carbon,
        }
    }
}
