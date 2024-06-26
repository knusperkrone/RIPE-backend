use chrono::{DateTime, Duration, Timelike, Utc};
use chrono_tz::Tz;
use ripe_core::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    pin::Pin,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, RwLock,
    },
};
use tracing::{debug, error, info, warn};

mod time;
use crate::time::*;

const NAME: &str = "TimeAgent";
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
    let mut inner_agent = TimeAgentInner::default();
    if let Some(config_json) = config {
        if let Ok(deserialized) = serde_json::from_str::<TimeAgentInner>(&config_json) {
            debug!("Restored {} from config", NAME);
            inner_agent = deserialized;
        } else {
            warn!("{} coulnd't restore from config {}", NAME, config_json);
        }
    }

    Box::new(TimeAgent {
        inner: Arc::new(TimeAgentInner {
            sender: sender,
            ..inner_agent
        }),
    })
}

#[derive(Debug)]
pub struct TimeAgent {
    inner: Arc<TimeAgentInner>,
}

impl AgentTrait for TimeAgent {
    fn init(&mut self) {
        self.dispatch_task();
    }

    fn handle_data(&mut self, _data: &SensorDataMessage) {
        // no-op
    }

    fn handle_cmd(&mut self, payload: i64) {
        let is_enabled = payload.is_positive();

        let until_opt = AgentUIDecorator::transform_cmd_timepane(payload);
        if let Some(until) = until_opt {
            if is_enabled {
                info!("Time agent received force");
                self.inner.force(until);
            } else {
                info!("Time agent received stop");
                self.inner.stop(until);
            }
        } else {
            error!("Invalid payload!");
        }
    }

    fn render_ui(&self, _: &SensorDataMessage, timezone: Tz) -> AgentUI {
        let start_time_ms = ms_add_offset(self.inner.start_time_ms(), timezone);
        let end_time_ms = ms_add_offset(self.inner.end_time_ms(), timezone);
        AgentUI {
            decorator: AgentUIDecorator::TimePane(60),
            state: self.state(),
            rendered: format!(
                "Von {} - {}",
                ms_to_hr(start_time_ms as u32),
                ms_to_hr(end_time_ms as u32),
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

    fn config(&self, timezone: Tz) -> HashMap<String, (String, AgentConfigType)> {
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
                format!("Startuhrzeit ({})", timezone),
                AgentConfigType::DayTime(
                    ms_add_offset(self.inner.start_time_ms(), timezone) as u64
                ),
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

    fn set_config(&mut self, values: &HashMap<String, AgentConfigType>, timezone: Tz) -> bool {
        let active;
        let time;
        let duration;
        if let AgentConfigType::Switch(val) = &values["01_active"] {
            active = *val;
        } else {
            error!("Not active");
            return false;
        }
        if let AgentConfigType::DayTime(val) = &values["02_start_time"] {
            time = ms_clear_offset(*val as u32, timezone);
        } else {
            error!("No start_time");
            return false;
        }
        if let AgentConfigType::TimeSlider(_l, _u, val, _s) = &values["03_slider"] {
            duration = *val;
        } else {
            error!("No slider");
            return false;
        }

        self.inner.set_config(time as u32, duration as u32);
        info!("Set config: {} {} {}", active, time, duration,);
        return true;
    }
}

impl TimeAgent {
    fn dispatch_task(&self) {
        let task_inner = self.inner.clone();
        self.inner.sender.send(AgentMessage::RepeatedTask(
            std::time::Duration::from_secs(1),
            Box::new(TickerFutBuilder {
                inner: task_inner.clone(),
            }),
        ));
    }
}

struct TickerFutBuilder {
    inner: Arc<TimeAgentInner>,
}

impl FutBuilder for TickerFutBuilder {
    fn build_future(
        &self,
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
    #[serde(skip, default = "ripe_core::sender_sentinel")]
    sender: AgentStreamSender,
    #[serde(skip, default)]
    last_state: RwLock<AgentState>,
    start_time_ms: AtomicU32,
    end_time_ms: AtomicU32,
}

impl Default for TimeAgentInner {
    fn default() -> Self {
        TimeAgentInner {
            sender: ripe_core::sender_sentinel(),
            last_state: RwLock::new(AgentState::Stopped(Utc::now())),
            start_time_ms: AtomicU32::new(0),
            end_time_ms: AtomicU32::new(0),
        }
    }
}

impl TimeAgentInner {
    fn tick(&self) {
        let curr_state = *self.last_state.read().unwrap();
        if let AgentState::Executing(until)
        | AgentState::Forced(until)
        | AgentState::Stopped(until) = curr_state
        {
            if until > Utc::now() {
                return; // still forced
            }
        }

        return if self.now_in_range() {
            self.set_state(AgentState::Executing(self.until()));
        } else {
            self.set_state(AgentState::Ready);
        };
    }

    fn force(&self, until: DateTime<Utc>) {
        let curr_state = *self.last_state.read().unwrap();

        if let AgentState::Executing(_) = curr_state {
            // no-op
        } else if let AgentState::Forced(_) | AgentState::Ready = curr_state {
            self.set_state(AgentState::Forced(until));
        } else if let AgentState::Stopped(_) = curr_state {
            if self.now_in_range() {
                self.set_state(AgentState::Executing(self.until()));
            } else {
                self.set_state(AgentState::Ready);
            }
        }
    }

    fn stop(&self, until: DateTime<Utc>) {
        let curr_state = *self.last_state.read().unwrap();

        if let AgentState::Executing(_) = curr_state {
            self.set_state(AgentState::Stopped(until));
        } else if let AgentState::Forced(_) = curr_state {
            if self.now_in_range() {
                self.set_state(AgentState::Executing(self.until()));
            } else {
                self.set_state(AgentState::Ready);
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
        if self.now_in_range() {
            self.set_state(AgentState::Executing(self.until()));
        } else {
            self.set_state(AgentState::Ready);
        }
    }

    fn set_state(&self, state: AgentState) {
        let curr_cmd = match state {
            AgentState::Executing(_) | AgentState::Forced(_) => CMD_ACTIVE,
            _ => CMD_INACTIVE,
        };

        self.sender.send(AgentMessage::Command(curr_cmd));
        *self.last_state.write().unwrap() = state;
    }

    fn cmd(&self) -> i32 {
        let guard = self.last_state.read().unwrap();
        match *guard {
            AgentState::Executing(_) | AgentState::Forced(_) => CMD_ACTIVE,
            _ => CMD_INACTIVE,
        }
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
