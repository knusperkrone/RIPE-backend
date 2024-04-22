use std::collections::HashMap;

use chrono::Duration;
use chrono_tz::Tz;
use ripe_core::*;

#[derive(std::fmt::Debug)]
pub struct MockAgent {
    _sender: AgentStreamSender,
    pub last_action: Option<i32>,
    pub last_forced: Option<Duration>,
}

impl MockAgent {
    pub fn new(sender: AgentStreamSender) -> Self {
        MockAgent {
            _sender: sender,
            last_action: None,
            last_forced: None,
        }
    }
}

impl AgentTrait for MockAgent {
    fn init(&mut self) {}

    fn handle_data(&mut self, _data: &SensorDataMessage) {
        //
    }

    fn handle_cmd(&mut self, _payload: i64) {
        // self.sender.send(AgentMessage::Command(0));
    }

    fn deserialize(&self) -> String {
        "{}".to_string()
    }

    fn cmd(&self) -> i32 {
        0
    }

    fn state(&self) -> AgentState {
        AgentState::Ready
    }

    fn render_ui(&self, data: &SensorDataMessage, timezone: Tz) -> AgentUI {
        AgentUI {
            decorator: AgentUIDecorator::Slider(0.0, 1.0, 0.5),
            rendered: format!(
                "Last tested at {} with timezone {}",
                data.timestamp, timezone
            ),
            state: AgentState::default(),
        }
    }

    fn config(&self, _timezone: Tz) -> HashMap<String, (String, AgentConfigType)> {
        HashMap::new()
    }

    fn set_config(&mut self, _: &HashMap<String, AgentConfigType>, _: Tz) -> bool {
        true
    }
}

#[tokio::test]
async fn test_plugin_iac_channel() {
    use super::*;
    use tokio::sync::mpsc::unbounded_channel;

    let (mqtt_sender, _mqtt_receiver) = unbounded_channel();
    let factory = AgentFactory::new(mqtt_sender);

    let mut agent = factory
        .create_agent(1, &"MockAgent", &"TEST", None)
        .unwrap();

    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    agent.handle_cmd(0);
}
