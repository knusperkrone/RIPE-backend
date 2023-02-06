use super::handle::SensorHandle;
use crate::error::DBError;

use std::collections::HashMap;
use tokio::sync::Mutex;

pub struct SensorContainer {
    sensors: HashMap<i32, Mutex<SensorHandle>>,
}

impl SensorContainer {
    pub fn new() -> Self {
        SensorContainer {
            sensors: HashMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.sensors.len()
    }

    pub fn sensors(
        &self,
    ) -> std::collections::hash_map::Values<'_, i32, tokio::sync::Mutex<SensorHandle>> {
        self.sensors.values().into_iter()
    }

    pub async fn sensor_unchecked(
        &self,
        sensor_id: i32,
    ) -> Option<tokio::sync::MutexGuard<'_, SensorHandle>> {
        let sensor_mutex = self.sensors.get(&sensor_id)?;
        Some(sensor_mutex.lock().await)
    }

    pub async fn sensor(
        &self,
        sensor_id: i32,
        key_b64: &str,
    ) -> Option<tokio::sync::MutexGuard<'_, SensorHandle>> {
        let sensor_mutex = self.sensors.get(&sensor_id)?;
        let sensor = sensor_mutex.lock().await;
        if sensor.key_b64() == key_b64 {
            Some(sensor)
        } else {
            None
        }
    }

    pub fn insert_sensor(&mut self, sensor: SensorHandle) {
        // TODO: Read-write lock
        self.sensors.insert(sensor.id(), Mutex::new(sensor));
    }

    pub async fn remove_sensor(
        &mut self,
        sensor_id: i32,
        key_b64: &str,
    ) -> Result<Mutex<SensorHandle>, DBError> {
        if let None = self.sensor(sensor_id, key_b64).await {
            return Err(DBError::SensorNotFound(sensor_id));
        }

        if let Some(sensor_mtx) = self.sensors.remove(&sensor_id) {
            Ok(sensor_mtx)
        } else {
            Err(DBError::SensorNotFound(sensor_id))
        }
    }
}
