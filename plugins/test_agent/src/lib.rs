#[macro_use]
extern crate slog;

use std::sync::{
    atomic::{AtomicI32, Ordering},
    Arc,
};
use std::{collections::HashMap, pin::Pin};

use iftem_core::*;
use tokio::sync::mpsc::Sender;

const NAME: &str = "TestAgent";
const VERSION_CODE: u32 = 2;

export_plugin!(NAME, VERSION_CODE, build_agent);

#[allow(improper_ctypes_definitions)]
extern "C" fn build_agent(
    _config: Option<&str>,
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

    send_payload(
        &logger,
        &sender,
        AgentMessage::RepeatedTask(
            std::time::Duration::from_secs(1),
            Box::new(TestFutBuilder {
                is_oneshot: false,
                sender: sender.clone(),
                logger: logger.clone(),
                counter: Arc::new(AtomicI32::new(2)),
            }),
        ),
    );

    Box::new(TestAgent {
        val: 0.5,
        logger: logger,
        sender: sender,
    })
}

#[derive(Debug)]
struct TestAgent {
    val: f32,
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
        runtime: tokio::runtime::Handle,
    ) -> Pin<Box<dyn std::future::Future<Output = bool> + Send + Sync + 'static>> {
        let logger = self.logger.clone();
        if self.is_oneshot {
            let oneshot_sender = self.sender.clone();
            std::boxed::Box::pin(async move {
                let _guard = runtime.enter();
                if let Ok(_) = oneshot_sender.clone().try_send(AgentMessage::Command(1)) {
                    info!(logger, "SENT MESSAGE");
                } else {
                    error!(logger, "FAILED SENDING 1");
                }

                info!(logger, "TASK IS SLEEPING");
                iftem_core::sleep(&runtime, std::time::Duration::from_secs(10)).await;
                info!(logger, "TASK IS AWAKE");
                true
            })
        } else {
            let counter = self.counter.clone();
            std::boxed::Box::pin(async move {
                let _guard = runtime.enter();
                info!(logger, "REPEATING {}", counter.load(Ordering::Relaxed));
                let i = counter.fetch_sub(1, Ordering::Relaxed);

                i == 0
            })
        }
    }
}

impl AgentTrait for TestAgent {
    fn handle_data(&mut self, data: &SensorDataMessage) {
        info!(self.logger, "Received data: {:?}", data);
    }

    fn handle_cmd(&mut self, payload: i64) {
        let val = AgentUIDecorator::transform_cmd_slider(payload);
        if val >= 0.0 && val <= 1.0 {
            self.val = val;
            info!(self.logger, "Received cmd val: {}", self.val);
        }
    }

    fn render_ui(&self, _data: &SensorDataMessage) -> AgentUI {
        AgentUI {
            decorator: AgentUIDecorator::Slider(0.0, 1.0, self.val),
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
        AgentState::Ready
    }

    fn cmd(&self) -> i32 {
        CMD_INACTIVE
    }

    fn config(&self) -> HashMap<&str, (&str, AgentConfigType)> {
        let mut config = HashMap::new();
        config.insert("active", ("TestSwitch", AgentConfigType::Switch(true)));
        config.insert("time", ("TestDateTime", AgentConfigType::DateTime(36000)));
        config.insert(
            "slider",
            (
                "TestSliderRange",
                AgentConfigType::IntSliderRange(SliderFormatter::Linear, 0, 24, 8),
            ),
        );
        config.insert(
            "slider",
            ("TestSlider", AgentConfigType::IntRange(0, 1024, 42)),
        );
        config
    }

    fn set_config(&mut self, values: &HashMap<String, AgentConfigType>) -> bool {
        let active;
        let time;
        let slider;
        if let AgentConfigType::Switch(val) = &values["active"] {
            active = *val;
        } else {
            return false;
        }
        if let AgentConfigType::DateTime(val) = &values["time"] {
            time = *val;
        } else {
            return false;
        }
        if let AgentConfigType::IntSliderRange(_l, _u, val, _f) = &values["slider"] {
            slider = *val;
        } else {
            return false;
        }

        info!(
            self.logger,
            "Set config: {}, {}, {:?}", active, time, slider
        );
        true
    }
}
