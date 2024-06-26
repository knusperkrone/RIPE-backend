use chrono_tz::Tz;
use ripe_core::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{info, error};

const NAME: &str = "PercentAgent";
const VERSION_CODE: u32 = 2;

export_plugin!(NAME, VERSION_CODE, build_agent);

/*
 * Implementation
 */
#[no_mangle]
extern "Rust" fn build_agent(
    config: Option<&str>,
    sender: AgentStreamSender,
) -> Box<dyn AgentTrait> {
    let mut agent = PercentAgent::default();
    if let Some(config_json) = config {
        if let Ok(deserialized) = serde_json::from_str(&config_json) {
            agent = deserialized;
        }
    }

    Box::new(PercentAgent { sender, ..agent })
}

#[derive(Debug, Deserialize, Serialize)]
struct PercentAgent {
    val: i32,
    #[serde(skip, default = "ripe_core::sender_sentinel")]
    sender: AgentStreamSender,
}

impl Default for PercentAgent {
    fn default() -> Self {
        PercentAgent {
            val: 50,
            sender: ripe_core::sender_sentinel(),
        }
    }
}

impl AgentTrait for PercentAgent {
    fn init(&mut self) {}

    fn handle_data(&mut self, _data: &SensorDataMessage) {
        // no-op
    }

    fn handle_cmd(&mut self, mut payload: i64) {
        payload /= 10;
        if payload < 0 || payload > 100 {
            error!("Invalid command {}", payload);
            return;
        }

        self.val = payload as i32;

        info!("Transient set command to: {}", self.val);
        self.sender.send(AgentMessage::Command(self.val));
    }

    fn render_ui(&self, _data: &SensorDataMessage, _timezone: Tz) -> AgentUI {
        let percent = self.val as f32 / 100.0;
        AgentUI {
            decorator: AgentUIDecorator::Slider(0.0, 1.0, percent),
            rendered: format!("{}%", self.val),
            state: self.state(),
        }
    }

    fn state(&self) -> AgentState {
        AgentState::Ready
    }

    fn cmd(&self) -> i32 {
        self.val
    }

    fn deserialize(&self) -> String {
        serde_json::to_string(self).unwrap()
    }

    fn config(&self, _timezone: Tz) -> HashMap<String, (String, AgentConfigType)> {
        let mut config = HashMap::new();
        config.insert(
            "01_active".to_owned(),
            ("TestSwitch".to_owned(), AgentConfigType::Switch(true)),
        );
        config
    }

    fn set_config(&mut self, values: &HashMap<String, AgentConfigType>, _timezone: Tz) -> bool {
        let active;
        if let AgentConfigType::Switch(val) = &values["01_active"] {
            active = *val;
        } else {
            return false;
        }

        if !active {
            self.val = 0;
            self.sender.send(AgentMessage::Command(self.val));
        }
        true
    }
}
