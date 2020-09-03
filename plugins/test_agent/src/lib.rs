#[macro_use]
extern crate slog;

use chrono::{DateTime, Utc};
use iftem_core::*;
use tokio::sync::mpsc::Sender;

const NAME: &str = "TestAgent";
const VERSION_CODE: u32 = 1;

export_plugin!(NAME, VERSION_CODE, build_agent);

extern "C" fn build_agent(
    _config: Option<&std::string::String>,
    logger: slog::Logger,
    sender: Sender<AgentMessage>,
) -> Box<dyn AgentTrait> {
    let agent_logger = logger.clone();
    let agent_sender = sender.clone();

    send_payload(
        &logger.clone(),
        &sender.clone(),
        AgentMessage::Task(std::boxed::Box::pin(async move {
            if let Ok(_) = sender
                .clone()
                .send(AgentMessage::State(AgentState::Active))
                .await
            {
                info!(logger, "SENT MESSAGE");
            }

            info!(logger, "TASK IS SLEEPING");
            delay_task_for(std::time::Duration::from_secs(20)).await;
            info!(logger, "TASK IS AWAKE");
        })),
    );

    let agent = TestAgent {
        logger: agent_logger.clone(),
        sender: agent_sender.clone(),
    };
    Box::new(agent)
}

#[derive(Debug)]
struct TestAgent {
    logger: slog::Logger,
    sender: Sender<AgentMessage>,
}

impl AgentTrait for TestAgent {
    fn do_action(&mut self, _data: &SensorDataMessage) {
        // todo!()
    }

    fn do_force(&mut self, _active: bool, _until: DateTime<Utc>) {
        // todo!()
    }

    fn render_ui(&self, _data: &SensorDataMessage) -> AgentUI {
        AgentUI {
            decorator: AgentUIDecorator::Slide(0.0, 1.0),
            rendered: "TEXT".to_owned(),
            state: *self.state(),
        }
    }

    fn deserialize(&self) -> AgentConfig {
        AgentConfig {
            name: NAME.to_owned(),
            state_json: "".to_owned(),
        }
    }

    fn state(&self) -> &AgentState {
        &AgentState::Default
    }

    fn cmd(&self) -> i32 {
        0
    }
}
