use super::*;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct AgentConfig {
    pub name: String,
    pub state_json: String,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct AgentUI {
    pub decorator: AgentUIDecorator,
    pub state: AgentState,
    pub rendered: String, // pub config_rendered: String
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub enum AgentConfigType {
    Switch(bool),                   // val
    DayTime(u64),                   // ms_of_day
    TimeSlider(i64, i64, i64, i64), // lower_ms, upper_ms, val_ms, stepsize_ms
    IntSlider(i64, i64, i64),       // lower, upper, val
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub enum AgentUIDecorator {
    Text,
    TimePane(u32),         // ui_stepsize
    Slider(f32, f32, f32), // Range, Value
}

impl AgentUIDecorator {
    pub fn transform_cmd_timepane(payload: i64) -> Option<DateTime<Utc>> {
        Utc::now().checked_add_signed(Duration::seconds(payload.abs()))
    }

    pub fn transform_cmd_slider(payload: i64) -> f32 {
        (payload as f64 / 1000.0) as f32
    }
}

#[derive(PartialEq, Debug, Deserialize, Serialize, Clone, Copy)]
pub enum AgentState {
    Disabled,
    Ready,
    Executing(DateTime<Utc>),
    Stopped(DateTime<Utc>),
    Forced(DateTime<Utc>),
    Error,
}

impl Default for AgentState {
    fn default() -> Self {
        AgentState::Ready
    }
}

pub trait AgentTrait: std::fmt::Debug + Send + Sync {
    // event busness logic
    fn handle_data(&mut self, data: &SensorDataMessage);
    fn handle_cmd(&mut self, payload: i64);

    // ui related
    fn render_ui(&self, data: &SensorDataMessage, timezone: chrono_tz::Tz) -> AgentUI;

    // framework logic
    fn state(&self) -> AgentState;
    fn cmd(&self) -> i32;
    fn deserialize(&self) -> String;

    // user config
    fn config(&self, timezone: chrono_tz::Tz) -> HashMap<String, (String, AgentConfigType)>; // key, translation, ui
    fn set_config(
        &mut self,
        values: &HashMap<String, AgentConfigType>,
        timezone: chrono_tz::Tz,
    ) -> bool;
}

///
/// TEST
///
#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    async fn test_print_serialized_agent_state() {
        println!(
            "Slider: {}",
            serde_json::json!(AgentUIDecorator::Slider(0.0, 1.0, 0.5))
        );
        println!(
            "TimePane: {}",
            serde_json::json!(AgentUIDecorator::TimePane(60))
        );

        println!(
            "Active: {}",
            serde_json::json!(AgentState::Executing(Utc::now()))
        );
        println!(
            "Forced: {}",
            serde_json::json!(AgentState::Forced(Utc::now()))
        );
        println!("Ready: {}", serde_json::json!(AgentState::Ready));
        println!("Default: {}", serde_json::json!(AgentState::Disabled));
        println!("Error: {}", serde_json::json!(AgentState::Error));
    }

    #[tokio::test]
    async fn test_print_serialized_agent_config() {
        println!(
            "Switch: {}",
            serde_json::json!(AgentConfigType::Switch(true))
        );
        println!(
            "DayTime: {}",
            serde_json::json!(AgentConfigType::DayTime(0))
        );
        println!(
            "TimeSliderRange: {}",
            serde_json::json!(AgentConfigType::TimeSlider(0, 3600, 0, 10))
        );
        println!(
            "IntSliderRange: {}",
            serde_json::json!(AgentConfigType::IntSlider(0, 5, 3))
        );

        let mut map = HashMap::<String, (String, AgentConfigType)>::new();
        map.insert(
            "key".to_owned(),
            ("Some Text".to_owned(), AgentConfigType::Switch(false)),
        );

        println!("Example config: {}", serde_json::json!(map))
    }

    #[tokio::test]
    async fn test_print_serialized_agent_ui_decorator() {
        println!("Text: {}", serde_json::json!(AgentUIDecorator::Text));
        println!(
            "TimePane: {}",
            serde_json::json!(AgentUIDecorator::TimePane(5))
        );
        println!(
            "Slider: {}",
            serde_json::json!(AgentUIDecorator::Slider(0.0, 0.0, 0.0))
        );
    }
}
