#[macro_use]
extern crate slog;

use chrono::{DateTime, Timelike, Utc};
use iftem_core::*;
use serde::{Deserialize, Serialize};
use std::{
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

const CMD_ACTIVE: i32 = 1;
const CMD_INACTIVE: i32 = 1;

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
    force_state: RwLock<Option<(bool, DateTime<Utc>)>>,
    #[serde(skip, default = "default_shadow_cmd")]
    shadow_command: AtomicI32,
    start_time_ms: u32,
    end_time_ms: u32,
}

fn default_force_state() -> RwLock<Option<(bool, DateTime<Utc>)>> {
    RwLock::new(None)
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
    fn do_action(&mut self, _: &SensorDataMessage) {
        // NO-OP
    }

    fn do_force(&mut self, active: bool, until: DateTime<Utc>) {
        *self.inner.force_state.write().unwrap() = Some((active, until));
        iftem_core::send_payload(
            &self.inner.logger,
            &self.inner.sender,
            AgentMessage::Command(self.cmd()),
        );
    }

    fn cmd(&self) -> i32 {
        return self.inner.cmd();
    }

    fn render_ui(&self, _: &SensorDataMessage) -> AgentUI {
        AgentUI {
            decorator: AgentUIDecorator::TimePane(60),
            state: self.state(),
            rendered: format!(
                "Von {} - {}",
                self.to_hr(self.inner.start_time_ms),
                self.to_hr(self.inner.end_time_ms)
            ),
        }
    }

    fn deserialize(&self) -> AgentConfig {
        AgentConfig {
            name: NAME.to_owned(),
            state_json: serde_json::to_string(&(*self.inner)).unwrap(),
        }
    }

    fn state(&self) -> AgentState {
        if self.inner.cmd() == CMD_ACTIVE {
            if let Some((active, until)) = self.inner.force_state.read().unwrap().as_ref() {
                if until < &Utc::now() {
                    return AgentState::Forced(*active, *until);
                } else {
                    *self.inner.force_state.write().unwrap() = None
                }
            }
            return AgentState::Active;
        }
        AgentState::Default
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

    fn to_hr(&self, time_ms: u32) -> String {
        let seconds = time_ms / 1000;
        let minutes = seconds / 60;
        let hours = minutes / 60;

        let pad = |x| {
            return if x < 10 {
                format!("0{}", x)
            } else {
                format!("{}", x)
            };
        };
        return format!("{}:{}", pad(hours), pad(minutes % 60));
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
        if let Some((active, until)) = self.force_state.read().unwrap().as_ref() {
            let now = Utc::now();
            if until < &now {
                return if *active { CMD_ACTIVE } else { CMD_INACTIVE };
            } else {
                *self.force_state.write().unwrap() = None
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

        let mut daytime_ms = now.second() * 1000; // Secs
        daytime_ms += now.minute() * 60 * 1000; // Mins
        daytime_ms += now.hour() * 60 * (60 * 1000); // Hours

        let is_next_day = self.end_time_ms <= self.start_time_ms;
        if is_next_day {
            return daytime_ms > self.start_time_ms || daytime_ms < self.end_time_ms;
        } else {
            return daytime_ms > self.start_time_ms && daytime_ms < self.start_time_ms;
        }
    }
}
