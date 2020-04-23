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

pub fn send_payload<'a>(
    logger: &'a slog::Logger,
    sender: &'a tokio::sync::mpsc::Sender<AgentPayload>,
    payload: AgentPayload,
) {
    let task_logger = logger.clone();
    let mut task_sender = sender.clone();

    tokio::spawn(async move {
        let mut trie: i32 = 0;
        while let Err(e) = task_sender.send(payload).await {
            if trie >= 3 {
                error!(task_logger, "Failed sending {:?}", payload);
                return;
            }

            warn!(task_logger, "Retrying send {:?}, due: {}", payload, e);
            tokio::time::delay_for(std::time::Duration::from_millis(100)).await;
            trie += 1;
        }
    });
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
