use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use chrono_tz::Tz;
use crossbeam::atomic::AtomicCell;
use ripe_core::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    pin::Pin,
    sync::{Arc, RwLock},
};
use tokio::sync::Notify;
use tokio::time::sleep;

use tracing::{debug, error, info, warn};

const NAME: &str = "ThresholdAgent";
const VERSION_CODE: u32 = 1;

export_plugin!(NAME, VERSION_CODE, build_agent);

/*
 * Implementation
 */

#[no_mangle]
extern "Rust" fn build_agent(
    config: Option<&str>,
    sender: AgentStreamSender,
) -> Box<dyn AgentTrait> {
    let mut agent: ThresholdAgent = ThresholdAgent::default();
    if let Some(config_json) = config {
        if let Ok(deserialized) = serde_json::from_str::<ThresholdAgent>(&config_json) {
            debug!("Restored {} from config", NAME);
            agent = deserialized;
        } else {
            warn!("{} coulnd't get restored from config {}", NAME, config_json);
        }
    };

    Box::new(ThresholdAgent {
        sender: Arc::new(sender),
        ..agent
    })
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ThresholdAgent {
    #[serde(skip, default = "ripe_core::sender_sentinel_arc")]
    sender: Arc<AgentStreamSender>,
    #[serde(skip)]
    task_cell: RwLock<ThresholdTask>,
    state: AgentState,
    min_threshold: u32,
    action_duration_ms: i64,
    action_cooldown_ms: i64,
    action_start: Option<DateTime<Utc>>,
    last_action: Option<DateTime<Utc>>,
}

impl PartialEq for ThresholdAgent {
    fn eq(&self, other: &Self) -> bool {
        self.state == other.state
            && self.min_threshold == other.min_threshold
            && self.action_duration_ms == other.action_duration_ms
            && self.action_cooldown_ms == other.action_cooldown_ms
            && self.action_start == other.action_start
            && self.last_action == other.last_action
    }
}

impl Default for ThresholdAgent {
    fn default() -> Self {
        ThresholdAgent {
            sender: ripe_core::sender_sentinel_arc(),
            task_cell: RwLock::default(),
            state: AgentState::Ready.into(),
            min_threshold: 20,
            action_duration_ms: 60 * 1000,
            action_cooldown_ms: 30 * 1000,
            action_start: None,
            last_action: None,
        }
    }
}

impl AgentTrait for ThresholdAgent {
    fn init(&mut self) {}

    fn handle_data(&mut self, data: &SensorDataMessage) {
        if data.moisture.is_none() {
            warn!("No moisture provided");
            return;
        }

        if let Ok(task) = self.task_cell.read() {
            if task.is_active() {
                info!("No action, we are already running");
                return;
            }
        }

        let watering_delta = Utc::now()
            - self
                .last_action
                .unwrap_or(DateTime::from_naive_utc_and_offset(NaiveDateTime::MIN, Utc));
        if watering_delta.num_milliseconds() < self.action_cooldown_ms {
            info!(
                "Still in cooldown for {} ms",
                self.action_cooldown_ms - watering_delta.num_milliseconds()
            );
            return;
        }

        let moisture = data.moisture.unwrap_or(std::f64::MAX) as u32;
        if moisture < self.min_threshold && self.action_duration_ms > 0 {
            info!("{} moisture below threshold", NAME);
            let until = Utc::now() + Duration::milliseconds(self.action_duration_ms);
            self.last_action = Some(until);
            if let Ok(mut task) = self.task_cell.write() {
                task.kickoff(false, until, self.sender.clone());
            }
        } else {
            info!(
                "{} moisture was fine {}% < {}%",
                NAME, moisture, self.min_threshold
            );
        }
    }

    fn handle_cmd(&mut self, payload: i64) {
        let is_enabled = payload.is_positive();

        if let Ok(mut task) = self.task_cell.write() {
            if is_enabled {
                if let Some(until) = AgentUIDecorator::transform_cmd_timepane(payload) {
                    info!("Threshold agent received run until {}", until);
                    self.last_action = Some(Utc::now());
                    task.kickoff(true, until, self.sender.clone());
                }
            } else {
                info!("Threshold agent received abort");
                task.abort();
            }
        }
    }

    fn render_ui(&self, _data: &SensorDataMessage, _timezone: Tz) -> AgentUI {
        let rendered: String;
        if let Some(last_action) = self.last_action {
            let delta: Duration = Utc::now() - last_action;
            if delta.num_hours() == 0 {
                rendered = format!("Letzte Aktion vor {} Minuten", delta.num_minutes());
            } else {
                rendered = format!(
                    "Letzte Aktion vor {}:{} Stunden",
                    delta.num_hours(),
                    (delta.num_minutes() - delta.num_hours() * 60)
                );
            }
        } else {
            rendered = "Noch keine Aktion".to_owned();
        }

        AgentUI {
            decorator: AgentUIDecorator::TimePane(30),
            rendered: rendered,
            state: self.state(),
        }
    }

    fn state(&self) -> AgentState {
        if let Ok(task) = self.task_cell.read() {
            return task.state();
        }
        AgentState::Error
    }

    fn cmd(&self) -> i32 {
        if let Ok(task) = self.task_cell.read() {
            if task.is_active() {
                return CMD_ACTIVE;
            }
        }
        CMD_INACTIVE
    }

    fn deserialize(&self) -> String {
        serde_json::to_string(self).unwrap()
    }

    fn config(&self, _timezone: Tz) -> HashMap<String, (String, AgentConfigType)> {
        let mut config = HashMap::new();
        config.insert(
            "01_active".to_owned(),
            ("Agent aktiviert".to_owned(), AgentConfigType::Switch(true)),
        );
        config.insert(
            "02_min_threshold".to_owned(),
            (
                "Schwellenwert in %".to_owned(),
                AgentConfigType::IntSlider(0, 100, self.min_threshold as i64),
            ),
        );
        config.insert(
            "03_action_duration_ms".to_owned(),
            (
                "Aktivierungsdauer in Minuten".to_owned(),
                AgentConfigType::TimeSlider(
                    0,
                    (DAY_MS / (24 * 6)) as i64,
                    self.action_duration_ms,
                    10 * 4,
                ),
            ),
        );
        config.insert(
            "04_action_cooldown_ms".to_owned(),
            (
                "Cooldown in Minuten".to_owned(),
                AgentConfigType::TimeSlider(0, (DAY_MS / 24) as i64, self.action_cooldown_ms, 60),
            ),
        );
        config
    }

    fn set_config(&mut self, values: &HashMap<String, AgentConfigType>, _timezone: Tz) -> bool {
        let min_threshold;
        let action_duration_ms;
        let action_cooldown_ms;
        if let AgentConfigType::Switch(_val) = &values["01_active"] {
            //
        } else {
            error!("No active");
            return false;
        }
        if let AgentConfigType::IntSlider(_l, _u, val) = &values["02_min_threshold"] {
            min_threshold = *val;
        } else {
            error!("No min_threshold");
            return false;
        }
        if let AgentConfigType::TimeSlider(_l, _u, val, _s) = &values["03_action_duration_ms"] {
            action_duration_ms = *val;
        } else {
            error!("No action_duration_ms");
            return false;
        }
        if let AgentConfigType::TimeSlider(_l, _u, val, _s) = &values["04_action_cooldown_ms"] {
            action_cooldown_ms = *val;
        } else {
            error!("No action_cooldown_ms");
            return false;
        }

        self.min_threshold = min_threshold as u32;
        self.action_duration_ms = action_duration_ms;
        self.action_cooldown_ms = action_cooldown_ms;
        true
    }
}

#[derive(Debug)]
struct ThresholdTask {
    task_config: Arc<ThresholdTaskInner>,
}

#[derive(Debug)]
struct ThresholdTaskBuilder {
    task_config: Arc<ThresholdTaskInner>,
}

#[derive(Debug)]
struct ThresholdTaskInner {
    state: AtomicCell<AgentState>,
    sender: Arc<AgentStreamSender>,
    until: AtomicCell<DateTime<Utc>>,
    notify: Notify,
    is_forced: bool,
}

impl Default for ThresholdTask {
    fn default() -> Self {
        ThresholdTask {
            task_config: Arc::new(ThresholdTaskInner {
                state: AtomicCell::new(AgentState::Ready),
                until: AtomicCell::new(Utc::now()),
                sender: Arc::new(ripe_core::sender_sentinel()),
                notify: Notify::default(),
                is_forced: false,
            }),
        }
    }
}

impl ThresholdTask {
    pub fn kickoff(
        &mut self,
        is_forced: bool,
        until: DateTime<Utc>,
        sender: Arc<AgentStreamSender>,
    ) {
        if self.is_active() {
            let running_state = if is_forced {
                AgentState::Forced(until)
            } else {
                AgentState::Executing(until)
            };
            self.task_config.until.store(until);
            self.task_config.state.store(running_state);
            return;
        }

        // send new task
        self.task_config = Arc::new(ThresholdTaskInner {
            state: AtomicCell::new(AgentState::Ready),
            until: AtomicCell::new(until),
            notify: Notify::default(),
            sender,
            is_forced,
        });

        self.task_config
            .sender
            .send(AgentMessage::OneshotTask(Box::new(ThresholdTaskBuilder {
                task_config: self.task_config.clone(),
            })));
    }

    pub fn abort(&mut self) {
        self.task_config
            .sender
            .send(AgentMessage::Command(CMD_INACTIVE));
        self.task_config.notify.notify_one();
    }

    fn state(&self) -> AgentState {
        self.task_config.state.load()
    }

    fn is_active(&self) -> bool {
        if let AgentState::Executing(_) | AgentState::Forced(_) = self.task_config.state.load() {
            true
        } else {
            false
        }
    }
}

impl FutBuilder for ThresholdTaskBuilder {
    fn build_future(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = bool> + Send + Sync + 'static>> {
        let config = self.task_config.clone();
        Box::pin(async move {
            let running_state = if config.is_forced {
                AgentState::Forced(config.until.load())
            } else {
                AgentState::Executing(config.until.load())
            };

            config.state.store(running_state);
            config.sender.send(AgentMessage::Command(CMD_ACTIVE));

            info!("{} started Task until {}", NAME, config.until.load());

            let start = Utc::now();
            let delta = config.until.load() - start;
            tokio::select!(
                _ = config.notify.notified() => {
                    info!("{} received abort signal", NAME);
                },
                _ = sleep(std::time::Duration::from_millis(delta.num_milliseconds() as u64)) => {}
            );

            config.state.store(AgentState::Ready);
            config.sender.send(AgentMessage::Command(CMD_INACTIVE));
            info!(
                "{} ended Task after {} secs",
                NAME,
                (Utc::now() - start).num_seconds()
            );

            true
        })
    }
}
