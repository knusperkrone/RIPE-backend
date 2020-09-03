use super::*;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct AgentConfig {
    pub name: String,
    pub state_json: String,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct AgentUI {
    pub decorator: AgentUIDecorator,
    pub state: AgentState,
    pub rendered: String,
    // pub config_rendered: String
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub enum AgentUIDecorator {
    TimedButton(u32), // ui_stepsize
    Slide(f32, f32),  // Range
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

pub trait AgentTrait: std::fmt::Debug + Send {
    fn do_action(&mut self, data: &SensorDataMessage);
    fn do_force(&mut self, active: bool, until: DateTime<Utc>);

    fn on_force(&mut self, active: bool, duration: Duration) {
        let until = Utc::now() + duration;
        self.do_force(active, until);
    }

    fn on_data(&mut self, data: &SensorDataMessage) {
        if AgentState::Active == *self.state() {
            self.do_action(data);
        }
    }

    fn cmd(&self) -> i32;
    fn render_ui(&self, data: &SensorDataMessage) -> AgentUI;
    fn deserialize(&self) -> AgentConfig;
    fn state(&self) -> &AgentState;
}
