use crate::agent::{Agent, AgentTrait, State};
use crate::sensor::SensorData;
use chrono::{DateTime, NaiveDateTime, Utc};
use std::time::Duration;

pub fn new(config: &AgentRegisterConfig) -> Option<Agent> {
    if let Some(agent_impl) = &config.agent_impl {
        match agent_impl.as_str() {
            TresholdWaterAgent::IDENTIFIER => TresholdWaterAgent::from(&config.config_json),
            _ => None,
        }
    } else {
        TresholdWaterAgent::from(&None)
    }
}

#[derive(std::fmt::Debug, PartialEq)]
pub struct TresholdWaterAgent {
    state: State,
    min_treshold: u32,
    watering_start: Option<DateTime<Utc>>,
    last_watering: Option<DateTime<Utc>>,
}

impl TresholdWaterAgent {
    pub const IDENTIFIER: &'static str = "TresholdWaterAgent";

    pub fn from(_config_json: &Option<String>) -> Option<Agent> {
        // TODO: Implement
        Some(TresholdWaterAgent::new())
    }

    pub fn new() -> Agent {
        Agent::TresholdWaterAgent(TresholdWaterAgent {
            state: State::Active,
            min_treshold: 20,
            watering_start: None,
            last_watering: None,
        })
    }
}

impl AgentTrait for TresholdWaterAgent {
    fn state(&self) -> &State {
        &self.state
    }

    fn on_force(&mut self, duration: Duration) {
        self.state = State::Forced(duration);
    }

    fn do_action(&mut self, data: SensorData) {
        let _now = Utc::now();
        let _watering_diff = self
            .last_watering
            .unwrap_or(DateTime::from_utc(NaiveDateTime::from_timestamp(0, 0), Utc))
            - data.timestamp;

        if data.moisture.unwrap_or(std::u32::MAX) < self.min_treshold {
            //
        }
        /*
        moisture = sensor_data.moisture

        if moisture is None or not self.global_settings.is_in_quiet_range(now):
            logger.debug(f"Not watering as no data or in quiet range!")
        elif now - self.last_watering < timedelta(minutes=_LAST_WATERING_MIN_TIMEOUT):
            logger.debug(f"Not watering as last watering is only {_LAST_WATERING_MIN_TIMEOUT} mins passed")
        elif moisture < self.config_min_threshold and self.last_moisture < self.config_min_threshold:
            logger.debug(f"Watering with: {moisture}% and {self.last_moisture}%")
            self._water()

        self.last_moisture = moisture
            */
    }
}
