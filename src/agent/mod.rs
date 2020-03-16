use crate::error::AgentError;
use crate::logging::APP_LOGGING;
use crate::models::{AgentConfigDao, NewAgentConfig};
use crate::sensor::SensorData;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::string::String;

pub mod water;

#[derive(Serialize, Deserialize, Debug)]
pub struct AgentRegisterConfig {
    pub domain: String,
}

#[derive(Debug, PartialEq)]
pub enum Agent {
    ThresholdWaterAgent(water::ThresholdWaterAgent),
    #[cfg(test)]
    MockAgent(mock::MockAgent),
}

impl Agent {
    pub const WATER_DOMAIN: &'static str = "water";

    pub fn new(config: &AgentRegisterConfig) -> Result<Agent, AgentError> {
        match config.domain.as_str() {
            Agent::WATER_DOMAIN => Ok(water::create_new()),
            _ => Err(AgentError::InvalidDomain(config.domain.clone())),
        }
    }

    pub fn from(config: &AgentConfigDao) -> Result<Agent, AgentError> {
        match config.domain.as_str() {
            Agent::WATER_DOMAIN => water::create_from(config),
            _ => Err(AgentError::InvalidDomain(config.domain.clone())),
        }
    }

    pub fn on_data(&mut self, data: SensorData) {
        match self {
            Agent::ThresholdWaterAgent(x) => x.on_data(data),
            #[cfg(test)]
            Agent::MockAgent(x) => x.on_data(data),
        }
    }

    pub fn deserialize(&self) -> NewAgentConfig {
        match self {
            Agent::ThresholdWaterAgent(x) => x.deserialize(),
            #[cfg(test)]
            Agent::MockAgent(x) => x.deserialize(),
        }
    }

    /*
    pub fn on_force(&mut self, duration: Duration) {
        match self {
            Agent::TresholdWaterAgent(x) => x.on_force(duration),
        }
    }
    */
}

#[derive(PartialEq, std::fmt::Debug, Deserialize, Serialize)]
enum AgentState {
    Active,
    Forced(DateTime<Utc>),
}

impl Default for AgentState {
    fn default() -> Self {
        AgentState::Active
    }
}

trait AgentTrait {
    fn do_action(&mut self, data: SensorData);
    fn do_force(&mut self, until: DateTime<Utc>);

    fn on_force(&mut self, duration: Duration) {
        let until = Utc::now() + duration;
        self.do_force(until);
    }
    fn on_data(&mut self, data: SensorData) {
        match self.state() {
            AgentState::Active => self.do_action(data),
            AgentState::Forced(_) => {
                // TODO: Log
                info!(APP_LOGGING, "Sensor is already active!");
            }
        }
    }

    fn deserialize(&self) -> NewAgentConfig;
    fn state(&self) -> &AgentState;
}

#[cfg(test)]
pub mod mock {
    use super::*;

    #[derive(std::fmt::Debug, PartialEq)]
    pub struct MockAgent {
        pub last_action: Option<SensorData>,
        pub last_forced: Option<Duration>,
    }

    impl MockAgent {
        pub fn new() -> Agent {
            Agent::MockAgent(MockAgent {
                last_action: None,
                last_forced: None,
            })
        }
    }

    impl AgentTrait for MockAgent {
        fn do_action(&mut self, data: SensorData) {
            self.last_action = Some(data);
        }

        fn deserialize(&self) -> NewAgentConfig {
            NewAgentConfig {
                agent_impl: "mock".to_string(),
                domain: "mock".to_string(),
                state_json: "{}".to_string(),
            }
        }
        fn state(&self) -> &AgentState {
            &AgentState::Active
        }

        fn do_force(&mut self, _until: DateTime<Utc>) {
            // no-op
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_create_default_water_agent() {
        let actual = Agent::new(&AgentRegisterConfig {
            domain: Agent::WATER_DOMAIN.to_string(),
        });

        assert_eq!(true, actual.is_ok())
    }

    #[test]
    fn test_create_threshold_water_agent() {
        let actual = Agent::from(&AgentConfigDao {
            sensor_id: 0,
            domain: Agent::WATER_DOMAIN.to_string(),
            agent_impl: water::ThresholdWaterAgent::IDENTIFIER.to_string(),
            state_json: "{\"state\":\"Active\",\"min_threshold\":20,\"watering_start\":null,\"last_watering\":null}".to_string(),
        });

        assert_eq!(true, actual.is_ok());
        if let Ok(Agent::ThresholdWaterAgent(_)) = actual {
            // no-op
        } else {
            panic!("Wrong agent type!");
        }
    }
}
