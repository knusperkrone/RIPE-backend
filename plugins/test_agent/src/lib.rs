use chrono_tz::Tz;
use ripe_core::*;
use std::sync::{
    atomic::{AtomicI32, Ordering},
    Arc,
};
use std::{collections::HashMap, pin::Pin};
use tracing::{error, info};

const NAME: &str = "TestAgent";
const VERSION_CODE: u32 = 2;

export_plugin!(NAME, VERSION_CODE, build_agent);

/*
 * Implementation
 */
#[no_mangle]
extern "Rust" fn build_agent(
    _config: Option<&str>,
    sender: AgentStreamSender,
) -> Box<dyn AgentTrait> {
    Box::new(TestAgent {
        val: 0.5,
        sender: Arc::new(sender),
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
    sender: Arc<AgentStreamSender>,
    config_active: bool,
    config_daytime_ms: u64,
    config_time_slider_hour: i64,
    config_time_slider_min: i64,
    config_int_slider: i64,
}

struct TestFutBuilder {
    is_oneshot: bool,
    counter: Arc<AtomicI32>,
    sender: Arc<AgentStreamSender>,
}

impl FutBuilder for TestFutBuilder {
    fn build_future(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = bool> + Send + Sync + 'static>> {
        let sender = self.sender.clone();
        if self.is_oneshot {
            std::boxed::Box::pin(async move {
                error!("TASK IS SLEEPING");
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;

                error!("TASK IS AWAKE");
                sender.send(AgentMessage::Command(1));

                true
            })
        } else {
            let counter = self.counter.clone();
            std::boxed::Box::pin(async move {
                let count = counter.load(Ordering::Relaxed);
                error!("REPEATING {}", count);
                let i = counter.fetch_sub(1, Ordering::Relaxed);

                sender.send(AgentMessage::Command(count));
                i == 0
            })
        }
    }
}

impl AgentTrait for TestAgent {
    fn init(&mut self) {
        /*
        if let Err(e) = sender.try_send(AgentMessage::Command(0)) {
            panic!("TestAgent - failed to send Command {}", e);
        } else if let Err(e) = sender.try_send(AgentMessage::Command(1)) {
            panic!("TestAgent - failed to send Command {}", e);
        } else if lett Err(e) = sender.try_send(AgentMessage::OneshotTask(Box::new(TestFutBuilder {
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
                counter: Arc::new(AtomicI32::new(5)),
            }),
        )) {
            panic!("TestAgent - failed to send RepeatedTask {}", e);
        }
        */

        let sender_arc = self.sender.clone();
        sender_arc.clone().send(AgentMessage::RepeatedTask(
            std::time::Duration::from_secs(1),
            Box::new(TestFutBuilder {
                is_oneshot: false,
                sender: sender_arc.clone(),
                counter: Arc::new(AtomicI32::new(5)),
            }),
        ));
        sender_arc
            .clone()
            .send(AgentMessage::OneshotTask(Box::new(TestFutBuilder {
                is_oneshot: true,
                sender: sender_arc.clone(),
                counter: Arc::new(AtomicI32::new(5)),
            })));
    }

    fn handle_data(&mut self, _data: &SensorDataMessage) {
        info!("Handle data called");
    }

    fn handle_cmd(&mut self, payload: i64) {
        let val = AgentUIDecorator::transform_cmd_slider(payload);
        if val >= 0.0 && val <= 1.0 {
            self.val = val;
            //  info!(self.logger, "Received cmd val: {}", self.val);
        }
    }

    fn render_ui(&self, _data: &SensorDataMessage, timezone: Tz) -> AgentUI {
        AgentUI {
            decorator: AgentUIDecorator::Slider(0.0, 1.0, self.val),
            rendered: format!("Server rendered text, timezone: {}", timezone),
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

    fn config(&self, _timezone: Tz) -> HashMap<String, (String, AgentConfigType)> {
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

    fn set_config(&mut self, values: &HashMap<String, AgentConfigType>, _timezone: Tz) -> bool {
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
        /*
        info!(
            self.logger,
            "Set config: {}, {}, {}, {}, {}",
            active,
            daytime_ms,
            time_slider_hour,
            time_slider_min,
            int_slider
        );
         */
        true
    }
}
