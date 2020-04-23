use super::*;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct AgentConfig {
    pub name: String,
    pub state_json: String,
}

#[derive(PartialEq, Debug, Deserialize, Serialize, Clone, Copy)]
pub enum AgentState {
    Active,
    Default,
    Forced(DateTime<Utc>),
}

impl Default for AgentState {
    fn default() -> Self {
        AgentState::Active
    }
}

pub fn send_payload(
    logger: &slog::Logger,
    sender: &tokio::sync::mpsc::Sender<AgentMessage>,
    payload: AgentMessage,
) {
    let task_logger = logger.clone();
    let mut task_sender = sender.clone();
    if let Err(e) = task_sender.try_send(payload) {
        error!(task_logger, "Failed sending {}", e);
    }
}

pub trait AgentTrait: std::fmt::Debug + Send {
    fn do_action(&mut self, data: &SensorDataDto);
    fn do_force(&mut self, active: bool, until: DateTime<Utc>);

    fn on_force(&mut self, active: bool, duration: Duration) {
        let until = Utc::now() + duration;
        self.do_force(active, until);
    }

    fn on_data(&mut self, data: &SensorDataDto) {
        if AgentState::Active == *self.state() {
            self.do_action(data);
        }
    }

    fn deserialize(&self) -> AgentConfig;
    fn state(&self) -> &AgentState;
}
