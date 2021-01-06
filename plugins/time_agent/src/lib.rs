#[macro_use]
extern crate slog;

use chrono::{Timelike, Utc};
use iftem_core::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    pin::Pin,
    sync::{
        atomic::{AtomicI32, Ordering},
        Arc, RwLock,
    },
};
use tokio::sync::mpsc::Sender;

const NAME: &str = "TimeAgent";
const VERSION_CODE: u32 = 1;

export_plugin!(NAME, VERSION_CODE, build_agent);

const DAY_MS: i32 = 86_400_000;

#[allow(improper_ctypes_definitions)]
extern "C" fn build_agent(
    config: Option<&std::string::String>,
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
                "{} coulnd't get restored from config {}", NAME, config_json
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

#[derive(Debug, Deserialize, Serialize)]
pub struct TimeAgentInner {
    #[serde(skip, default = "iftem_core::logger_sentinel")]
    logger: slog::Logger,
    #[serde(skip, default = "iftem_core::sender_sentinel")]
    sender: Sender<AgentMessage>,
    #[serde(skip, default = "default_force_state")]
    force_state: RwLock<AgentState>,
    #[serde(skip, default = "default_shadow_cmd")]
    shadow_command: AtomicI32,
    start_time_ms: i32,
    end_time_ms: i32,
}

fn default_force_state() -> RwLock<AgentState> {
    RwLock::new(AgentState::Ready)
}

fn default_shadow_cmd() -> AtomicI32 {
    AtomicI32::new(-1)
}

impl Default for TimeAgentInner {
    fn default() -> Self {
        TimeAgentInner {
            logger: iftem_core::logger_sentinel(),
            sender: iftem_core::sender_sentinel(),
            force_state: default_force_state(),
            shadow_command: default_shadow_cmd(),
            start_time_ms: 0,
            end_time_ms: 0,
        }
    }
}

impl AgentTrait for TimeAgent {
    fn on_data(&mut self, _data: &SensorDataMessage) {
        // no-op
    }

    fn on_cmd(&mut self, payload: i64) {
        let until_opt = AgentUIDecorator::transform_cmd_timepane(payload);
        if let Some(until) = until_opt {
            if self.inner.cmd() == CMD_ACTIVE {
                *self.inner.force_state.write().unwrap() = AgentState::Stopped(until);
            } else {
                *self.inner.force_state.write().unwrap() = AgentState::Forced(until);
            }

            iftem_core::send_payload(
                &self.inner.logger,
                &self.inner.sender,
                AgentMessage::Command(self.cmd()),
            );
        }
    }

    fn render_ui(&self, _: &SensorDataMessage) -> AgentUI {
        AgentUI {
            decorator: AgentUIDecorator::TimePane(60),
            state: self.state(),
            rendered: format!(
                "Von {} - {}",
                secs_to_hr(self.inner.start_time_ms),
                secs_to_hr(self.inner.end_time_ms)
            ),
        }
    }

    fn cmd(&self) -> i32 {
        return self.inner.cmd();
    }

    fn deserialize(&self) -> AgentConfig {
        AgentConfig {
            name: NAME.to_owned(),
            state_json: serde_json::to_string(&(*self.inner)).unwrap(),
        }
    }

    fn state(&self) -> AgentState {
        if self.inner.cmd() == CMD_ACTIVE {
            let state_guard = self.inner.force_state.read().unwrap().to_owned();
            let until_opt = match state_guard {
                AgentState::Executing(until) => (true, Some(until)),
                AgentState::Stopped(until) => (false, Some(until)),
                _ => (false, None),
            };

            if let (active, Some(until)) = until_opt {
                if until < Utc::now() {
                    return if active {
                        AgentState::Executing(until)
                    } else {
                        AgentState::Stopped(until)
                    };
                };
            }
            return AgentState::Active;
        }
        AgentState::Ready
    }

    fn config(&self) -> HashMap<&str, (&str, AgentConfigType)> {
        let mut config = HashMap::new();
        config.insert("active", ("Agent aktiv", AgentConfigType::Switch(true)));
        config.insert(
            "time",
            (
                "Startzeit",
                AgentConfigType::DateTime(self.inner.start_time_ms as u64),
            ),
        );
        config.insert(
            "slider",
            (
                "Bewässerungsdauer in Stunden",
                AgentConfigType::IntSliderRange(0, 24, self.duration() as i64),
            ),
        );
        config
    }

    fn on_config(&mut self, values: &HashMap<String, AgentConfigType>) -> bool {
        let active;
        let time;
        let duration;
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
        if let AgentConfigType::IntSliderRange(_, __, val) = &values["slider"] {
            duration = *val;
        } else {
            return false;
        }

        info!(
            self.inner.logger,
            "Set config: {}, {}, {}", active, time, duration
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

    fn duration(&self) -> i32 {
        let is_next_day = self.inner.end_time_ms <= self.inner.start_time_ms;
        if is_next_day {
            DAY_MS - self.inner.start_time_ms + self.inner.start_time_ms
        } else {
            self.inner.end_time_ms - self.inner.start_time_ms
        }
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

impl TimeAgentInner {
    fn tick(&self) {
        let curr_cmd = self.cmd();
        if curr_cmd != self.shadow_command.load(Ordering::Relaxed) {
            info!(self.logger, "Changed cmd to: {}", curr_cmd);
            self.shadow_command.store(curr_cmd, Ordering::Relaxed);
            send_payload(&self.logger, &self.sender, AgentMessage::Command(curr_cmd));
        }
    }

    fn cmd(&self) -> i32 {
        let state_guard = self.force_state.read().unwrap().to_owned();
        if let AgentState::Executing(until) = state_guard {
            if until < Utc::now() {
                return CMD_ACTIVE;
            }
        } else if let AgentState::Stopped(until) = state_guard {
            if until < Utc::now() {
                return CMD_INACTIVE;
            }
        }

        return if self.now_in_range() {
            CMD_ACTIVE
        } else {
            CMD_INACTIVE
        };
    }

    fn now_in_range(&self) -> bool {
        let now = Utc::now();

        let mut daytime_ms = now.second() as i32 * 1000; // Secs
        daytime_ms += now.minute() as i32 * 60 * 1000; // Mins
        daytime_ms += now.hour() as i32 * 60 * (60 * 1000); // Hours

        let is_next_day = self.end_time_ms <= self.start_time_ms;
        if is_next_day {
            return daytime_ms > self.start_time_ms || daytime_ms < self.end_time_ms;
        } else {
            return daytime_ms > self.start_time_ms && daytime_ms < self.start_time_ms;
        }
    }
}
