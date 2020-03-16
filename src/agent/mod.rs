pub mod water;

use crate::logging::APP_LOGGING;
use crate::models::SensorAgentConfigDao;
use crate::sensor::SensorData;
use std::time::Duration;

#[derive(std::fmt::Debug, PartialEq)]
pub enum Agent {
    TresholdWaterAgent(water::TresholdWaterAgent),
    #[cfg(test)]
    MockAgent(mock::MockAgent),
}

impl Agent {
    pub const WATER_IDENTIFIER: &'static str = "water";

    pub fn new(config: &SensorAgentConfigDao) -> Option<Agent> {
        match config.action.as_str() {
            Agent::WATER_IDENTIFIER => water::new(config),
            _ => {
                warn!(APP_LOGGING, "Invalid agent name: {}", config.action);
                None
            }
        }
    }

    pub fn on_data(&mut self, data: SensorData) {
        match self {
            Agent::TresholdWaterAgent(x) => x.on_data(data),
            #[cfg(test)]
            Agent::MockAgent(x) => x.on_data(data),
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

#[derive(PartialEq, std::fmt::Debug)]
enum State {
    Active,
    Forced(Duration),
}

trait AgentTrait {
    fn state(&self) -> &State;
    fn do_action(&mut self, data: SensorData);
    fn on_force(&mut self, duration: Duration);
    fn on_data(&mut self, data: SensorData) {
        match self.state() {
            State::Active => self.do_action(data),
            State::Forced(_) => {
                // TODO: Log
                info!(APP_LOGGING, "Sensor is already active!");
            }
        }
    }
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
        fn state(&self) -> &State {
            &State::Active
        }
        fn do_action(&mut self, data: SensorData) {
            self.last_action = Some(data);
        }
        fn on_force(&mut self, duration: Duration) {
            self.last_forced = Some(duration);
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_create_treshold_water_agent() {
        let actual = Agent::new(&SensorAgentConfigDao {
            sensor_id: 0,
            action: Agent::WATER_IDENTIFIER.to_string(),
            agent_impl: water::TresholdWaterAgent::IDENTIFIER.to_string(),
            config_json: "TODO".to_string(),
        });

        if let Some(Agent::TresholdWaterAgent(_)) = actual {
            // no-op
        } else {
            panic!("Wrong agent type!");
        }
    }
}
