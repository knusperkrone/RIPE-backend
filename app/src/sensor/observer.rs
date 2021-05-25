use super::handle::{SensorHandle, SensorMQTTCommand};
use crate::logging::APP_LOGGING;
use crate::models::{
    self,
    dao::{AgentConfigDao, SensorDao},
};
use crate::mqtt::MqttSensorClient;
use crate::plugin::AgentFactory;
use crate::rest::{AgentStatusDto, SensorCredentialDto, SensorStatusDto};
use crate::{
    error::{DBError, ObserverError},
    rest::AgentDto,
};
use chrono::Utc;
use diesel::pg::PgConnection;
use ripe_core::{AgentConfigType, SensorDataMessage};
use notify::{watcher, Watcher};
use parking_lot::{Mutex, RawMutex, RwLock};
use std::{collections::HashMap, path::Path, sync::Arc, time::Duration};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

pub struct ConcurrentSensorObserver {
    plugin_dir: std::path::PathBuf,
    container: RwLock<SensorCache>,
    agent_factory: RwLock<AgentFactory>,
    iac_receiver: Mutex<UnboundedReceiver<SensorMQTTCommand>>,
    data_receveiver: Mutex<UnboundedReceiver<(i32, SensorDataMessage)>>,
    mqtt_client: MqttSensorClient,
    db_conn: Arc<Mutex<PgConnection>>,
}

impl ConcurrentSensorObserver {
    pub fn new(mqtt_name: String, plugin_dir: &Path, db_conn: PgConnection) -> Arc<Self> {
        let (iac_sender, iac_receiver) = unbounded_channel::<SensorMQTTCommand>();
        let (sensor_data_sender, data_receiver) = unbounded_channel::<(i32, SensorDataMessage)>();
        let agent_factory = AgentFactory::new(iac_sender);
        let container = SensorCache::new();
        let mqtt_client = MqttSensorClient::new(mqtt_name, sensor_data_sender);

        let observer = ConcurrentSensorObserver {
            mqtt_client,
            plugin_dir: plugin_dir.to_owned(),
            container: RwLock::new(container),
            agent_factory: RwLock::new(agent_factory),
            iac_receiver: Mutex::new(iac_receiver),
            data_receveiver: Mutex::new(data_receiver),
            db_conn: Arc::new(Mutex::new(db_conn)),
        };
        Arc::new(observer)
    }

    pub async fn init(&self) {
        self.load_plugins();
        self.mqtt_client.connect().await;
        self.populate_agents().await;
    }

    pub fn load_plugins(&self) {
        let mut agent_factory = self.agent_factory.write();
        agent_factory.load_new_plugins(self.plugin_dir.as_path());
    }

    /// Starts the paho-thread, registers all callbacks
    /// After a successful connection, the agent's get inited from the database
    /// On each message event the according sensor get's called and the data get's persistet
    /// Blocks caller thread in infinite loop
    pub async fn dispatch_mqtt_receive_loop(self: Arc<ConcurrentSensorObserver>) -> () {
        let reveiver_res = self.data_receveiver.try_lock();
        if reveiver_res.is_none() {
            error!(APP_LOGGING, "dispatch_mqtt_receive_loop() already called!");
            return;
        }
        let mut receiver = reveiver_res.unwrap();

        loop {
            info!(APP_LOGGING, "Start capturing sensor data events");
            while let Some((sensor_id, data)) = receiver.recv().await {
                if let Some(mut sensor) = self.container.write().sensor_unchecked(sensor_id) {
                    sensor.handle_data(&data);
                } else {
                    warn!(APP_LOGGING, "Sensor not found: {}", sensor_id);
                    break;
                }
                if let Err(e) = models::insert_sensor_data(&self.db_conn.lock(), sensor_id, data) {
                    error!(APP_LOGGING, "Failed persiting sensor data: {}", e);
                }
            }
        }
    }

    /// Dispatches the inter-agent-communication (iac) stream
    /// Each agent sends it's mqtt command over this channel
    /// Blocks caller thread in infinite loop
    pub async fn dispatch_mqtt_send_loop(self: Arc<ConcurrentSensorObserver>) -> () {
        let receiver_res = self.iac_receiver.try_lock();
        if receiver_res.is_none() {
            error!(APP_LOGGING, "dispatch_mqtt_send_loop() already called!");
            return;
        }

        let mut receiver = receiver_res.unwrap();
        loop {
            info!(APP_LOGGING, "Start capturing iac events");
            while let Some(item) = receiver.recv().await {
                let container = self.container.read();
                let sensor_opt = container.sensor_unchecked(item.sensor_id);
                if sensor_opt.is_some() {
                    if let Err(e) = self.mqtt_client.send_cmd(&sensor_opt.unwrap()).await {
                        error!(APP_LOGGING, "Failed sending command {:?} with {}", item, e);
                    }
                }
            }
        }
    }

    /// Dispatches a interval checking loop
    /// checks if the plugin libary files got updated
    /// Blocks caller thread in infinite loop
    pub async fn dispatch_plugin_refresh_loop(self: Arc<ConcurrentSensorObserver>) -> () {
        let plugin_dir = self.plugin_dir.as_path();

        // watch plugin dir
        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher = watcher(tx, Duration::from_secs(1)).unwrap();
        if let Err(e) = watcher.watch(&plugin_dir, notify::RecursiveMode::NonRecursive) {
            crit!(APP_LOGGING, "Cannot watch plugin dir: {}", e);
            return;
        }

        info!(APP_LOGGING, "Start watching plugin dir: {:?}", plugin_dir);
        loop {
            if let Ok(_) = rx.try_recv() {
                let mut agent_factory = self.agent_factory.write();
                let loaded_libs = agent_factory.load_new_plugins(plugin_dir);
                if !loaded_libs.is_empty() {
                    let lib_names = loaded_libs.iter().map(|e| &e.0).collect::<Vec<_>>();
                    info!(APP_LOGGING, "Updating plugins {:?}", lib_names);

                    let now = Utc::now();
                    let container = self.container.write();
                    let conn = self.db_conn.lock();
                    for sensor_mtx in container.sensors() {
                        let mut sensor = sensor_mtx.lock();
                        sensor.update(&lib_names, &agent_factory);

                        if let Ok(Some(data)) =
                            models::get_latest_sensor_data_unchecked(&conn, sensor.id())
                        {
                            let casted: SensorDataMessage = data.into();
                            if (casted.timestamp - now) < chrono::Duration::minutes(60) {
                                sensor.handle_data(&casted); // last hour
                            }
                        }
                    }
                }
            }
            let timeout = if cfg!(prod) { 10 } else { 1 };
            tokio::time::sleep(Duration::from_secs(timeout)).await;
        }
    }
}

/*
 * Sensor
 */

impl ConcurrentSensorObserver {
    pub async fn register_sensor(
        &self,
        name: Option<String>,
    ) -> Result<SensorCredentialDto, ObserverError> {
        // Create sensor dao
        let key = self.generate_sensor_key();
        let conn = self.db_conn.lock();
        let sensor_dao = models::create_new_sensor(&conn, key.clone(), &name)?;
        let dao_id = sensor_dao.id();

        // Create agents and persist
        let factory = self.agent_factory.read();
        match self.observe_sensor(&factory, sensor_dao, None).await {
            Ok((id, broker)) => {
                info!(APP_LOGGING, "Registered new sensor: {}", id);
                Ok(SensorCredentialDto { id, key, broker })
            }
            Err(err) => {
                models::delete_sensor(&conn, dao_id)?; // Fallback delete
                Err(ObserverError::from(err))
            }
        }
    }

    pub async fn unregister_sensor(
        &self,
        sensor_id: i32,
        key_b64: String,
    ) -> Result<SensorCredentialDto, ObserverError> {
        models::delete_sensor(&self.db_conn.lock(), sensor_id)?;

        let sensor_mtx = self.container.write().remove_sensor(sensor_id, &key_b64)?;
        let sensor = sensor_mtx.lock();
        self.mqtt_client.unsubscribe_sensor(&sensor).await?;

        info!(APP_LOGGING, "Removed sensor: {}", sensor_id);
        Ok(SensorCredentialDto {
            id: sensor_id,
            key: key_b64,
            broker: None,
        })
    }

    pub async fn sensor_status(
        &self,
        sensor_id: i32,
        key_b64: String,
    ) -> Result<SensorStatusDto, ObserverError> {
        // Cummulate and render sensors
        let container = self.container.read();
        let sensor = container
            .sensor(sensor_id, key_b64.as_str())
            .ok_or_else(|| DBError::SensorNotFound(sensor_id))?;

        // Get sensor data
        let data = match models::get_latest_sensor_data(&self.db_conn.lock(), sensor_id, &key_b64)?
        {
            Some(dao) => dao.into(),
            None => SensorDataMessage::default(),
        };

        let agents: Vec<AgentStatusDto> = sensor
            .agents()
            .iter()
            .map(|a| AgentStatusDto {
                domain: a.domain().clone(),
                agent_name: a.agent_name().clone(),
                ui: a.render_ui(&data),
            })
            .collect();

        info!(APP_LOGGING, "Fetched sensor status: {}", sensor_id);
        Ok(SensorStatusDto {
            name: sensor.name().clone(),
            data: data,
            agents: agents,
            broker: self.mqtt_client.broker(),
        })
    }

    /*
     * Agent
     */

    pub async fn agents(&self) -> Vec<String> {
        let agent_factory = self.agent_factory.read();
        agent_factory.agents()
    }

    pub async fn register_agent(
        &self,
        sensor_id: i32,
        key_b64: String,
        domain: String,
        agent_name: String,
    ) -> Result<AgentDto, ObserverError> {
        let container = self.container.read();
        let mut sensor = container
            .sensor(sensor_id, &key_b64)
            .ok_or(DBError::SensorNotFound(sensor_id))?;

        let factory = self.agent_factory.read();
        let agent = factory.create_agent(sensor_id, &agent_name, &domain, None)?;
        models::create_sensor_agent(&self.db_conn.lock(), agent.deserialize())?;

        sensor.add_agent(agent);

        info!(
            APP_LOGGING,
            "Added agent {}, {} to sensor {}", agent_name, domain, sensor_id
        );
        Ok(AgentDto { domain, agent_name })
    }

    pub async fn unregister_agent(
        &self,
        sensor_id: i32,
        key_b64: String,
        domain: String,
        agent_name: String,
    ) -> Result<AgentDto, ObserverError> {
        let container = self.container.read();
        let mut sensor = container
            .sensor(sensor_id, &key_b64)
            .ok_or(DBError::SensorNotFound(sensor_id))?;

        let agent = sensor
            .remove_agent(&agent_name, &domain)
            .ok_or(DBError::SensorNotFound(sensor_id))?;
        models::delete_sensor_agent(&self.db_conn.lock(), sensor.id(), agent)?;

        info!(
            APP_LOGGING,
            "Removed agent {}, {} from sensor {}", agent_name, domain, sensor_id
        );
        Ok(AgentDto { domain, agent_name })
    }

    pub async fn on_agent_cmd(
        &self,
        sensor_id: i32,
        key_b64: String,
        domain: String,
        payload: i64,
    ) -> Result<AgentDto, ObserverError> {
        let container = self.container.read();
        let mut sensor = container
            .sensor(sensor_id, &key_b64)
            .ok_or(DBError::SensorNotFound(sensor_id))?;

        let agent = sensor.handle_agent_cmd(&domain, payload)?;
        Ok(AgentDto {
            domain,
            agent_name: agent.agent_name().clone(),
        })
    }

    pub async fn agent_config(
        &self,
        sensor_id: i32,
        key_b64: String,
        domain: String,
    ) -> Result<HashMap<String, (String, AgentConfigType)>, ObserverError> {
        let container = self.container.read();
        let sensor = container
            .sensor(sensor_id, &key_b64)
            .ok_or(DBError::SensorNotFound(sensor_id))?;

        Ok(sensor
            .agent_config(&domain)
            .ok_or(DBError::SensorNotFound(sensor_id))?)
    }

    pub async fn set_agent_config(
        &self,
        sensor_id: i32,
        key_b64: String,
        domain: String,
        config: HashMap<String, AgentConfigType>,
    ) -> Result<AgentDto, ObserverError> {
        let container = self.container.write();
        let mut sensor = container
            .sensor(sensor_id, &key_b64)
            .ok_or(DBError::SensorNotFound(sensor_id))?;

        let agent = sensor.set_agent_config(&domain, config)?;
        models::update_sensor_agent(&self.db_conn.lock(), &agent.deserialize())?;
        Ok(AgentDto {
            domain,
            agent_name: agent.agent_name().clone(),
        })
    }

    /*
     * Helpers
     */

    async fn populate_agents(&self) {
        let conn = self.db_conn.lock();
        let sensor_daos = models::get_sensors(&conn).into_iter().map(|sensor_dao| {
            let configs = models::get_agent_config(&conn, &sensor_dao);
            (sensor_dao, configs)
        });

        let mut count = 0;
        let agent_factory = self.agent_factory.read();
        for (sensor_dao, agent_configs) in sensor_daos {
            let restore_result = self
                .observe_sensor(&agent_factory, sensor_dao, Some(&agent_configs))
                .await;
            match restore_result {
                Ok((id, _)) => {
                    count += 1;
                    debug!(APP_LOGGING, "Restored sensor {}", id)
                }
                Err(msg) => error!(APP_LOGGING, "{}", msg),
            }
        }
        info!(APP_LOGGING, "Restored {} sensors", count);
    }

    async fn observe_sensor(
        &self,
        agent_factory: &AgentFactory,
        sensor_dao: SensorDao,
        configs: Option<&Vec<AgentConfigDao>>,
    ) -> Result<(i32, Option<String>), ObserverError> {
        let sensor_id = sensor_dao.id();
        let sensor = SensorHandle::from(sensor_dao, configs.unwrap_or(&vec![]), &agent_factory)?;

        let mqtt_broker = self.register_sensor_mqtt(&sensor).await?;
        self.container.write().insert_sensor(sensor);
        Ok((sensor_id, mqtt_broker))
    }

    async fn register_sensor_mqtt(
        &self,
        sensor: &SensorHandle,
    ) -> Result<Option<String>, ObserverError> {
        self.mqtt_client.subscribe_sensor(sensor).await?;
        if let Err(e) = self.mqtt_client.send_cmd(sensor).await {
            error!(APP_LOGGING, "Failed sending initial mqtt command: {}", e);
        }
        Ok(self.mqtt_client.broker())
    }

    fn generate_sensor_key(&self) -> String {
        let mut buffer = Vec::with_capacity(6);
        for _ in 0..6 {
            buffer.push(rand::random::<u8>());
        }
        base64::encode(buffer)
            .replace('/', &"-")
            .replace('+', &"_")
            .replace('#', &"_")
    }
}

/*
 * SensorCache
 */

pub struct SensorCache {
    sensors: HashMap<i32, Mutex<SensorHandle>>,
}

impl SensorCache {
    pub fn new() -> Self {
        SensorCache {
            sensors: HashMap::new(),
        }
    }

    pub fn sensors(
        &self,
    ) -> std::collections::hash_map::Values<
        '_,
        i32,
        parking_lot::lock_api::Mutex<RawMutex, SensorHandle>,
    > {
        self.sensors.values().into_iter()
    }

    pub fn sensor_unchecked(
        &self,
        sensor_id: i32,
    ) -> Option<parking_lot::lock_api::MutexGuard<'_, RawMutex, SensorHandle>> {
        let sensor_mutex = self.sensors.get(&sensor_id)?;
        Some(sensor_mutex.lock())
    }

    pub fn sensor(
        &self,
        sensor_id: i32,
        key_b64: &str,
    ) -> Option<parking_lot::lock_api::MutexGuard<'_, RawMutex, SensorHandle>> {
        let sensor_mutex = self.sensors.get(&sensor_id)?;
        let sensor = sensor_mutex.lock();
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

    pub fn remove_sensor(
        &mut self,
        sensor_id: i32,
        key_b64: &str,
    ) -> Result<Mutex<SensorHandle>, DBError> {
        if let None = self.sensor(sensor_id, key_b64) {
            return Err(DBError::SensorNotFound(sensor_id));
        }

        if let Some(sensor_mtx) = self.sensors.remove(&sensor_id) {
            Ok(sensor_mtx)
        } else {
            Err(DBError::SensorNotFound(sensor_id))
        }
    }
}
