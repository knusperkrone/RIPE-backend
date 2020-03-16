use crate::agent::{Agent, AgentState, AgentTrait};
use crate::models::{AgentConfigDao, NewAgentConfig};
use crate::sensor::SensorData;
use crate::error::AgentError;
use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};

pub fn create_new() -> Agent {
    ThresholdWaterAgent::new()
}

pub fn create_from(config: &AgentConfigDao) -> Result<Agent, AgentError> {
    match config.agent_impl.as_str() {
        ThresholdWaterAgent::IDENTIFIER => ThresholdWaterAgent::with_config(config),
        _ => Err(AgentError::InvalidIdentifier(config.agent_impl.clone())),
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct ThresholdWaterAgent {
    state: AgentState,
    min_threshold: u32,
    watering_start: Option<DateTime<Utc>>,
    last_watering: Option<DateTime<Utc>>,
}

impl ThresholdWaterAgent {
    pub const IDENTIFIER: &'static str = "ThresholdWaterAgent";

    pub fn new() -> Agent {
        Agent::ThresholdWaterAgent(ThresholdWaterAgent {
            state: AgentState::Active,
            min_threshold: 20,
            watering_start: None,
            last_watering: None,
        })
    }

    pub fn with_config(config: &AgentConfigDao) -> Result<Agent, AgentError> {
        match serde_json::from_str::<ThresholdWaterAgent>(&config.state_json) {
            Ok(agent) => Ok(Agent::ThresholdWaterAgent(agent)),
            Err(e) => Err(AgentError::InvalidConfig(config.state_json.clone(), e)),
        }
    }
}

impl AgentTrait for ThresholdWaterAgent {
    fn do_action(&mut self, data: SensorData) {
        let _now = Utc::now();
        let _watering_diff = self
            .last_watering
            .unwrap_or(DateTime::from_utc(NaiveDateTime::from_timestamp(0, 0), Utc))
            - data.timestamp;

        if data.moisture.unwrap_or(std::u32::MAX) < self.min_threshold {
            //
        }
        /*
        moisture = sensor_data.moisture

        if moisture is None or not self.global_settings.is_in_quiet_range(now):
            logger.debug(f"Not watering as no data or in quiet range!")
        elif now - self.last_watering < timedelta(minutes=_LAST_WATERING_MIN_TIMEOUT):
            logger.debug(f"Not watering as last watering is only {_LAST_WATERING_MIN_TIMEOUT} mins passed")
        elif moisture < self.config_min_threshold and self.last_moisture < self.config_min_threshold:
            logger.debug(f"Watering with: {moisture}% and {self.last_moisture}%")
            self._water()

        self.last_moisture = moisture
            */
    }

    fn do_force(&mut self, until: DateTime<Utc>) {
        self.state = AgentState::Forced(until);
    }

    fn state(&self) -> &AgentState {
        &self.state
    }

    fn deserialize(&self) -> NewAgentConfig {
        NewAgentConfig {
            domain: Agent::WATER_DOMAIN.to_string(),
            agent_impl: ThresholdWaterAgent::IDENTIFIER.to_string(),
            state_json: serde_json::to_string(self).unwrap(),
        }
    }
}
