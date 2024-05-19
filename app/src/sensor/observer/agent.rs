use super::ConcurrentObserver;
use crate::error::{DBError, ObserverError};
use crate::models::{
    agent,
    agent_command::{self, AgentCommandDao},
};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use ripe_core::AgentConfigType;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::info;

pub struct AgentObserver {
    inner: Arc<ConcurrentObserver>,
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
        key_b64: &str,
        domain: &String,
        agent_name: &String,
    ) -> Result<(), ObserverError> {
        let container = self.inner.container.read().await;
        let mut sensor = container
            .sensor(sensor_id, key_b64)
            .await
            .ok_or(DBError::SensorNotFound(sensor_id))?;

        let factory = self.inner.agent_factory.read().await;
        let agent = factory.create_agent(sensor_id, agent_name, domain, None)?;
        agent::insert(&self.inner.db_conn, &agent.deserialize()).await?;

        sensor.add_agent(agent);

        info!(
            sensor_id = sensor.id(),
            agent_name = agent_name,
            domain = domain,
            "Added agent to sensor",
        );
        Ok(())
    }

    pub async fn unregister(
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

        let agent = sensor
            .remove_agent(agent_name, domain)
            .ok_or(DBError::SensorNotFound(sensor_id))?;
        agent::delete(&self.inner.db_conn, sensor.id(), agent).await?;

        info!(
            sensor_id = sensor_id,
            agent_name = agent_name,
            domain = domain,
            "Removed agent from sensor"
        );
        Ok(())
    }

    pub async fn on_cmd(
        &self,
        sensor_id: i32,
        key_b64: String,
        domain: &String,
        payload: i64,
    ) -> Result<(), ObserverError> {
        let container = self.inner.container.read().await;
        let mut sensor = container
            .sensor(sensor_id, &key_b64)
            .await
            .ok_or(DBError::SensorNotFound(sensor_id))?;

        sensor.handle_agent_cmd(domain, payload)?;
        Ok(())
    }

    pub async fn commands<T>(
        &self,
        sensor_id: i32,
        domain: &str,
        from: DateTime<Utc>,
        until: DateTime<Utc>,
    ) -> Result<Vec<T>, ObserverError>
    where
        T: From<AgentCommandDao>,
    {
        let container = self.inner.container.read().await;
        let _ = container
            .sensor(sensor_id, domain)
            .await
            .ok_or(DBError::SensorNotFound(sensor_id))?;

        let mut commands = agent_command::get(&self.inner.db_conn, sensor_id, from, until)
            .await
            .ok()
            .ok_or(DBError::SensorNotFound(sensor_id))?;

        let transformed: Vec<T> = commands.drain(..).map(|dao| T::from(dao)).collect();
        Ok(transformed)
    }

    pub async fn config(
        &self,
        sensor_id: i32,
        key_b64: String,
        domain: &String,
        timezone: Tz,
    ) -> Result<HashMap<String, (String, AgentConfigType)>, ObserverError> {
        let container = self.inner.container.read().await;
        let sensor = container
            .sensor(sensor_id, &key_b64)
            .await
            .ok_or(DBError::SensorNotFound(sensor_id))?;

        Ok(sensor
            .agent_config(domain, timezone)
            .ok_or(DBError::SensorNotFound(sensor_id))?)
    }

    pub async fn set_config(
        &self,
        sensor_id: i32,
        key_b64: String,
        domain: &String,
        config: HashMap<String, AgentConfigType>,
        timezone: Tz,
    ) -> Result<(), ObserverError> {
        let container = self.inner.container.write().await;
        let mut sensor = container
            .sensor(sensor_id, &key_b64)
            .await
            .ok_or(DBError::SensorNotFound(sensor_id))?;

        let agent = sensor.set_agent_config(domain, config, timezone)?;
        agent::update(&self.inner.db_conn, &agent.deserialize()).await?;
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        config::CONFIG, models::establish_db_connection,
        sensor::observer::controller::SensorObserver,
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
            .register(sensor_res.id, &sensor_res.key, &domain, &agent)
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
                &sensor_res.key.clone(),
                &domain.clone(),
                &agent.clone(),
            )
            .await
            .unwrap();

        // execute
        let res = agent_observer
            .unregister(sensor_res.id, sensor_res.key, &domain, &agent)
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
                &sensor_res.key.clone(),
                &domain.clone(),
                &agent.clone(),
            )
            .await
            .unwrap();

        // execute
        let config_res = agent_observer
            .config(sensor_res.id, sensor_res.key, &domain, UTC)
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
                &sensor_res.key.clone(),
                &domain.clone(),
                &agent.clone(),
            )
            .await
            .unwrap();

        // execute
        let expected_config = HashMap::new();
        let config_res = agent_observer
            .set_config(sensor_res.id, sensor_res.key, &domain, expected_config, UTC)
            .await;

        // validate
        assert!(config_res.is_ok());
    }
}
