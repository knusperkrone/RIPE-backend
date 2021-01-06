#[macro_use]
extern crate slog;

use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use crossbeam::atomic::AtomicCell;
use iftem_core::*;
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

#[allow(improper_ctypes_definitions)]
extern "C" fn build_agent(
    config: Option<&std::string::String>,
    logger: slog::Logger,
    sender: Sender<AgentMessage>,
) -> Box<dyn AgentTrait> {
    let mut agent: ThresholdAgent = ThresholdAgent::default();
    if let Some(config_json) = config {
        if let Ok(deserialized) = serde_json::from_str::<ThresholdAgent>(&config_json) {
            info!(logger, "Restored {} from config", NAME);
            agent = deserialized;
        } else {
            warn!(
                logger,
                "{} coulnd't get restored from config {}", NAME, config_json
            );
        }
    } else {
        info!(logger, "Created new {}", NAME);
    }

    agent.logger = logger;
    agent.sender = sender;
    Box::new(agent)
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ThresholdAgent {
    #[serde(skip, default = "iftem_core::logger_sentinel")]
    logger: slog::Logger,
    #[serde(skip, default = "iftem_core::sender_sentinel")]
    sender: Sender<AgentMessage>,
    #[serde(skip)]
    task_cell: RwLock<ThresholdTask>,
    state: AgentState,
    min_threshold: u32,
    action_duration_sec: i64,
    action_cooldown_sec: i64,
    action_start: Option<DateTime<Utc>>,
    last_action: Option<DateTime<Utc>>,
}

impl PartialEq for ThresholdAgent {
    fn eq(&self, other: &Self) -> bool {
        self.state == other.state
            && self.min_threshold == other.min_threshold
            && self.action_duration_sec == other.action_duration_sec
            && self.action_cooldown_sec == other.action_cooldown_sec
            && self.action_start == other.action_start
            && self.last_action == other.last_action
    }
}

impl Default for ThresholdAgent {
    fn default() -> Self {
        ThresholdAgent {
            logger: iftem_core::logger_sentinel(),
            sender: iftem_core::sender_sentinel(),
            task_cell: RwLock::default(),
            state: AgentState::Active.into(),
            min_threshold: 20,
            action_duration_sec: 60,
            action_cooldown_sec: 30,
            action_start: None,
            last_action: None,
        }
    }
}

impl AgentTrait for ThresholdAgent {
    fn on_data(&mut self, data: &SensorDataMessage) {
        if let Ok(task) = self.task_cell.read() {
            if task.is_active() {
                info!(self.logger, "{} no action, as already running", NAME);
                return;
            }
        }

        let watering_delta = Utc::now()
            - self
                .last_action
                .unwrap_or(DateTime::from_utc(NaiveDateTime::from_timestamp(0, 0), Utc));
        if watering_delta.num_seconds() < self.action_cooldown_sec {
            info!(self.logger, "{} still in cooldown", NAME);
            return;
        }

        if data.moisture.is_none() {
            warn!(self.logger, "{} no moisture provided", NAME);
            return;
        }

        let moisture = data.moisture.unwrap_or(std::f64::MAX) as u32;
        if moisture < self.min_threshold {
            info!(self.logger, "{} moisture below threshold", NAME);
            let until = Utc::now() + Duration::seconds(self.action_duration_sec);
            self.last_action = Some(until);
            if let Ok(mut task) = self.task_cell.write() {
                task.kickoff(until, self.logger.clone(), self.sender.clone());
            }
        } else {
            info!(self.logger, "{} moisture was fine {}%", NAME, moisture);
        }
    }

    fn on_cmd(&mut self, payload: i64) {
        if let Ok(mut task) = self.task_cell.write() {
            if task.is_active() {
                task.abort();
            } else if !task.is_active() {
                let until_opt = AgentUIDecorator::transform_cmd_timepane(payload);
                if let Some(until) = until_opt {
                    self.last_action = Some(Utc::now());
                    task.kickoff(until, self.logger.clone(), self.sender.clone());
                }
            }
        }
    }

    fn render_ui(&self, _data: &SensorDataMessage) -> AgentUI {
        let rendered: String;
        if let Some(last_action) = self.last_action {
            let delta: Duration = Utc::now() - last_action;
            if delta.num_hours() == 0 {
                rendered = format!("Letzte Aktion vor {} Minuten", delta.num_minutes());
            } else {
                rendered = format!(
                    "Letzte Aktion vor {}.{} Stunden",
                    delta.num_hours(),
                    (delta.num_minutes() - delta.num_hours() * 60)
                );
            }
        } else {
            rendered = "Noch keine Aktion.".to_owned();
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

    fn deserialize(&self) -> AgentConfig {
        AgentConfig {
            name: NAME.to_owned(),
            state_json: serde_json::to_string(self).unwrap(),
        }
    }

    fn config(&self) -> HashMap<&str, (&str, AgentConfigType)> {
        let mut config = HashMap::new();
        config.insert("active", ("Agent aktiv", AgentConfigType::Switch(true)));
        config.insert(
            "min_threshold",
            (
                "Schwellenwert",
                AgentConfigType::IntRange(0, 100, self.min_threshold as i64),
            ),
        );
        config.insert(
            "action_duration_sec",
            (
                "Bewässerungsdauer in Minuten",
                AgentConfigType::IntRange(0, i64::MAX, self.action_duration_sec),
            ),
        );
        config.insert(
            "action_cooldown_sec",
            (
                "Pause in Minuten",
                AgentConfigType::IntRange(0, i64::MAX, self.action_cooldown_sec),
            ),
        );
        config
    }

    fn on_config(&mut self, values: &HashMap<String, AgentConfigType>) -> bool {
        let min_threshold;
        let action_duration_sec;
        let action_cooldown_sec;
        if let AgentConfigType::IntRange(_, __, val) = &values["min_threshold"] {
            min_threshold = *val;
        } else {
            return false;
        }
        if let AgentConfigType::IntRange(_, __, val) = &values["action_duration_sec"] {
            action_duration_sec = *val;
        } else {
            return false;
        }
        if let AgentConfigType::IntRange(_, __, val) = &values["action_cooldown_sec"] {
            action_cooldown_sec = *val;
        } else {
            return false;
        }

        self.min_threshold = min_threshold as u32;
        self.action_duration_sec = action_duration_sec;
        self.action_cooldown_sec = action_cooldown_sec;
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
    until: DateTime<Utc>,
    logger: slog::Logger,
    sender: Sender<AgentMessage>,
}

impl Default for ThresholdTask {
    fn default() -> Self {
        ThresholdTask {
            task_config: Arc::new(ThresholdTaskInner {
                aborted: Arc::new(AtomicBool::from(false)),
                state: AtomicCell::new(AgentState::Active),
                until: Utc::now(),
                logger: iftem_core::logger_sentinel(),
                sender: iftem_core::sender_sentinel(),
            }),
        }
    }
}

impl ThresholdTask {
    pub fn kickoff(
        &mut self,
        until: DateTime<Utc>,
        logger: slog::Logger,
        sender: Sender<AgentMessage>,
    ) {
        if self.is_active() {
            warn!(
                logger,
                "Cannot kickoff task, as there is one already running!"
            );
            return;
        }

        self.task_config = Arc::new(ThresholdTaskInner {
            aborted: Arc::new(AtomicBool::new(false)),
            state: AtomicCell::new(AgentState::Active),
            until: until,
            logger: logger,
            sender: sender,
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
        return if let AgentState::Executing(_) = self.task_config.state.load() {
            true
        } else {
            false
        };
    }
}

impl FutBuilder for ThresholdTaskBuilder {
    fn build_future(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + Sync + 'static>> {
        let config = self.task_config.clone();
        Box::pin(async move {
            let start = Utc::now();

            config
                .state
                .store(AgentState::Executing(config.until.clone()));
            iftem_core::send_payload(
                &config.logger,
                &config.sender,
                AgentMessage::Command(CMD_ACTIVE),
            );
            info!(
                config.logger,
                "{} started Task until {}", NAME, config.until
            );

            while Utc::now() < config.until && !config.aborted.load(Ordering::Relaxed) {
                iftem_core::task_sleep(0).await;
            }

            config.state.store(AgentState::Ready);
            iftem_core::send_payload(
                &config.logger,
                &config.sender,
                AgentMessage::Command(CMD_INACTIVE),
            );

            let delta_secs = (Utc::now() - start).num_seconds();
            if config.aborted.load(Ordering::Relaxed) {
                info!(
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
