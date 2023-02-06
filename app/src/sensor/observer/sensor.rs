use super::ConcurrentObserver;
use crate::error::{DBError, ObserverError};
use crate::logging::APP_LOGGING;
use crate::models::dao::SensorDataDao;
use crate::models::{self};
use crate::mqtt::Broker;
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use ripe_core::SensorDataMessage;
use std::sync::Arc;

use super::{Agent, Sensor};

pub struct SensorObserver {
    inner: Arc<ConcurrentObserver>,
}

impl Clone for SensorObserver {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl SensorObserver {
    pub fn new(inner: Arc<ConcurrentObserver>) -> Self {
        SensorObserver { inner }
    }

    pub async fn register(&self, name: Option<String>) -> Result<Sensor, ObserverError> {
        // Create sensor dao
        let key = self.generate_sensor_key();
        let sensor_dao = models::create_new_sensor(&self.inner.db_conn, key.clone(), &name).await?;
        let dao_id = sensor_dao.id();

        // Create agents and persist
        let factory = self.inner.agent_factory.read().await;
        match self.inner.insert_sensor(&factory, sensor_dao, None).await {
            Ok(id) => {
                info!(APP_LOGGING, "Registered new sensor: {}", id);
                Ok(Sensor { id, key })
            }
            Err(err) => {
                models::delete_sensor(&self.inner.db_conn, dao_id).await?; // Fallback delete
                Err(ObserverError::from(err))
            }
        }
    }

    pub async fn unregister(&self, sensor_id: i32, key_b64: &String) -> Result<(), ObserverError> {
        models::delete_sensor(&self.inner.db_conn, sensor_id).await?;

        let sensor_mtx = self
            .inner
            .container
            .write()
            .await
            .remove_sensor(sensor_id, &key_b64)
            .await?;
        let sensor = sensor_mtx.lock().await;
        self.inner.mqtt_client.unsubscribe_sensor(&sensor).await?;

        info!(APP_LOGGING, "Removed sensor: {}", sensor_id);
        Ok(())
    }

    pub async fn status(
        &self,
        sensor_id: i32,
        key_b64: String,
        timezone: Tz,
    ) -> Result<(SensorDataMessage, Vec<Agent>), ObserverError> {
        // Cummulate and render sensors
        let container = self.inner.container.read().await;
        let sensor = container
            .sensor(sensor_id, key_b64.as_str())
            .await
            .ok_or_else(|| DBError::SensorNotFound(sensor_id))?;

        // Get sensor data
        let data =
            match models::get_latest_sensor_data(&self.inner.db_conn, sensor_id, &key_b64).await? {
                Some(dao) => dao.into(),
                None => SensorDataMessage::default(),
            };

        let agents: Vec<Agent> = sensor
            .agents()
            .iter()
            .map(|a| Agent {
                domain: a.domain().clone(),
                name: a.agent_name().clone(),
                ui: a.render_ui(&data, timezone),
            })
            .collect();

        debug!(APP_LOGGING, "Fetched sensor status: {}", sensor_id);
        Ok((data, agents))
    }

    pub async fn logs(
        &self,
        sensor_id: i32,
        key_b64: &String,
        tz: Tz,
    ) -> Result<Vec<String>, ObserverError> {
        if !models::sensor_exists(&self.inner.db_conn, sensor_id, key_b64).await {
            return Err(DBError::SensorNotFound(sensor_id).into());
        }

        let mut logs = models::get_sensor_logs(&self.inner.db_conn, sensor_id).await?;
        Ok(logs
            .drain(..)
            .map(|l| format!("[{}] {}", l.time(&tz).format(&"%b %e %T %Y"), l.log()))
            .collect())
    }

    pub async fn data<T>(
        &self,
        sensor_id: i32,
        key_b64: &String,
        from: DateTime<Utc>,
        until: DateTime<Utc>,
    ) -> Result<Vec<T>, ObserverError>
    where
        T: From<SensorDataDao>,
    {
        if !models::sensor_exists(&self.inner.db_conn, sensor_id, key_b64).await {
            return Err(DBError::SensorNotFound(sensor_id).into());
        }

        let mut data = models::get_sensor_data(&self.inner.db_conn, sensor_id, from, until).await?;
        let transformed: Vec<T> = data.drain(..).map(|dao| T::from(dao)).collect();

        Ok(transformed)
    }

    pub fn broker(&self) -> Broker {
        self.inner.mqtt_client.broker()
    }

    /*
     * Helpers
     */

    fn generate_sensor_key(&self) -> String {
        use rand::distributions::Alphanumeric;
        use rand::{thread_rng, Rng};
        let mut rng = thread_rng();
        std::iter::repeat(())
            .map(|()| rng.sample(Alphanumeric))
            .map(char::from)
            .take(6)
            .collect()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{config::CONFIG, models::establish_db_connection, sensor::ConcurrentObserver};
    use chrono_tz::UTC;

    async fn build_mocked_observer() -> SensorObserver {
        let plugin_path = CONFIG.plugin_dir();
        let plugin_dir = std::path::Path::new(&plugin_path);
        let db_conn = establish_db_connection().await.unwrap();
        SensorObserver::new(ConcurrentObserver::new(plugin_dir, db_conn))
    }

    #[tokio::test]
    async fn test_insert_sensor() {
        // prepare
        let observer = build_mocked_observer().await;

        // Execute
        let mut results = Vec::<i32>::new();
        for _ in 0..2 {
            let res = observer.register(None).await;

            let resp = res.unwrap();
            results.push(resp.id);
        }

        // Validate
        assert_ne!(results[0], results[1]);
    }

    #[tokio::test]
    async fn test_unregister_sensor() {
        // prepare
        let observer = build_mocked_observer().await;
        let cred = observer.register(None).await.unwrap();

        // execute
        let res = observer.unregister(cred.id, &cred.key).await;

        // validate
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_invalid_remove_sensor() {
        // prepare
        let observer = build_mocked_observer().await;
        let remove_id = -1;
        let remove_key = "asdase".to_owned();

        // execute
        let res = observer.unregister(remove_id, &remove_key).await;

        // validate
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn test_sensor_status() {
        // prepare
        let observer = build_mocked_observer().await;
        let sensor_res = observer.register(None).await.unwrap();

        // execute
        let res = observer.status(sensor_res.id, sensor_res.key, UTC).await;

        // validate
        assert!(res.is_ok());
    }
}
