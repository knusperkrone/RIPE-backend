#[macro_use]
extern crate slog;

use chrono::{DateTime, Duration, Timelike, Utc};
use ripe_core::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    pin::Pin,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, Mutex, MutexGuard, RwLock,
    },
};
use tokio::sync::mpsc::Sender;

const NAME: &str = "TimeAgent";
const VERSION_CODE: u32 = 2;

export_plugin!(NAME, VERSION_CODE, build_agent);

/*
 * Implementation
 */

#[allow(improper_ctypes_definitions)]
unsafe extern "C" fn build_agent(
    config: Option<&str>,
    logger: slog::Logger,
    sender: Sender<AgentMessage>,
) -> Box<dyn AgentTrait> {
    let mut inner_agent = TimeAgentInner::default();
    if let Some(config_json) = config {
        if let Ok(deserialized) = serde_json::from_str::<TimeAgentInner>(&config_json) {
            debug!(logger, "Restored {} from config", NAME);
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
        let is_enabled = payload.is_positive();

        let until_opt = AgentUIDecorator::transform_cmd_timepane(payload);
        if let Some(until) = until_opt {
            if is_enabled {
                debug!(self.inner.logger, "Time agent received force");
                self.inner.force(until);
            } else {
                debug!(self.inner.logger, "Time agent received stop");
                self.inner.stop(until);
            }
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
                ms_to_hr(self.inner.start_time_ms()),
                ms_to_hr(self.inner.end_time_ms())
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
                AgentConfigType::DayTime(self.inner.start_time_ms() as u64),
            ),
        );
        config.insert(
            "03_slider".to_owned(),
            (
                "Dauer in Stunden".to_owned(),
                AgentConfigType::TimeSlider(
                    0,
                    DAY_MS as i64,
                    self.inner.duration_ms() as i64,
                    24 * 4, // quarter
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
            error!(self.inner.logger, "No active");
            return false;
        }
        if let AgentConfigType::DayTime(val) = &values["02_start_time"] {
            time = *val;
        } else {
            error!(self.inner.logger, "No start_time");
            return false;
        }
        if let AgentConfigType::TimeSlider(_l, _u, val, _s) = &values["03_slider"] {
            duration = *val;
        } else {
            error!(self.inner.logger, "No slider");
            return false;
        }

        self.inner.set_config(time as u32, duration as u32);
        info!(
            self.inner.logger,
            "Set config: {} {} {}", active, time, duration,
        );
        return true;
    }
}

impl TimeAgent {
    fn dispatch_task(&self) {
        let task_inner = self.inner.clone();
        ripe_core::send_payload(
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
    #[serde(skip, default = "ripe_core::logger_sentinel")]
    logger: slog::Logger,
    #[serde(skip, default = "ripe_core::sender_sentinel")]
    sender: Sender<AgentMessage>,
    #[serde(skip, default)]
    last_state: RwLock<AgentState>,
    #[serde(skip, default)]
    last_command_mtx: Mutex<i32>,
    start_time_ms: AtomicU32,
    end_time_ms: AtomicU32,
}

impl Default for TimeAgentInner {
    fn default() -> Self {
        TimeAgentInner {
            logger: ripe_core::logger_sentinel(),
            sender: ripe_core::sender_sentinel(),
            last_state: RwLock::new(AgentState::default()),
            last_command_mtx: Mutex::new(CMD_INACTIVE),
            start_time_ms: AtomicU32::new(0),
            end_time_ms: AtomicU32::new(0),
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

        if let AgentState::Executing(_) = curr_state {
            // no-op
        } else if let AgentState::Forced(_) | AgentState::Ready = curr_state {
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

    fn set_config(&self, start_time_ms: u32, duration_ms: u32) {
        self.start_time_ms.store(start_time_ms, Ordering::Relaxed);
        let mut end_time_ms = start_time_ms + duration_ms;
        if end_time_ms >= DAY_MS {
            end_time_ms -= DAY_MS;
        }
        self.end_time_ms.store(end_time_ms, Ordering::Relaxed);
    }

    fn set_state(&self, mut last_cmd_guard: MutexGuard<i32>, state: AgentState) {
        let last_cmd = *last_cmd_guard;
        let curr_cmd = match state {
            AgentState::Executing(_) | AgentState::Forced(_) => CMD_ACTIVE,
            _ => CMD_INACTIVE,
        };
        *self.last_state.write().unwrap() = state;

        if last_cmd != curr_cmd {
            *last_cmd_guard = curr_cmd;
            send_payload(
                &self.logger,
                &self.sender,
                AgentMessage::Command(curr_cmd),
            );    
        }
    }

    fn cmd(&self) -> i32 {
        *self.last_command_mtx.lock().unwrap()
    }

    fn until(&self) -> DateTime<Utc> {
        let start_time_ms = self.start_time_ms();
        let end_time_ms = self.end_time_ms();
        let now = Utc::now();
        let mut duration: u32 = 0;

        let mut passed_ms = now.num_seconds_from_midnight() * 1000;
        let is_next_day = end_time_ms <= start_time_ms;
        if is_next_day {
            if passed_ms > start_time_ms {
                duration += DAY_MS - passed_ms;
                passed_ms = 0
            }
        }

        // fix arithmetic panic
        if passed_ms < end_time_ms {
            duration += end_time_ms - passed_ms;
        }

        now + Duration::milliseconds(duration as i64)
    }

    fn start_time_ms(&self) -> u32 {
        return self.start_time_ms.load(Ordering::Relaxed);
    }

    fn end_time_ms(&self) -> u32 {
        return self.end_time_ms.load(Ordering::Relaxed);
    }

    fn duration_ms(&self) -> u32 {
        let start_time_ms = self.start_time_ms();
        let end_time_ms = self.end_time_ms();

        let is_next_day = end_time_ms < start_time_ms;
        if is_next_day {
            DAY_MS - start_time_ms + end_time_ms
        } else {
            end_time_ms - start_time_ms
        }
    }

    fn now_in_range(&self) -> bool {
        // Transform Utc::now to ms of current day
        let start_time_ms = self.start_time_ms();
        let end_time_ms = self.end_time_ms();
        let now = Utc::now();
        let currtime_ms = now.num_seconds_from_midnight() * 1000;

        // Check if thresholds are skipping a day and modulo check range
        let is_next_day = end_time_ms < start_time_ms;
        return if is_next_day {
            currtime_ms > start_time_ms || currtime_ms < end_time_ms
        } else {
            currtime_ms > start_time_ms && currtime_ms < end_time_ms
        };
    }
}
