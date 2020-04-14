use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq)]
pub struct SensorData {
    pub timestamp: DateTime<Utc>,
    pub refresh: Option<bool>,
    pub temperature: Option<f32>,
    pub light: Option<u32>,
    pub moisture: Option<u32>,
    pub conductivity: Option<u32>,
    pub battery: Option<u32>,
    pub carbon: Option<u32>,
}

impl std::default::Default for SensorData {
    fn default() -> Self {
        SensorData {
            timestamp: Utc::now(),
            refresh: None,
            temperature: None,
            light: None,
            moisture: None,
            conductivity: None,
            battery: None,
            carbon: None,
        }
    }
}
