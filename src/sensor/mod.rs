use crate::agent::Agent;
use crate::models::{SensorDao, SensorAgentConfigDao};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::vec::Vec;

pub struct SensorHandle {
    pub dao: SensorDao,
    pub agents: Vec<Agent>,
}

impl SensorHandle {
    pub fn from(sensor: SensorDao, actions: &Vec<SensorAgentConfigDao>) -> Option<SensorHandle> {
        let agent_optionals: Vec<Option<Agent>> =
            actions.iter().map(|config| Agent::new(config)).collect();

        if agent_optionals.contains(&Option::None) {
            None // Invalid config provided!
        } else {
            Some(SensorHandle {
                dao: sensor,
                agents: agent_optionals.into_iter().filter_map(|a| a).collect(),
            })
        }
    }

    pub fn on_data(&mut self, data: SensorData) {
        for agent in &mut self.agents {
            // TODO: Async!
            agent.on_data(data.clone());
        }
    }

    pub fn id(&self) -> i32 {
        self.dao.id
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SensorData {
    #[serde(skip, default = "now_timestamp")]
    pub timestamp: DateTime<Utc>,
    pub sensor_id: u32,
    pub grow_id: u32,
    pub temperature: Option<f32>,
    pub light: Option<u32>,
    pub moisture: Option<u32>,
    pub conductivity: Option<u32>,
    pub battery: Option<u32>,
    pub carbon: Option<u32>,
}

impl std::default::Default for SensorData {
    fn default() -> Self {
        SensorData {
            timestamp: now_timestamp(),
            sensor_id: 0,
            grow_id: 0,
            temperature: None,
            light: None,
            moisture: None,
            conductivity: None,
            battery: None,
            carbon: None,
        }
    }
}

fn now_timestamp() -> DateTime<Utc> {
    Utc::now()
}
