#[macro_use]
extern crate slog;

use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use iftem_core::*;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::sync::mpsc::Sender;

const NAME: &str = "ThresholdAgent";
const VERSION_CODE: u32 = 1;

export_plugin!(NAME, VERSION_CODE, build_agent);

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
    task_cell: RefCell<ThresholdTask>,
    state: AgentState,
    min_threshold: f64,
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
            task_cell: RefCell::default(),
            state: AgentState::Active.into(),
            min_threshold: 20.0,
            action_duration_sec: 60,
            action_cooldown_sec: 30,
            action_start: None,
            last_action: None,
        }
    }
}

impl ThresholdAgent {
    fn start_task(&self, force: bool, until: DateTime<Utc>) {
        let mut action_task =
            ThresholdTask::kickoff(force, until, self.logger.clone(), self.sender.clone());
        action_task.run();

        self.task_cell.replace(action_task);
    }
}

impl AgentTrait for ThresholdAgent {
    fn do_action(&mut self, data: &SensorDataMessage) {
        if let Ok(task) = self.task_cell.try_borrow() {
            if task.is_active() {
                info!(self.logger, "{} Already active action", NAME);
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

        // TODO: Configure getter
        if data.moisture.is_none() {
            warn!(self.logger, "{} no moisture provided", NAME);
            return;
        }

        let moisture = data.moisture.unwrap_or(std::f64::MAX);
        if moisture < self.min_threshold {
            info!(self.logger, "{} moisture below threshold", NAME);
            let until = Utc::now() + Duration::seconds(self.action_duration_sec);
            self.last_action = Some(until);
            self.start_task(false, until);
        } else {
            info!(self.logger, "{} moisture was fine {}%", NAME, moisture);
        }
    }

    fn do_force(&mut self, active: bool, until: DateTime<Utc>) {
        let mut task = self.task_cell.borrow_mut();
        if task.is_active() && !active {
            task.abort();
        } else if !task.is_active() && active {
            self.start_task(true, until);
        }
    }

    fn state(&self) -> &AgentState {
        &self.state
    }

    fn cmd(&self) -> i32 {
        if let Ok(task) = self.task_cell.try_borrow() {
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

    fn render_ui(&self, _data: &SensorDataMessage) -> AgentUI {
        let rendered: String;
        if let Some(last_action) = self.last_action {
            let delta: Duration = Utc::now() - last_action;
            if delta.num_hours() != 0 {
                rendered = format!("Letzte Wässerung vor {} Minuten.", delta.num_minutes());
            } else {
                rendered = format!(
                    "Letzte Wässerung vor {}:{} Stunden.",
                    delta.num_hours(),
                    delta.num_hours()
                );
            }
        } else {
            rendered = "Noch nicht gewässert.".to_owned();
        }

        AgentUI {
            decorator: AgentUIDecorator::TimedButton(30),
            rendered: rendered,
            state: self.state,
        }
    }
}

#[derive(Debug)]
struct ThresholdTaskConfig {
    active: Arc<AtomicBool>,
    aborted: Arc<AtomicBool>,
    forced: bool,
    until: DateTime<Utc>,
    logger: slog::Logger,
    sender: Sender<AgentMessage>,
}

#[derive(Debug)]
struct ThresholdTask {
    task_config: Arc<ThresholdTaskConfig>,
}

impl Default for ThresholdTask {
    fn default() -> Self {
        ThresholdTask {
            task_config: Arc::new(ThresholdTaskConfig {
                active: Arc::new(AtomicBool::from(false)),
                aborted: Arc::new(AtomicBool::from(false)),
                forced: false,
                until: Utc::now(),
                logger: iftem_core::logger_sentinel(),
                sender: iftem_core::sender_sentinel(),
            }),
        }
    }
}

impl ThresholdTask {
    pub fn kickoff(
        forced: bool,
        until: DateTime<Utc>,
        logger: slog::Logger,
        sender: Sender<AgentMessage>,
    ) -> Self {
        ThresholdTask {
            task_config: Arc::new(ThresholdTaskConfig {
                active: Arc::new(AtomicBool::new(true)),
                aborted: Arc::new(AtomicBool::new(false)),
                forced: forced,
                logger: logger,
                sender: sender,
                until: until,
            }),
        }
    }

    pub fn abort(&mut self) {
        self.task_config.aborted.store(true, Ordering::Relaxed);
    }

    fn run(&mut self) {
        let config = self.task_config.clone();

        send_payload(
            &self.task_config.logger,
            &self.task_config.sender,
            AgentMessage::Task(Box::pin(async move {
                let start = Utc::now();

                let mut state = AgentState::Active;
                if config.forced {
                    state = AgentState::Forced(config.until.clone());
                }
                iftem_core::send_payload(
                    &config.logger,
                    &config.sender,
                    AgentMessage::State(state),
                );
                iftem_core::send_payload(&config.logger, &config.sender, AgentMessage::Command(1));
                info!(
                    config.logger,
                    "{} started Task until {}", NAME, config.until
                );

                while Utc::now() < config.until && !config.aborted.load(Ordering::Relaxed) {
                    iftem_core::task_sleep(0).await;
                }

                config.active.store(false, Ordering::Relaxed);
                state = AgentState::Default;
                iftem_core::send_payload(
                    &config.logger,
                    &config.sender,
                    AgentMessage::State(state),
                );
                iftem_core::send_payload(&config.logger, &config.sender, AgentMessage::Command(0));

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
            })),
        );
    }

    fn is_active(&self) -> bool {
        self.task_config.active.load(Ordering::Relaxed)
    }
}
