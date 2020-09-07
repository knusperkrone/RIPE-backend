use crate::error::{DBError, ObserverError};
use crate::plugin::agent::AgentFactory;

use super::handle::{SensorHandle, SensorHandleMessage};
use crate::logging::APP_LOGGING;
use crate::models::{
    self,
    dao::{AgentConfigDao, SensorDao},
    establish_db_connection,
};
use crate::mqtt::MqttSensorClient;
use crate::rest::{AgentRegisterDto, AgentStatusDto, SensorRegisterResponseDto, SensorStatusDto};
use diesel::pg::PgConnection;
use iftem_core::SensorDataMessage;
use notify::{watcher, Watcher};
use std::{
    collections::{hash_map::Values, HashMap},
    sync::Arc,
    time::Duration,
};
use tokio::{
    stream::StreamExt,
    sync::mpsc::Receiver,
    sync::{Mutex, RwLock},
};
pub struct ConcurrentSensorObserver {
    container_ref: Arc<std::sync::RwLock<SensorCache>>,
    agent_factory: Mutex<AgentFactory>,
    receiver: Arc<Mutex<Receiver<SensorHandleMessage>>>,
    mqtt_client: RwLock<MqttSensorClient>,
    db_conn: Mutex<PgConnection>,
}

unsafe impl Send for ConcurrentSensorObserver {}
unsafe impl Sync for ConcurrentSensorObserver {}

// Init
impl ConcurrentSensorObserver {
    pub fn new() -> Arc<Self> {
        let db_conn = establish_db_connection();
        let (sender, receiver) = tokio::sync::mpsc::channel::<SensorHandleMessage>(128);
        let agent_factory = AgentFactory::new(sender);
        let container_ref = Arc::new(std::sync::RwLock::new(SensorCache::new()));
        let client = MqttSensorClient::new(container_ref.clone());
        let observer = ConcurrentSensorObserver {
            container_ref: container_ref,
            agent_factory: Mutex::new(agent_factory),
            receiver: Arc::new(Mutex::new(receiver)),
            mqtt_client: RwLock::new(client),
            db_conn: Mutex::new(db_conn),
        };

        Arc::new(observer)
    }

    pub async fn dispatch_plugin(self: Arc<ConcurrentSensorObserver>) -> () {
        let path = std::env::var("PLUGIN_DIR").expect("PLUGIN_DIR must be set");
        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher = watcher(tx, Duration::from_secs(10)).unwrap();
        watcher
            .watch(&path, notify::RecursiveMode::NonRecursive)
            .unwrap();

        info!(APP_LOGGING, "Start watching plugin dir: {}", path);
        loop {
            if let Ok(_) = rx.try_recv() { // Non blocking
                info!(APP_LOGGING, "Plugin changes registered - reloading");
                let mut factory = self.agent_factory.lock().await;
                unsafe {
                    factory.load_plugins();
                }
            }
            tokio::time::delay_for(Duration::from_secs(5)).await;
        }
    }

    pub async fn dispatch_ipc(self: Arc<ConcurrentSensorObserver>) -> () {
        let receiver = self.receiver.clone();
        let receiver_res = receiver.try_lock();
        if receiver_res.is_err() {
            error!(APP_LOGGING, "dispatch_ipc() already called!");
            return;
        }

        let mut receiver = receiver_res.unwrap();
        loop {
            info!(APP_LOGGING, "Start capturing ipc-plugin events");
            while let Some(item) = receiver.next().await {
                let container = self.container_ref.read().unwrap();
                let sensor_opt = container.sensor_unchecked(item.sensor_id);
                if sensor_opt.is_some() {
                    let mut mqtt_client = self.mqtt_client.write().await;
                    if let Err(e) = mqtt_client.send_cmd(&sensor_opt.unwrap()) {
                        error!(APP_LOGGING, "Failed sending command {:?} with {}", item, e);
                    }
                }
            }
        }
    }
}

// MQTT
impl ConcurrentSensorObserver {
    pub async fn dispatch_mqtt(self: Arc<ConcurrentSensorObserver>) -> () {
        self.mqtt_client.read().await.connect();
        self.populate().await; // Subscribing to persisted sensors
    }

    async fn populate(&self) {
        let db_conn = self.db_conn.lock().await;
        let mut persisted_sensors = models::get_sensors(&db_conn);
        let mut sensor_configs: Vec<Vec<AgentConfigDao>> = persisted_sensors
            .iter()
            .map(|dao| models::get_agent_config(&db_conn, &dao)) // retrieve agent config
            .collect();

        while !persisted_sensors.is_empty() {
            let sensor_dao = persisted_sensors.pop().unwrap();
            let agent_configs = sensor_configs.pop().unwrap();
            let restore_result = self
                .register_sensor_dao(sensor_dao, Some(&agent_configs))
                .await;
            match restore_result {
                Ok(id) => info!(APP_LOGGING, "Restored sensor {}", id),
                Err(msg) => error!(APP_LOGGING, "{}", msg),
            }
        }
    }
}

// REST
impl ConcurrentSensorObserver {
    /*
     * Sensor
     */

    pub async fn register_new_sensor(
        &self,
        name: &Option<String>,
    ) -> Result<SensorRegisterResponseDto, ObserverError> {
        // Create sensor
        let key_b64 = self.generate_sensor_key();
        let conn = self.db_conn.lock().await;
        let sensor_dao = models::create_new_sensor(&conn, key_b64.clone(), name)?;
        let dao_id = sensor_dao.id();

        // Create agents and persist
        match self.register_sensor_dao(sensor_dao, None).await {
            Ok(_) => {
                info!(APP_LOGGING, "Registered new sensor: {}", dao_id);
                Ok(SensorRegisterResponseDto {
                    id: dao_id,
                    key: key_b64,
                })
            }
            Err(err) => {
                models::delete_sensor(&conn, dao_id)?; // Fallback delete
                Err(ObserverError::from(err))
            }
        }
    }

    pub async fn remove_sensor(&self, sensor_id: i32) -> Result<(), ObserverError> {
        let conn = self.db_conn.lock().await;
        models::delete_sensor(&conn, sensor_id)?;

        let sensor_mtx = self
            .container_ref
            .write()
            .unwrap()
            .remove_sensor(sensor_id)?;
        let sensor = sensor_mtx.lock().unwrap();
        self.mqtt_client.write().await.unsubscribe_sensor(&sensor)?;

        info!(APP_LOGGING, "Removed sensor: {}", sensor_id);
        Ok(())
    }

    pub async fn sensor_status(
        &self,
        sensor_id: i32,
        key_b64: String,
    ) -> Result<SensorStatusDto, ObserverError> {
        // Get sensor data
        let conn = self.db_conn.lock().await;
        let data = match models::get_latest_sensor_data(&conn, sensor_id, &key_b64)? {
            Some(dao) => dao.into(),
            None => SensorDataMessage::default(),
        };
        drop(conn);

        // Cummulate and render sensors
        let container = self.container_ref.read().unwrap();
        let sensor = container
            .sensor(sensor_id, key_b64.as_str())
            .ok_or_else(|| DBError::SensorNotFound(sensor_id))?;

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
        })
    }

    pub async fn reload_sensor(
        &self,
        sensor_id: i32,
        key_b64: String,
    ) -> Result<(), ObserverError> {
        let container = self.container_ref.read().unwrap();
        let mut sensor = container
            .sensor(sensor_id, &key_b64)
            .ok_or(DBError::SensorNotFound(sensor_id))?;

        let factory = self.agent_factory.lock().await;
        sensor.reload(&factory)?;

        info!(APP_LOGGING, "Reloaded sensor: {}", sensor_id);
        Ok(())
    }

    /*
     * Agent
     */

    pub async fn agents(&self) -> Vec<String> {
        let container_factory = self.agent_factory.lock().await;

        info!(APP_LOGGING, "fetched active agents");
        container_factory.agents()
    }

    pub async fn register_agent(
        &self,
        sensor_id: i32,
        key_b64: String,
        request: AgentRegisterDto,
    ) -> Result<(), ObserverError> {
        let container = self.container_ref.read().unwrap();
        let mut sensor = container
            .sensor(sensor_id, &key_b64)
            .ok_or(DBError::SensorNotFound(sensor_id))?;

        let factory = self.agent_factory.lock().await;
        let conn = self.db_conn.lock().await;
        let agent = factory.new_agent(sensor_id, &request.agent_name, &request.domain)?;
        models::create_sensor_agent(&conn, agent.deserialize())?;
        sensor.add_agent(agent);

        info!(
            APP_LOGGING,
            "Added agent {}, {} to sensor {}", request.agent_name, request.domain, sensor_id
        );
        Ok(())
    }

    pub async fn unregister_agent(
        &self,
        sensor_id: i32,
        key_b64: String,
        request: AgentRegisterDto,
    ) -> Result<(), ObserverError> {
        let container = self.container_ref.read().unwrap();
        let mut sensor = container
            .sensor(sensor_id, &key_b64)
            .ok_or(DBError::SensorNotFound(sensor_id))?;

        let agent = sensor
            .remove_agent(&request.agent_name, &request.domain)
            .ok_or(DBError::SensorNotFound(sensor_id))?;
        let conn = self.db_conn.lock().await;
        models::delete_sensor_agent(&conn, sensor.id(), agent)?;

        info!(
            APP_LOGGING,
            "Removed agent {}, {} from sensor {}", request.agent_name, request.domain, sensor_id
        );
        Ok(())
    }

    /*
     * Helpers
     */

    async fn register_sensor_dao(
        &self,
        sensor_dao: SensorDao,
        configs: Option<&Vec<AgentConfigDao>>,
    ) -> Result<i32, ObserverError> {
        let agent_factory = self.agent_factory.lock().await;
        let sensor_id = sensor_dao.id();
        let sensor = SensorHandle::from(sensor_dao, configs.unwrap_or(&vec![]), &agent_factory)?;

        self.register_sensor_mqtt(&sensor).await?;
        self.container_ref.write().unwrap().insert_sensor(sensor);
        Ok(sensor_id)
    }

    async fn register_sensor_mqtt(&self, sensor: &SensorHandle) -> Result<(), ObserverError> {
        let mut mqtt = self.mqtt_client.write().await;
        mqtt.subscribe_sensor(sensor)?;
        if let Err(e) = mqtt.send_cmd(sensor) {
            error!(APP_LOGGING, "Failed sending initial mqtt command: {}", e);
        }
        Ok(())
    }

    fn generate_sensor_key(&self) -> String {
        let mut buffer = Vec::with_capacity(6);
        for _ in 0..6 {
            buffer.push(rand::random::<u8>());
        }
        base64::encode(buffer).replace('/', &"-")
    }
}

pub struct SensorCache {
    sensors: HashMap<i32, std::sync::Mutex<SensorHandle>>,
}

impl SensorCache {
    pub fn new() -> Self {
        SensorCache {
            sensors: HashMap::new(),
        }
    }

    pub fn sensors(&self) -> Values<'_, i32, std::sync::Mutex<SensorHandle>> {
        self.sensors.values()
    }

    pub fn sensor_unchecked(
        &self,
        sensor_id: i32,
    ) -> Option<std::sync::MutexGuard<'_, SensorHandle>> {
        let sensor_mutex = self.sensors.get(&sensor_id)?;
        Some(sensor_mutex.lock().unwrap())
    }

    pub fn sensor(
        &self,
        sensor_id: i32,
        key_b64: &str,
    ) -> Option<std::sync::MutexGuard<'_, SensorHandle>> {
        let sensor_mutex = self.sensors.get(&sensor_id)?;
        let sensor = sensor_mutex.lock().unwrap();
        if sensor.key_b64() == key_b64 {
            Some(sensor)
        } else {
            None
        }
    }

    pub fn insert_sensor(&mut self, sensor: SensorHandle) {
        // TODO: Read-write lock
        self.sensors
            .insert(sensor.id(), std::sync::Mutex::new(sensor));
    }

    pub fn remove_sensor(
        &mut self,
        sensor_id: i32,
    ) -> Result<std::sync::Mutex<SensorHandle>, DBError> {
        if let Some(sensor_mtx) = self.sensors.remove(&sensor_id) {
            Ok(sensor_mtx)
        } else {
            Err(DBError::SensorNotFound(sensor_id))
        }
    }
}
