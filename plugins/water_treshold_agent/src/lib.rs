#[macro_use]
extern crate slog;

use chrono::{DateTime, NaiveDateTime, Utc};
use once_cell::sync::OnceCell;
use plugins_core::*;
use serde::{Deserialize, Serialize};

const NAME: &str = "ThresholdWaterAgent";

export_plugin!(NAME, build_agent);

static LOGGER: OnceCell<slog::Logger> = OnceCell::new();

fn log() -> &'static slog::Logger {
    LOGGER.get().unwrap()
}

extern "C" fn build_agent(
    config: Option<&std::string::String>,
    logger: slog::Logger,
) -> Box<dyn AgentTrait> {
    if let Err(_) = LOGGER.set(logger) {
        error!(log(), "{} AND PLUGIN WAS ALREADY INITED?", NAME);
        panic!();
    }
    
    if let Some(config_json) = config {
        if let Ok(deserialized) = serde_json::from_str::<ThresholdWaterAgent>(&config_json) {
            error!(log(), "Restored {} from config", NAME);
            return Box::new(deserialized);
        } else {
            error!(
                log(),
                "{} coulnd't get restored from config {}", NAME, config_json
            );
        }
    }

    info!(log(), "Created new {}", NAME);
    Box::new(ThresholdWaterAgent {
        state: AgentState::Active.into(),
        min_threshold: 20,
        watering_start: None,
        last_watering: None,
    })
}

#[derive(Debug, PartialEq, Deserialize, Serialize)]
enum SerializeState {
    Active,
    Forced(DateTime<Utc>),
}

impl SerializeState {
    fn into_agent(&self) -> AgentState {
        match self {
            SerializeState::Active => AgentState::Active,
            SerializeState::Forced(time) => AgentState::Forced(time.clone()),
        }
    }
}

impl From<AgentState> for SerializeState {
    fn from(other: AgentState) -> Self {
        match other {
            AgentState::Active => SerializeState::Active,
            AgentState::Forced(time) => SerializeState::Forced(time.clone()),
        }
    }
}

#[derive(Debug, PartialEq, Deserialize, Serialize)]
pub struct ThresholdWaterAgent {
    state: SerializeState,
    min_threshold: u32,
    watering_start: Option<DateTime<Utc>>,
    last_watering: Option<DateTime<Utc>>,
}

impl AgentTrait for ThresholdWaterAgent {
    fn do_action(&mut self, data: &SensorData) -> Option<Payload> {
        let _now = Utc::now();
        let _watering_diff = self
            .last_watering
            .unwrap_or(DateTime::from_utc(NaiveDateTime::from_timestamp(0, 0), Utc))
            - data.timestamp;

        if data.moisture.is_none() {
            warn!(log(), "{} no moisture provided", NAME);
            return None;
        }

        let moisture = data.moisture.unwrap_or(std::u32::MAX);
        if moisture < self.min_threshold {
            info!(log(), "{} moisture below threshold", NAME);
            Some(Payload::Bool(true))
        } else {
            info!(log(), "{} moisture was fine {}%", NAME, moisture);
            None
        }
    }

    fn do_force(&mut self, until: DateTime<Utc>) {
        self.state = AgentState::Forced(until).into();
    }

    fn state(&self) -> AgentState {
        self.state.into_agent()
    }

    fn deserialize(&self) -> AgentConfig {
        AgentConfig {
            name: NAME.to_owned(),
            state_json: serde_json::to_string(self).unwrap(),
        }
    }
}
