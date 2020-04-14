use chrono::{DateTime, Duration, Utc};

use super::*;

#[derive(Debug, PartialEq)]
pub struct AgentConfig {
    pub domain: String,
    pub name: String,
    pub state_json: String,
}

#[derive(PartialEq, std::fmt::Debug)]
pub enum AgentState {
    Active,
    Forced(DateTime<Utc>),
}

impl Default for AgentState {
    fn default() -> Self {
        AgentState::Active
    }
}

pub trait AgentTrait: std::fmt::Debug + Send {
    fn do_action(&mut self, data: &SensorData) -> Option<Payload>;
    fn do_force(&mut self, until: DateTime<Utc>);

    fn on_force(&mut self, duration: Duration) {
        let until = Utc::now() + duration;
        self.do_force(until);
    }
    fn on_data(&mut self, data: &SensorData) -> Option<Payload> {
        match self.state() {
            AgentState::Active => self.do_action(data),
            AgentState::Forced(_) => None,
        }
    }

    fn deserialize(&self) -> AgentConfig;
    fn state(&self) -> AgentState;
}
