#[macro_use]
extern crate slog;

use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use plugins_core::*;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::sync::mpsc::Sender;
use tokio::task::JoinHandle;

const NAME: &str = "ThresholdAgent";

export_plugin!(NAME, build_agent);

extern "C" fn build_agent(
    config: Option<&std::string::String>,
    logger: slog::Logger,
    sender: Sender<AgentPayload>,
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
    #[serde(skip, default = "plugins_core::logger_sentinel")]
    logger: slog::Logger,
    #[serde(skip, default = "plugins_core::sender_sentinel")]
    sender: Sender<AgentPayload>,
    #[serde(skip)]
    task_cell: RefCell<ThresholdTask>,
    state: AgentState,
    min_threshold: u32,
    action_duration_sec: i64,
    action_start: Option<DateTime<Utc>>,
    last_action: Option<DateTime<Utc>>,
}

impl PartialEq for ThresholdAgent {
    fn eq(&self, other: &Self) -> bool {
        self.state == other.state
            && self.min_threshold == other.min_threshold
            && self.action_start == other.action_start
            && self.last_action == other.last_action
    }
}

impl Default for ThresholdAgent {
    fn default() -> Self {
        ThresholdAgent {
            logger: plugins_core::logger_sentinel(),
            sender: plugins_core::sender_sentinel(),
            task_cell: RefCell::default(),
            state: AgentState::Active.into(),
            min_threshold: 20,
            action_duration_sec: 60,
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
    fn do_action(&mut self, data: &SensorDataDto) {
        let _now = Utc::now();
        let _watering_diff = self
            .last_action
            .unwrap_or(DateTime::from_utc(NaiveDateTime::from_timestamp(0, 0), Utc))
            - data.timestamp;

        // TODO: Configure getter
        if data.moisture.is_none() {
            warn!(self.logger, "{} no moisture provided", NAME);
            return;
        }

        let moisture = data.moisture.unwrap_or(std::u32::MAX);
        if moisture < self.min_threshold {
            info!(self.logger, "{} moisture below threshold", NAME);
            let until = Utc::now() + Duration::seconds(self.action_duration_sec);
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

    fn deserialize(&self) -> AgentConfig {
        AgentConfig {
            name: NAME.to_owned(),
            state_json: serde_json::to_string(self).unwrap(),
        }
    }
}

#[derive(Debug)]
struct ThresholdTaskConfig {
    forced: bool,
    until: DateTime<Utc>,
    logger: slog::Logger,
    sender: Sender<AgentPayload>,
}

#[derive(Debug)]
struct ThresholdTask {
    active: Arc<AtomicBool>,
    aborted: Arc<AtomicBool>,
    task_handle: Option<JoinHandle<()>>,
    task_config: Arc<ThresholdTaskConfig>,
}

impl Default for ThresholdTask {
    fn default() -> Self {
        ThresholdTask {
            active: Arc::new(AtomicBool::from(false)),
            aborted: Arc::new(AtomicBool::from(false)),
            task_handle: None,
            task_config: Arc::new(ThresholdTaskConfig {
                forced: false,
                until: Utc::now(),
                logger: plugins_core::logger_sentinel(),
                sender: plugins_core::sender_sentinel(),
            }),
        }
    }
}

unsafe impl Send for ThresholdTask {}

impl ThresholdTask {
    pub fn kickoff(
        forced: bool,
        until: DateTime<Utc>,
        logger: slog::Logger,
        sender: Sender<AgentPayload>,
    ) -> Self {
        ThresholdTask {
            active: Arc::new(AtomicBool::new(true)),
            aborted: Arc::new(AtomicBool::new(false)),
            task_handle: None,
            task_config: Arc::new(ThresholdTaskConfig {
                forced: forced,
                logger: logger,
                sender: sender,
                until: until,
            }),
        }
    }

    pub fn abort(&mut self) {
        self.aborted.store(true, Ordering::Relaxed);
    }

    fn run(&mut self) {
        let task_aborted = self.aborted.clone();
        let task_active = self.active.clone();

        let config = self.task_config.clone();
        self.task_handle = Some(tokio::spawn(async move {
            let start = Utc::now();

            let mut state = AgentState::Active;
            if config.forced {
                state = AgentState::Forced(config.until.clone());
            }
            plugins_core::send_payload(&config.logger, &config.sender, AgentPayload::State(state));
            plugins_core::send_payload(&config.logger, &config.sender, AgentPayload::Bool(true));
            info!(config.logger, "{} started Task", NAME);

            while Utc::now() > config.until && !task_aborted.load(Ordering::Relaxed) {
                tokio::time::delay_for(std::time::Duration::from_millis(250)).await;
            }

            task_active.store(false, Ordering::Relaxed);
            state = AgentState::Default;
            plugins_core::send_payload(&config.logger, &config.sender, AgentPayload::State(state));
            plugins_core::send_payload(&config.logger, &config.sender, AgentPayload::Bool(false));

            let delta_secs = (start - Utc::now()).num_seconds();
            if task_aborted.load(Ordering::Relaxed) {
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
        }));
    }

    fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }
}
