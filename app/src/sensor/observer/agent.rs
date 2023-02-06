use super::ConcurrentObserver;
use crate::logging::APP_LOGGING;
use crate::models::{self};
use crate::{
    error::{DBError, ObserverError},
    rest::AgentDto,
};

use chrono_tz::Tz;
use ripe_core::AgentConfigType;
use std::collections::HashMap;
use std::sync::Arc;

pub struct AgentObserver {
    pub inner: Arc<ConcurrentObserver>,
}

impl Clone for AgentObserver {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl AgentObserver {
    pub fn new(inner: Arc<ConcurrentObserver>) -> Self {
        AgentObserver { inner }
    }

    pub async fn register(
        &self,
        sensor_id: i32,
        key_b64: String,
        domain: &String,
        agent_name: &String,
    ) -> Result<(), ObserverError> {
        let container = self.inner.container.read().await;
        let mut sensor = container
            .sensor(sensor_id, &key_b64)
            .await
            .ok_or(DBError::SensorNotFound(sensor_id))?;

        let factory = self.inner.agent_factory.read().await;
        let agent = factory.create_agent(sensor_id, &agent_name, &domain, None)?;
        models::create_agent_config(&self.inner.db_conn, &agent.deserialize()).await?;

        sensor.add_agent(agent);

        info!(
            APP_LOGGING,
            "Added agent {}, {} to sensor {}", agent_name, domain, sensor_id
        );
        Ok(())
    }

    pub async fn unregister(
        &self,
        sensor_id: i32,
        key_b64: String,
        domain: String,
        agent_name: String,
    ) -> Result<AgentDto, ObserverError> {
        let container = self.inner.container.read().await;
        let mut sensor = container
            .sensor(sensor_id, &key_b64)
            .await
            .ok_or(DBError::SensorNotFound(sensor_id))?;

        let agent = sensor
            .remove_agent(&agent_name, &domain)
            .ok_or(DBError::SensorNotFound(sensor_id))?;
        models::delete_sensor_agent(&self.inner.db_conn, sensor.id(), agent).await?;

        info!(
            APP_LOGGING,
            "Removed agent {}, {} from sensor {}", agent_name, domain, sensor_id
        );
        Ok(AgentDto { domain, agent_name })
    }

    pub async fn on_cmd(
        &self,
        sensor_id: i32,
        key_b64: String,
        domain: String,
        payload: i64,
    ) -> Result<AgentDto, ObserverError> {
        let container = self.inner.container.read().await;
        let mut sensor = container
            .sensor(sensor_id, &key_b64)
            .await
            .ok_or(DBError::SensorNotFound(sensor_id))?;

        let agent = sensor.handle_agent_cmd(&domain, payload)?;
        Ok(AgentDto {
            domain,
            agent_name: agent.agent_name().clone(),
        })
    }

    pub async fn config(
        &self,
        sensor_id: i32,
        key_b64: String,
        domain: String,
        timezone: Tz,
    ) -> Result<HashMap<String, (String, AgentConfigType)>, ObserverError> {
        let container = self.inner.container.read().await;
        let sensor = container
            .sensor(sensor_id, &key_b64)
            .await
            .ok_or(DBError::SensorNotFound(sensor_id))?;

        Ok(sensor
            .agent_config(&domain, timezone)
            .ok_or(DBError::SensorNotFound(sensor_id))?)
    }

    pub async fn set_config(
        &self,
        sensor_id: i32,
        key_b64: String,
        domain: String,
        config: HashMap<String, AgentConfigType>,
        timezone: Tz,
    ) -> Result<AgentDto, ObserverError> {
        let container = self.inner.container.write().await;
        let mut sensor = container
            .sensor(sensor_id, &key_b64)
            .await
            .ok_or(DBError::SensorNotFound(sensor_id))?;

        let agent = sensor.set_agent_config(&domain, config, timezone)?;
        models::update_agent_config(&self.inner.db_conn, &agent.deserialize()).await?;
        Ok(AgentDto {
            domain,
            agent_name: agent.agent_name().clone(),
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        config::CONFIG, models::establish_db_connection, sensor::observer::sensor::SensorObserver,
    };
    use chrono_tz::UTC;

    async fn build_mocked_observer() -> (AgentObserver, SensorObserver) {
        let plugin_path = CONFIG.plugin_dir();
        let plugin_dir = std::path::Path::new(&plugin_path);
        let db_conn = establish_db_connection().await.unwrap();
        let observer = ConcurrentObserver::new(plugin_dir, db_conn);
        (
            AgentObserver::new(observer.clone()),
            SensorObserver::new(observer),
        )
    }

    #[tokio::test]
    async fn test_register_agent() {
        // prepare
        let domain = "TEST".to_owned();
        let agent = "MockAgent".to_owned();
        let (agent_observer, sensor_observer) = build_mocked_observer().await;
        let sensor_res = sensor_observer.register(None).await.unwrap();

        // execute

        let res = agent_observer
            .register(sensor_res.id, sensor_res.key, &domain, &agent)
            .await;

        // validate
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_unregister_agent() {
        // prepare
        let domain = "TEST".to_owned();
        let agent = "MockAgent".to_owned();
        let (agent_observer, sensor_observer) = build_mocked_observer().await;
        let sensor_res = sensor_observer.register(None).await.unwrap();
        agent_observer
            .register(
                sensor_res.id,
                sensor_res.key.clone(),
                &domain.clone(),
                &agent.clone(),
            )
            .await
            .unwrap();

        // execute
        let res = agent_observer
            .unregister(sensor_res.id, sensor_res.key, domain, agent)
            .await;

        // validate
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_agent_config() {
        // prepare
        let domain = "TEST".to_owned();
        let agent = "MockAgent".to_owned();
        let (agent_observer, sensor_observer) = build_mocked_observer().await;
        let sensor_res = sensor_observer.register(None).await.unwrap();
        agent_observer
            .register(
                sensor_res.id,
                sensor_res.key.clone(),
                &domain.clone(),
                &agent.clone(),
            )
            .await
            .unwrap();

        // execute
        let config_res = agent_observer
            .config(sensor_res.id, sensor_res.key, domain, UTC)
            .await;

        // validate
        assert!(config_res.is_ok());
    }

    #[tokio::test]
    async fn test_set_agent_config() {
        // prepare
        let domain = "TEST".to_owned();
        let agent = "MockAgent".to_owned();
        let (agent_observer, sensor_observer) = build_mocked_observer().await;
        let sensor_res = sensor_observer.register(None).await.unwrap();
        agent_observer
            .register(
                sensor_res.id,
                sensor_res.key.clone(),
                &domain.clone(),
                &agent.clone(),
            )
            .await
            .unwrap();

        // execute
        let expected_config = HashMap::new();
        let config_res = agent_observer
            .set_config(sensor_res.id, sensor_res.key, domain, expected_config, UTC)
            .await;

        // validate
        assert!(config_res.is_ok());
    }
}
