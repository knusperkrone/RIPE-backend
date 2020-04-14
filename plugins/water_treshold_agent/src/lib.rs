use chrono::{DateTime, NaiveDateTime, Utc};
use plugins_core::*;
use serde::{Deserialize, Serialize};

const DOMAIN: &str = "Water";
const NAME: &str = "ThresholdWaterAgent";

export_plugin!(NAME, build_agent);

extern "C" fn build_agent(config: Option<&std::string::String>) -> Box<dyn AgentTrait> {
    if let Some(config_json) = config {
        if let Ok(deserialized) = serde_json::from_str::<ThresholdWaterAgent>(&config_json) {
            return Box::new(deserialized);
        } else {
            // LOG!
        }
    }

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
            //warn!(APP_LOGGING, "No moisture provided!");
            return None;
        }

        if data.moisture.unwrap_or(std::u32::MAX) < self.min_threshold {
            //
            Some(Payload::Bool(true))
        } else {
            None
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

    fn do_force(&mut self, until: DateTime<Utc>) {
        self.state = AgentState::Forced(until).into();
    }

    fn state(&self) -> AgentState {
        self.state.into_agent() 
    }

    fn deserialize(&self) -> AgentConfig {
        AgentConfig {
            domain: DOMAIN.to_owned(),
            name: NAME.to_owned(),
            state_json: serde_json::to_string(self).unwrap(),
        }
    }
}
