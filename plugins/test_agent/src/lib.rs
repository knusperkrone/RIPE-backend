#[macro_use]
extern crate slog;

use std::pin::Pin;
use std::sync::{
    atomic::{AtomicI32, Ordering},
    Arc,
};

use chrono::{DateTime, Utc};
use iftem_core::*;
use tokio::sync::mpsc::Sender;

const NAME: &str = "TestAgent";
const VERSION_CODE: u32 = 1;

export_plugin!(NAME, VERSION_CODE, build_agent);

#[allow(improper_ctypes_definitions)]
extern "C" fn build_agent(
    _config: Option<&std::string::String>,
    logger: slog::Logger,
    sender: Sender<AgentMessage>,
) -> Box<dyn AgentTrait> {
    send_payload(
        &logger,
        &sender,
        AgentMessage::OneshotTask(Box::new(TestFutBuilder {
            is_oneshot: true,
            sender: sender.clone(),
            logger: logger.clone(),
            counter: Arc::new(AtomicI32::new(5)),
        })),
    );

    // let repeat_sender = sender.clone();
    send_payload(
        &logger.clone(),
        &sender.clone(),
        AgentMessage::RepeatedTask(
            std::time::Duration::from_secs(1),
            Box::new(TestFutBuilder {
                is_oneshot: false,
                sender: sender.clone(),
                logger: logger.clone(),
                counter: Arc::new(AtomicI32::new(5)),
            }),
        ),
    );

    Box::new(TestAgent {
        logger: logger.clone(),
        sender: sender.clone(),
    })
}

#[derive(Debug)]
struct TestAgent {
    logger: slog::Logger,
    sender: Sender<AgentMessage>,
}

struct TestFutBuilder {
    is_oneshot: bool,
    logger: slog::Logger,
    counter: Arc<AtomicI32>,
    sender: Sender<AgentMessage>,
}

impl FutBuilder for TestFutBuilder {
    fn build_future(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = bool> + Send + Sync + 'static>> {
        let logger = self.logger.clone();
        if self.is_oneshot {
            let oneshot_sender = self.sender.clone();
            std::boxed::Box::pin(async move {
                if let Ok(_) = oneshot_sender.clone().send(AgentMessage::Command(1)).await {
                    info!(logger, "SENT MESSAGE");
                }

                info!(logger, "TASK IS SLEEPING");
                delay_task_for(std::time::Duration::from_secs(5)).await;
                info!(logger, "TASK IS AWAKE");
                true
            })
        } else {
            let counter = self.counter.clone();
            std::boxed::Box::pin(async move {
                info!(logger, "REPEATING {}", counter.load(Ordering::Relaxed));
                let i = counter.fetch_sub(1, Ordering::Relaxed);

                i == 0
            })
        }
    }
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
            decorator: AgentUIDecorator::Slider(0.0, 1.0),
            rendered: "TEXT".to_owned(),
            state: self.state(),
        }
    }

    fn deserialize(&self) -> AgentConfig {
        AgentConfig {
            name: NAME.to_owned(),
            state_json: "".to_owned(),
        }
    }

    fn state(&self) -> AgentState {
        AgentState::Default
    }

    fn cmd(&self) -> i32 {
        0
    }
}
