#[macro_use]
extern crate slog;

use std::sync::{
    atomic::{AtomicI32, Ordering},
    Arc,
};
use std::{collections::HashMap, pin::Pin};

use ripe_core::*;
use tokio::sync::mpsc::Sender;

const NAME: &str = "TestAgent";
const VERSION_CODE: u32 = 2;

export_plugin!(NAME, VERSION_CODE, build_agent);

/*
 * Implementation
 */

#[allow(improper_ctypes_definitions)]
unsafe extern "C" fn build_agent(
    _config: Option<&str>,
    logger: slog::Logger,
    sender: Sender<AgentMessage>,
) -> Box<dyn AgentTrait> {
    if let Err(e) = sender.try_send(AgentMessage::Command(0)) {
        panic!("TestAgent - failed to send Command {}", e);
    } else if let Err(e) = sender.try_send(AgentMessage::OneshotTask(Box::new(TestFutBuilder {
        is_oneshot: true,
        sender: sender.clone(),
        logger: logger.clone(),
        counter: Arc::new(AtomicI32::new(5)),
    }))) {
        panic!("TestAgent - failed to send OneshotTask {}", e);
    } else if let Err(e) = sender.try_send(AgentMessage::RepeatedTask(
        std::time::Duration::from_secs(1),
        Box::new(TestFutBuilder {
            is_oneshot: false,
            sender: sender.clone(),
            logger: logger.clone(),
            counter: Arc::new(AtomicI32::new(2)),
        }),
    )) {
        panic!("TestAgent - failed to send RepeatedTask {}", e);
    }

    Box::new(TestAgent {
        val: 0.5,
        logger: logger,
        sender: sender,
        config_active: true,
        config_daytime_ms: 0,
        config_time_slider_hour: 0,
        config_time_slider_min: 0,
        config_int_slider: 0,
    })
}

#[derive(Debug)]
struct TestAgent {
    val: f32,
    logger: slog::Logger,
    sender: Sender<AgentMessage>,
    config_active: bool,
    config_daytime_ms: u64,
    config_time_slider_hour: i64,
    config_time_slider_min: i64,
    config_int_slider: i64,
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
                info!(logger, "TASK IS SLEEPING");
                ripe_core::sleep(&runtime, std::time::Duration::from_secs(5)).await;
                info!(logger, "TASK IS AWAKE");

                if let Ok(_) = oneshot_sender.clone().try_send(AgentMessage::Command(1)) {
                    info!(logger, "SENT MESSAGE");
                } else {
                    error!(logger, "FAILED SENDING 1");
                }

                true
            })
        } else {
            let counter = self.counter.clone();
            std::boxed::Box::pin(async move {
                let _guard = runtime.enter();
                ripe_core::sleep(&runtime, std::time::Duration::from_secs(1)).await;
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
            rendered: "Server rendered text".to_owned(),
            state: self.state(),
        }
    }

    fn deserialize(&self) -> String {
        "{}".to_owned()
    }

    fn state(&self) -> AgentState {
        AgentState::Ready
    }

    fn cmd(&self) -> i32 {
        CMD_INACTIVE
    }

    fn config(&self) -> HashMap<String, (String, AgentConfigType)> {
        let mut config = HashMap::new();
        config.insert(
            "01_active".to_owned(),
            (
                "TestSwitch".to_owned(),
                AgentConfigType::Switch(self.config_active),
            ),
        );
        config.insert(
            "02_day_time".to_owned(),
            (
                "TestDateTime".to_owned(),
                AgentConfigType::DayTime(self.config_daytime_ms),
            ),
        );
        config.insert(
            "03_time_slider_hour".to_owned(),
            (
                "TimeSlider in Hours".to_owned(),
                AgentConfigType::TimeSlider(
                    0,
                    DAY_MS as i64,
                    self.config_time_slider_hour,
                    24 * 12,
                ),
            ),
        );
        config.insert(
            "04_time_slider_minute".to_owned(),
            (
                "TimeSlider in Minutes".to_owned(),
                AgentConfigType::TimeSlider(
                    0,
                    (DAY_MS / 24) as i64,
                    self.config_time_slider_min,
                    120,
                ),
            ),
        );
        config.insert(
            "05_int_slider".to_owned(),
            (
                "IntSlider".to_owned(),
                AgentConfigType::IntSlider(0, 1024, self.config_int_slider),
            ),
        );
        config
    }

    fn set_config(&mut self, values: &HashMap<String, AgentConfigType>) -> bool {
        let active;
        let daytime_ms;
        let time_slider_hour;
        let time_slider_min;
        let int_slider;
        if let AgentConfigType::Switch(val) = &values["01_active"] {
            active = *val;
        } else {
            return false;
        }
        if let AgentConfigType::DayTime(val) = &values["02_day_time"] {
            daytime_ms = *val;
        } else {
            return false;
        }
        if let AgentConfigType::TimeSlider(_l, _u, val, _s) = &values["03_time_slider_hour"] {
            time_slider_hour = *val;
        } else {
            return false;
        }
        if let AgentConfigType::TimeSlider(_l, _u, val, _s) = &values["04_time_slider_minute"] {
            time_slider_min = *val;
        } else {
            return false;
        }
        if let AgentConfigType::IntSlider(_l, _u, val) = &values["05_int_slider"] {
            int_slider = *val;
        } else {
            return false;
        }

        self.config_active = active;
        self.config_daytime_ms = daytime_ms;
        self.config_time_slider_hour = time_slider_hour;
        self.config_time_slider_min = time_slider_min;
        self.config_int_slider = int_slider;
        info!(
            self.logger,
            "Set config: {}, {}, {}, {}, {}",
            active,
            daytime_ms,
            time_slider_hour,
            time_slider_min,
            int_slider
        );
        true
    }
}
