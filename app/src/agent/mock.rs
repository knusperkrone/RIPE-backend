use chrono::{DateTime, Duration, Utc};
use plugins_core::*;

#[derive(std::fmt::Debug, PartialEq)]
pub struct MockAgent {
    pub last_action: Option<i32>,
    pub last_forced: Option<Duration>,
}

impl AgentTrait for MockAgent {
    fn do_action(&mut self, _data: &SensorData) -> Option<Payload> {
        self.last_action = Some(1);
        None
    }

    fn do_force(&mut self, _until: DateTime<Utc>) {
        // no-op
    }

    fn deserialize(&self) -> AgentConfig {
        AgentConfig {
            domain: "Mock".to_owned(),
            name: "MockAgent".to_string(), 
            state_json: "{}".to_string(),
        }
    }
    
    fn state(&self) -> AgentState {
        AgentState::Active
    }
}
