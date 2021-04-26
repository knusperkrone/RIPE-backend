#[macro_use]
extern crate slog;

use chrono::{DateTime, Duration, Timelike, Utc};
use iftem_core::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    pin::Pin,
    sync::{Arc, Mutex, MutexGuard, RwLock},
};
use tokio::sync::mpsc::Sender;

const NAME: &str = "TimeAgent";
const VERSION_CODE: u32 = 2;

export_plugin!(NAME, VERSION_CODE, build_agent);

/*
 * Implementation
 */

const DAY_MS: u32 = 86_400_000;

#[allow(improper_ctypes_definitions)]
unsafe extern "C" fn build_agent(
    config: Option<&str>,
    logger: slog::Logger,
    sender: Sender<AgentMessage>,
) -> Box<dyn AgentTrait> {
    let mut inner_agent = TimeAgentInner::default();
    if let Some(config_json) = config {
        if let Ok(deserialized) = serde_json::from_str::<TimeAgentInner>(&config_json) {
            info!(logger, "Restored {} from config", NAME);
            inner_agent = deserialized;
        } else {
            warn!(
                logger,
                "{} coulnd't restore from config {}", NAME, config_json
            );
        }
    }

    inner_agent.logger = logger;
    inner_agent.sender = sender;
    let agent = TimeAgent {
        inner: Arc::new(inner_agent),
    };
    agent.dispatch_task();

    Box::new(agent)
}

#[derive(Debug)]
pub struct TimeAgent {
    inner: Arc<TimeAgentInner>,
}

impl AgentTrait for TimeAgent {
    fn handle_data(&mut self, _data: &SensorDataMessage) {
        // no-op
    }

    fn handle_cmd(&mut self, payload: i64) {
        let until_opt = AgentUIDecorator::transform_cmd_timepane(payload);
        if let Some(until) = until_opt {
            if self.inner.cmd() == CMD_ACTIVE {
                self.inner.stop(until);
            } else {
                self.inner.force(until);
            }

            iftem_core::send_payload(
                &self.inner.logger,
                &self.inner.sender,
                AgentMessage::Command(self.cmd()),
            );
        } else {
            error!(self.inner.logger, "Invalid payload!");
        }
    }

    fn render_ui(&self, _: &SensorDataMessage) -> AgentUI {
        AgentUI {
            decorator: AgentUIDecorator::TimePane(60),
            state: self.state(),
            rendered: format!(
                "Von {} - {} UTC",
                secs_to_hr(self.inner.start_time_ms),
                secs_to_hr(self.inner.end_time_ms)
            ),
        }
    }

    fn cmd(&self) -> i32 {
        return self.inner.cmd();
    }

    fn deserialize(&self) -> String {
        serde_json::to_string(&(*self.inner)).unwrap()
    }

    fn state(&self) -> AgentState {
        *self.inner.last_state.read().unwrap()
    }

    fn config(&self) -> HashMap<String, (String, AgentConfigType)> {
        let mut config = HashMap::new();
        config.insert(
            "01_active".to_owned(),
            (
                "Timeragent aktiviert".to_owned(),
                AgentConfigType::Switch(true),
            ),
        );
        config.insert(
            "02_start_time".to_owned(),
            (
                "Startuhrzeit  ".to_owned(),
                AgentConfigType::IntPickerRange(
                    0,
                    24 * 4,
                    24 * 60,
                    (self.inner.duration_ms() / 1000 / 60) as i64,
                ),
            ),
        );
        config.insert(
            "03_slider".to_owned(),
            (
                "Dauer in Stunden".to_owned(),
                AgentConfigType::IntPickerRange(
                    0,
                    24 * 4,
                    24 * 60,
                    (self.inner.duration_ms() / 1000 / 60) as i64,
                ),
            ),
        );
        config
    }

    fn set_config(&mut self, values: &HashMap<String, AgentConfigType>) -> bool {
        let active;
        let time;
        let duration;
        if let AgentConfigType::Switch(val) = &values["01_active"] {
            active = *val;
        } else {
            info!(self.inner.logger, "Not active");
            return false;
        }
        if let AgentConfigType::IntSliderRange(_l, _u, val) = &values["02_start_time"] {
            time = *val;
        } else {
            info!(self.inner.logger, "No start_time");
            return false;
        }
        if let AgentConfigType::IntSliderRange(_l, _u, val) = &values["03_slider"] {
            duration = *val;
        } else {
            info!(self.inner.logger, "No slider");
            return false;
        }

        info!(
            self.inner.logger,
            "Set config: {} {} {}", active, time, duration
        );
        return true;
    }
}

impl TimeAgent {
    fn dispatch_task(&self) {
        let task_inner = self.inner.clone();
        iftem_core::send_payload(
            &self.inner.logger,
            &self.inner.sender,
            AgentMessage::RepeatedTask(
                std::time::Duration::from_secs(1),
                Box::new(TickerFutBuilder {
                    inner: task_inner.clone(),
                }),
            ),
        );
    }
}

struct TickerFutBuilder {
    inner: Arc<TimeAgentInner>,
}

impl FutBuilder for TickerFutBuilder {
    fn build_future(
        &self,
        _runtime: tokio::runtime::Handle,
    ) -> Pin<Box<dyn std::future::Future<Output = bool> + Send + Sync + 'static>> {
        let inner = self.inner.clone();
        Box::pin(async move {
            inner.tick();
            false
        })
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TimeAgentInner {
    #[serde(skip, default = "iftem_core::logger_sentinel")]
    logger: slog::Logger,
    #[serde(skip, default = "iftem_core::sender_sentinel")]
    sender: Sender<AgentMessage>,
    #[serde(skip, default)]
    last_state: RwLock<AgentState>,
    #[serde(skip, default)]
    last_command_mtx: Mutex<i32>,
    start_time_ms: u32,
    end_time_ms: u32,
}

impl Default for TimeAgentInner {
    fn default() -> Self {
        TimeAgentInner {
            logger: iftem_core::logger_sentinel(),
            sender: iftem_core::sender_sentinel(),
            last_state: RwLock::new(AgentState::default()),
            last_command_mtx: Mutex::new(CMD_INACTIVE),
            start_time_ms: 0,
            end_time_ms: 0,
        }
    }
}

impl TimeAgentInner {
    fn tick(&self) {
        let curr_state = *self.last_state.read().unwrap();
        if let AgentState::Executing(until) | AgentState::Forced(until) = curr_state {
            if until > Utc::now() {
                return; // still forced
            }
        } else if let AgentState::Stopped(until) = curr_state {
            if until > Utc::now() {
                return; // still paused
            }
        }

        return if self.now_in_range() {
            let guard = self.last_command_mtx.lock().unwrap();
            self.set_state(guard, AgentState::Executing(self.until()));
        } else {
            let guard = self.last_command_mtx.lock().unwrap();
            self.set_state(guard, AgentState::Ready);
        };
    }

    fn force(&self, until: DateTime<Utc>) {
        let guard = self.last_command_mtx.lock().unwrap();
        let curr_state = *self.last_state.read().unwrap();

        if let AgentState::Executing(_) | AgentState::Ready = curr_state {
            self.set_state(guard, AgentState::Forced(until));
        } else if let AgentState::Stopped(_) = curr_state {
            if self.now_in_range() {
                self.set_state(guard, AgentState::Executing(self.until()));
            } else {
                self.set_state(guard, AgentState::Ready);
            }
        }
    }

    fn stop(&self, until: DateTime<Utc>) {
        let guard = self.last_command_mtx.lock().unwrap();
        let curr_state = *self.last_state.read().unwrap();

        if let AgentState::Executing(_) = curr_state {
            self.set_state(guard, AgentState::Stopped(until));
        } else if let AgentState::Forced(_) = curr_state {
            if self.now_in_range() {
                self.set_state(guard, AgentState::Executing(self.until()));
            } else {
                self.set_state(guard, AgentState::Ready);
            }
        }
    }

    /*
     * getter/setter
     */

    fn set_state(&self, mut last_cmd_guard: MutexGuard<i32>, state: AgentState) {
        let last_cmd = *last_cmd_guard;
        *last_cmd_guard = match state {
            AgentState::Executing(_) | AgentState::Forced(_) => CMD_ACTIVE,
            _ => CMD_INACTIVE,
        };
        *self.last_state.write().unwrap() = state;

        if last_cmd != *last_cmd_guard {
            send_payload(
                &self.logger,
                &self.sender,
                AgentMessage::Command(CMD_ACTIVE),
            );
        }
    }

    fn cmd(&self) -> i32 {
        *self.last_command_mtx.lock().unwrap()
    }

    fn until(&self) -> DateTime<Utc> {
        let now = Utc::now();
        let mut duration: u32 = 0;

        let mut passed_ms = now.num_seconds_from_midnight() * 1000;
        let is_next_day = self.end_time_ms <= self.start_time_ms;
        if is_next_day {
            if passed_ms > self.start_time_ms {
                duration += DAY_MS - passed_ms;
                passed_ms = 0
            }
        }

        // fix arithmetic panic
        if passed_ms < self.end_time_ms {
            duration += self.end_time_ms - passed_ms;
        }

        now + Duration::milliseconds(duration as i64)
    }

    fn duration_ms(&self) -> u32 {
        let is_next_day = self.end_time_ms < self.start_time_ms;
        if is_next_day {
            DAY_MS - self.start_time_ms + self.start_time_ms
        } else {
            self.end_time_ms - self.start_time_ms
        }
    }

    fn now_in_range(&self) -> bool {
        // Transform Utc::now to ms of current day
        let now = Utc::now();
        let currtime_ms = now.num_seconds_from_midnight() * 1000; // Secs

        // Check if thresholds are skipping a day and modulo check range
        let is_next_day = self.end_time_ms < self.start_time_ms;
        return if is_next_day {
            currtime_ms > self.start_time_ms || currtime_ms < self.end_time_ms
        } else {
            currtime_ms > self.start_time_ms && currtime_ms < self.end_time_ms
        };
    }
}
