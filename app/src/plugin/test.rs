use std::collections::HashMap;

use chrono::Duration;
use ripe_core::*;

use super::*;

#[derive(std::fmt::Debug)]
pub struct MockAgent {
    sender: Sender<AgentMessage>,
    pub last_action: Option<i32>,
    pub last_forced: Option<Duration>,
}

impl MockAgent {
    pub fn new(sender: Sender<AgentMessage>) -> Self {
        MockAgent {
            sender,
            last_action: None,
            last_forced: None,
        }
    }
}

impl AgentTrait for MockAgent {
    fn handle_data(&mut self, _data: &SensorDataMessage) {
        //
    }

    fn handle_cmd(&mut self, _payload: i64) {
        if let Err(e) = self.sender.try_send(AgentMessage::Command(0)) {
            panic!("{}", e);
        }
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

    fn render_ui(&self, data: &SensorDataMessage) -> AgentUI {
        AgentUI {
            decorator: AgentUIDecorator::Slider(0.0, 1.0, 0.5),
            rendered: format!("Last tested at {}", data.timestamp),
            state: AgentState::default(),
        }
    }

    fn config(&self) -> HashMap<String, (String, AgentConfigType)> {
        HashMap::new()
    }

    fn set_config(&mut self, _: &HashMap<String, AgentConfigType>) -> bool {
        true
    }
}

#[tokio::test]
async fn test_plugin_iac_channel() {
    use tokio::sync::mpsc::unbounded_channel;

    let (mqtt_sender, _mqtt_receiver) = unbounded_channel();
    let factory = AgentFactory::new(mqtt_sender);

    let mut agent = factory
        .create_agent(1, &"MockAgent", &"TEST", None)
        .unwrap();

    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    agent.handle_cmd(0);
}