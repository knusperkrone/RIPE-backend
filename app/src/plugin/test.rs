use std::collections::HashMap;

use chrono::Duration;
use iftem_core::*;

#[derive(std::fmt::Debug, PartialEq)]
pub struct MockAgent {
    pub last_action: Option<i32>,
    pub last_forced: Option<Duration>,
}

impl MockAgent {
    pub fn new() -> Self {
        MockAgent {
            last_action: None,
            last_forced: None,
        }
    }
}

impl AgentTrait for MockAgent {
    fn on_data(&mut self, _data: &SensorDataMessage) {
        // no-op
    }

    fn on_cmd(&mut self, _payload: i64) {
        // no-op
    }

    fn deserialize(&self) -> AgentConfig {
        AgentConfig {
            name: "MockAgent".to_string(),
            state_json: "{}".to_string(),
        }
    }

    fn cmd(&self) -> i32 {
        0
    }

    fn state(&self) -> AgentState {
        AgentState::Ready
    }

    fn render_ui(&self, data: &SensorDataMessage) -> AgentUI {
        AgentUI {
            decorator: AgentUIDecorator::Slider(0.0, 1.0, 0.5),
            rendered: format!("Last tested at {}", data.timestamp),
            state: AgentState::default(),
        }
    }

    fn config(&self) -> HashMap<&str, (&str, AgentConfigType)> {
        HashMap::new()
    }

    fn on_config(&mut self, _: &HashMap<String, AgentConfigType>) -> bool {
        true
    }
}
