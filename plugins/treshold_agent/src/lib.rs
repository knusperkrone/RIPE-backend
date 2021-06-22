#[macro_use]
extern crate slog;

use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use chrono_tz::Tz;
use crossbeam::atomic::AtomicCell;
use ripe_core::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, RwLock,
    },
};
use tokio::sync::mpsc::Sender;

const NAME: &str = "ThresholdAgent";
const VERSION_CODE: u32 = 1;

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
    let mut agent: ThresholdAgent = ThresholdAgent::default();
    if let Some(config_json) = config {
        if let Ok(deserialized) = serde_json::from_str::<ThresholdAgent>(&config_json) {
            debug!(logger, "Restored {} from config", NAME);
            agent = deserialized;
        } else {
            warn!(
                logger,
                "{} coulnd't get restored from config {}", NAME, config_json
            );
        }
    } else {
        debug!(logger, "Created new {}", NAME);
    }

    agent.logger = logger;
    agent.sender = sender;
    Box::new(agent)
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ThresholdAgent {
    #[serde(skip, default = "ripe_core::logger_sentinel")]
    logger: slog::Logger,
    #[serde(skip, default = "ripe_core::sender_sentinel")]
    sender: Sender<AgentMessage>,
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
            logger: ripe_core::logger_sentinel(),
            sender: ripe_core::sender_sentinel(),
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
    fn handle_data(&mut self, data: &SensorDataMessage) {
        if let Ok(task) = self.task_cell.read() {
            if task.is_active() {
                debug!(self.logger, "{} no action, as already running", NAME);
                return;
            }
        }

        let watering_delta = Utc::now()
            - self
                .last_action
                .unwrap_or(DateTime::from_utc(NaiveDateTime::from_timestamp(0, 0), Utc));
        if watering_delta.num_milliseconds() < self.action_cooldown_ms {
            debug!(self.logger, "{} still in cooldown", NAME);
            return;
        }

        if data.moisture.is_none() {
            debug!(self.logger, "{} no moisture provided", NAME);
            return;
        }

        let moisture = data.moisture.unwrap_or(std::f64::MAX) as u32;
        if moisture < self.min_threshold {
            debug!(self.logger, "{} moisture below threshold", NAME);
            let until = Utc::now() + Duration::milliseconds(self.action_duration_ms);
            self.last_action = Some(until);
            if let Ok(mut task) = self.task_cell.write() {
                task.kickoff(false, until, self.logger.clone(), self.sender.clone());
            }
        } else {
            debug!(self.logger, "{} moisture was fine {}%", NAME, moisture);
        }
    }

    fn handle_cmd(&mut self, payload: i64) {
        let is_enabled = payload.is_positive();

        if let Ok(mut task) = self.task_cell.write() {
            if is_enabled {
                if let Some(until) = AgentUIDecorator::transform_cmd_timepane(payload) {
                    info!(self.logger, "Threshold agent received run until {}", until);
                    self.last_action = Some(Utc::now());
                    task.kickoff(true, until, self.logger.clone(), self.sender.clone());
                }
            } else {
                debug!(self.logger, "Threshold agent received abort");
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
                return 1;
            }
        }
        0
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
            error!(self.logger, "No active");
            return false;
        }
        if let AgentConfigType::IntSlider(_l, _u, val) = &values["02_min_threshold"] {
            min_threshold = *val;
        } else {
            error!(self.logger, "No min_threshold");
            return false;
        }
        if let AgentConfigType::TimeSlider(_l, _u, val, _s) = &values["03_action_duration_ms"] {
            action_duration_ms = *val;
        } else {
            error!(self.logger, "No action_duration_ms");
            return false;
        }
        if let AgentConfigType::TimeSlider(_l, _u, val, _s) = &values["04_action_cooldown_ms"] {
            error!(self.logger, "No action_cooldown_ms");
            action_cooldown_ms = *val;
        } else { 
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
    aborted: Arc<AtomicBool>,
    state: AtomicCell<AgentState>,
    until: AtomicCell<DateTime<Utc>>,
    logger: slog::Logger,
    sender: Sender<AgentMessage>,
    is_forced: bool,
}

impl Default for ThresholdTask {
    fn default() -> Self {
        ThresholdTask {
            task_config: Arc::new(ThresholdTaskInner {
                aborted: Arc::new(AtomicBool::from(false)),
                state: AtomicCell::new(AgentState::Ready),
                until: AtomicCell::new(Utc::now()),
                logger: ripe_core::logger_sentinel(),
                sender: ripe_core::sender_sentinel(),
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
        logger: slog::Logger,
        sender: Sender<AgentMessage>,
    ) {
        if self.is_active() {
            // racecondtion, between busy while loop and this set
            // 500ms window should be enough, as thread will sleep.
            let race_window = self.task_config.until.load() - Utc::now();
            if race_window.num_milliseconds() > 500 {
                // internal update and broadcast new value
                let running_state = if is_forced {
                    AgentState::Forced(until)
                } else {
                    AgentState::Executing(until)
                };
                self.task_config.until.store(until);
                self.task_config.state.store(running_state);
                ripe_core::send_payload(&logger, &sender, AgentMessage::Command(CMD_ACTIVE));
                return;
            }
        }

        // send new task
        self.task_config = Arc::new(ThresholdTaskInner {
            aborted: Arc::new(AtomicBool::new(false)),
            state: AtomicCell::new(AgentState::Ready),
            until: AtomicCell::new(until),
            logger,
            sender,
            is_forced,
        });

        send_payload(
            &self.task_config.logger,
            &self.task_config.sender,
            AgentMessage::OneshotTask(Box::new(ThresholdTaskBuilder {
                task_config: self.task_config.clone(),
            })),
        )
    }

    pub fn abort(&mut self) {
        self.task_config.aborted.store(true, Ordering::Relaxed);
    }

    fn state(&self) -> AgentState {
        self.task_config.state.load()
    }

    fn is_active(&self) -> bool {
        return if let AgentState::Executing(_) | AgentState::Forced(_) =
            self.task_config.state.load()
        {
            true
        } else {
            false
        };
    }
}

impl FutBuilder for ThresholdTaskBuilder {
    fn build_future(
        &self,
        runtime: tokio::runtime::Handle,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + Sync + 'static>> {
        let config = self.task_config.clone();
        Box::pin(async move {
            let start = Utc::now();

            let running_state = if config.is_forced {
                AgentState::Forced(config.until.load())
            } else {
                AgentState::Executing(config.until.load())
            };
            config.state.store(running_state);

            ripe_core::send_payload(
                &config.logger,
                &config.sender,
                AgentMessage::Command(CMD_ACTIVE),
            );
            debug!(
                config.logger,
                "{} started Task until {}",
                NAME,
                config.until.load()
            );

            while Utc::now() < config.until.load() && !config.aborted.load(Ordering::Relaxed) {
                ripe_core::sleep(&runtime, std::time::Duration::from_secs(1)).await;
            }
            // Race condition
            config.state.store(AgentState::Ready);
            ripe_core::send_payload(
                &config.logger,
                &config.sender,
                AgentMessage::Command(CMD_INACTIVE),
            );

            let delta_secs = (Utc::now() - start).num_seconds();
            if config.aborted.load(Ordering::Relaxed) {
                debug!(
                    config.logger,
                    "{} aborted Task after {} secs", NAME, delta_secs
                );
            } else {
                debug!(
                    config.logger,
                    "{} ended Task after {} secs", NAME, delta_secs
                );
            }
            true
        })
    }
}
