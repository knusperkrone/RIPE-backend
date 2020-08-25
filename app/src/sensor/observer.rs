use crate::error::{DBError, ObserverError};
use crate::plugin::agent::AgentFactory;

use super::handle::SensorHandle;
use crate::logging::APP_LOGGING;
use crate::models::{
    self,
    dao::{AgentConfigDao, SensorDao},
    establish_db_connection,
};
use crate::mqtt::MqttSensorClient;
use crate::rest::{
    AgentRegisterDto, AgentStatusDto, SensorMessageDto, SensorRegisterResponseDto, SensorStatusDto,
};
use diesel::pg::PgConnection;
use rumq_client::{MqttEventLoop, Publish};
use std::{collections::HashMap, sync::Arc};
use tokio::{
    stream::StreamExt,
    sync::mpsc::Receiver,
    sync::{Mutex, MutexGuard, RwLock},
};

use iftem_core::SensorDataMessage;

pub struct ConcurrentSensorObserver {
    container_ref: Arc<RwLock<SensorCache>>,
    agent_factory: Mutex<AgentFactory>,
    mqtt_client: RwLock<MqttSensorClient>,
    eventloop: Mutex<MqttEventLoop>,
    receiver: Mutex<Receiver<SensorMessageDto>>,
    db_conn: Mutex<PgConnection>,
}

// Init
impl ConcurrentSensorObserver {
    pub fn new() -> Arc<Self> {
        let db_conn = establish_db_connection();
        let (sender, receiver) = tokio::sync::mpsc::channel::<SensorMessageDto>(128);
        let agent_factory = AgentFactory::new(sender);
        let container = RwLock::new(SensorCache::new());
        let (client, eventloop) = MqttSensorClient::new();
        let observer = ConcurrentSensorObserver {
            agent_factory: Mutex::new(agent_factory),
            container_ref: Arc::new(container),
            mqtt_client: RwLock::new(client),
            eventloop: Mutex::new(eventloop),
            receiver: Mutex::new(receiver),
            db_conn: Mutex::new(db_conn),
        };

        Arc::new(observer)
    }

    pub async fn dispatch_ipc(self: Arc<ConcurrentSensorObserver>) -> () {
        let receiver_res = self.receiver.try_lock();
        if receiver_res.is_err() {
            error!(APP_LOGGING, "dispatch_ipc() already called!");
            return;
        }

        let mut receiver = receiver_res.unwrap();
        info!(APP_LOGGING, "Start capturing ipc-plugin events");
        while let Some(item) = receiver.next().await {
            let mut mqtt_client = self.mqtt_client.write().await;
            if let Err(e) = mqtt_client.send_cmd(&item).await {
                error!(APP_LOGGING, "Failed sending command {:?} with {}", item, e);
            }
        }
    }
}

// MQTT
impl ConcurrentSensorObserver {
    pub async fn dispatch_mqtt(self: Arc<ConcurrentSensorObserver>) -> () {
        let mut reconnects: i32 = 0;
        let eventloop_res = self.eventloop.try_lock();
        if eventloop_res.is_err() {
            error!(APP_LOGGING, "dispatch_mqtt() already called!");
            return;
        }

        let mut eventloop = eventloop_res.unwrap();
        loop {
            let stream_res = eventloop.connect().await;
            if stream_res.is_err() {
                info!(APP_LOGGING, "Failed connecting mqtt!");
                tokio::time::delay_for(std::time::Duration::from_secs(2)).await;
                continue;
            }

            info!(APP_LOGGING, "MQTT Connected");
            if reconnects == 0 {
                self.populate().await; // Subscribing to persisted sensors
            }

            let mut stream = stream_res.unwrap();
            while let Some(item) = stream.next().await {
                match item {
                    rumq_client::Notification::Publish(msg) => {
                        debug!(APP_LOGGING, "MQTT Reveived topic: {}", msg.topic_name);
                        self.on_mqtt_message(msg).await;
                    }
                    rumq_client::Notification::Suback(_) => (),
                    rumq_client::Notification::Pubrec(_) => (),
                    rumq_client::Notification::Pubcomp(_) => (),
                    _ => warn!(APP_LOGGING, "Received unexpected = {:?}", item),
                };
            }

            tokio::time::delay_for(std::time::Duration::from_secs(2)).await;
            error!(APP_LOGGING, "MQTT Disconnected - retry {}", reconnects);
            reconnects += 1;
        }
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

    async fn on_mqtt_message(&self, msg: Publish) {
        let mut client = self.mqtt_client.write().await;
        match client.on_sensor_message(&self.container_ref, msg).await {
            Ok(None) => (),
            Ok(Some(tuple)) => {
                let conn = self.db_conn.lock().await;
                if let Err(e) = models::insert_sensor_data(&conn, tuple.0, tuple.1) {
                    error!(APP_LOGGING, "Failed persisting data: {}", e);
                }
            }
            Err(e) => warn!(APP_LOGGING, "On message error: {}", e),
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

        let sensor_mtx = self.container_ref.write().await.remove_sensor(sensor_id)?;
        let sensor = sensor_mtx.lock().await;
        self.mqtt_client
            .write()
            .await
            .unsubscribe_sensor(&sensor)
            .await;

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
        let container = self.container_ref.read().await;
        let sensor = container
            .sensors(sensor_id, key_b64.as_str())
            .await
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
        let container = self.container_ref.read().await;
        let mut sensor = container
            .sensors(sensor_id, &key_b64)
            .await
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
        let container = self.container_ref.read().await;
        let mut sensor = container
            .sensors(sensor_id, &key_b64)
            .await
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
        let container = self.container_ref.read().await;
        let mut sensor = container
            .sensors(sensor_id, &key_b64)
            .await
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

        self.mqtt_client
            .write()
            .await
            .subscribe_sensor(&sensor)
            .await?;
        self.container_ref.write().await.insert_sensor(sensor);
        Ok(sensor_id)
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
    sensors: HashMap<i32, Mutex<SensorHandle>>,
}

impl SensorCache {
    pub fn new() -> Self {
        SensorCache {
            sensors: HashMap::new(),
        }
    }

    pub async fn sensors(
        &self,
        sensor_id: i32,
        key_b64: &str,
    ) -> Option<MutexGuard<'_, SensorHandle>> {
        let sensor_mutex = self.sensors.get(&sensor_id)?;
        let sensor = sensor_mutex.lock().await;
        if sensor.key_b64() == key_b64 {
            Some(sensor)
        } else {
            None
        }
    }

    pub fn insert_sensor(&mut self, sensor: SensorHandle) {
        self.sensors.insert(sensor.id(), Mutex::new(sensor));
    }

    pub fn remove_sensor(&mut self, sensor_id: i32) -> Result<Mutex<SensorHandle>, DBError> {
        if let Some(sensor_mtx) = self.sensors.remove(&sensor_id) {
            Ok(sensor_mtx)
        } else {
            Err(DBError::SensorNotFound(sensor_id))
        }
    }
}
