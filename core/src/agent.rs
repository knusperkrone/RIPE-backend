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

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub enum AgentConfigType {
    Switch(bool),                                     // val
    DateTime(u64),                     // val
    IntRange(i64, i64, i64),                          // lower, upper, val
    IntSliderRange(SliderFormatter, i64, i64, i64),   // formatter, lower, upper, val
    FloatRange(f64, f64, f64),                        // lower, upper, val
    FloatSliderRange(SliderFormatter, f64, f64, f64), // formatter, lower, upper, val
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub enum SliderFormatter {
    Time(u32),
    Linear,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub enum AgentUIDecorator {
    TimePane(u32),         // ui_stepsize
    Slider(f32, f32, f32), // Range, Value
}

impl AgentUIDecorator {
    pub fn transform_cmd_timepane(payload: i64) -> Option<DateTime<Utc>> {
        Utc::now().checked_add_signed(Duration::seconds(payload))
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
    fn render_ui(&self, data: &SensorDataMessage) -> AgentUI;

    // framework logic
    fn state(&self) -> AgentState;
    fn cmd(&self) -> i32;
    fn deserialize(&self) -> AgentConfig;

    // user config
    fn config(&self) -> HashMap<&str, (&str, AgentConfigType)>; // key, translation, ui
    fn set_config(&mut self, values: &HashMap<String, AgentConfigType>) -> bool;
}
