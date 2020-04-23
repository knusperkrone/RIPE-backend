use crate::agent::{plugin::AgentFactory, Agent};
use crate::error::ObserverError;
use crate::logging::APP_LOGGING;
use crate::models::{
    self,
    dao::{AgentConfigDao, SensorDao},
    dto::{AgentPayload, AgentRegisterDto, SensorMessageDto},
    establish_db_connection,
};
use diesel::pg::PgConnection;
use mqtt::MqttSensorClient;
use rumq_client::{MqttEventLoop, Publish};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::{
    stream::StreamExt,
    sync::mpsc::Receiver,
    sync::{Mutex, MutexGuard, RwLock},
};

mod mqtt;
mod sensor;

use sensor::SensorHandle;

pub struct ConcurrentSensorObserver {
    container_ref: Arc<RwLock<SensorContainer>>,
    agent_factory: Mutex<AgentFactory>,
    mqtt_client: RwLock<MqttSensorClient>,
    eventloop: Mutex<MqttEventLoop>,
    receiver: Mutex<Receiver<SensorMessageDto>>,
    db_conn: Mutex<PgConnection>,
}

impl ConcurrentSensorObserver {
    pub fn new() -> Arc<Self> {
        let db_conn = establish_db_connection();
        let (sender, receiver) = tokio::sync::mpsc::channel::<SensorMessageDto>(128);
        let agent_factory = AgentFactory::new(sender);
        let container = RwLock::new(SensorContainer::new());
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
                        self.on_message(msg).await;
                    }
                    rumq_client::Notification::Suback(_) => (),
                    _ => warn!(APP_LOGGING, "Received unexpected = {:?}", item),
                };
            }

            tokio::time::delay_for(std::time::Duration::from_secs(2)).await;
            error!(APP_LOGGING, "MQTT Disconnected - retry {}", reconnects);
            reconnects += 1;
        }
    }

    pub async fn dispatch_ipc(self: Arc<ConcurrentSensorObserver>) -> () {
        let receiver_res = self.receiver.try_lock();
        if receiver_res.is_err() {
            error!(APP_LOGGING, "dispatch_ipc() already called!");
            return;
        }

        let mut receiver = receiver_res.unwrap();
        while let Some(item) = receiver.next().await {
            let mut mqtt_client = self.mqtt_client.write().await;
            if let AgentPayload::State(_agent_state) = item.payload {
                // TODO: REFRESH STATE
                info!(APP_LOGGING, "TODO refresh agent state");
            } else {
                if let Err(e) = mqtt_client.send_cmd(&item).await {
                    error!(APP_LOGGING, "Failed sending command {:?} with {}", item, e);
                }
            }
        }
    }

    pub async fn agents(&self) -> Vec<String> {
        let container_factory = self.agent_factory.lock().await;
        container_factory.agents()
    }

    pub async fn on_message(&self, msg: Publish) {
        // TODO: Check for api call or sensor data
        let mut client = self.mqtt_client.write().await;
        match client
            .on_sensor_message(self.container_ref.clone(), msg)
            .await
        {
            Ok(Some(_data)) => {
                // TODO: Persist data
            }
            Ok(None) => (),
            Err(e) => warn!(APP_LOGGING, "On message error: {}", e),
        }
    }

    pub async fn register_new_sensor(
        &self,
        name: &Option<String>,
        configs: &Vec<AgentRegisterDto>,
    ) -> Result<i32, ObserverError> {
        // Create sensor
        let conn = self.db_conn.lock().await;
        let sensor_dao = models::create_new_sensor(&conn, name)?;
        let dao_id = sensor_dao.id();

        // Create agents and persist
        let agents: Vec<Agent> = self.build_agents(dao_id, configs).await?;
        let agent_config: Vec<AgentConfigDao> = agents.iter().map(|x| x.deserialize()).collect();
        match models::create_sensor_agents(&conn, agent_config) {
            Ok(agent_config) => {
                match self.register_sensor_dao(sensor_dao, &agent_config).await {
                    Ok(id) => Ok(id),
                    Err(err) => {
                        models::delete_sensor(&conn, dao_id)?; // Fallback delete
                        Err(ObserverError::from(err))
                    }
                }
            }
            Err(err) => {
                models::delete_sensor(&conn, dao_id)?; // Fallback delete
                Err(ObserverError::from(err))
            }
        }
    }

    pub async fn remove_sensor(&self, sensor_id: i32) -> Result<i32, ObserverError> {
        let conn = self.db_conn.lock().await;
        models::delete_sensor(&conn, sensor_id)?;

        self.container_ref.write().await.remove_sensor(sensor_id);
        self.mqtt_client
            .write()
            .await
            .unsubscribe_sensor(sensor_id)
            .await;
        Ok(sensor_id)
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
            let restore_result = self.register_sensor_dao(sensor_dao, &agent_configs).await;
            match restore_result {
                Ok(id) => info!(APP_LOGGING, "Restored sensor {}", id),
                Err(msg) => error!(APP_LOGGING, "{}", msg),
            }
        }
    }

    async fn build_agents(
        &self,
        sensor_id: i32,
        configs: &Vec<AgentRegisterDto>,
    ) -> Result<Vec<Agent>, ObserverError> {
        let agent_factory = self.agent_factory.lock().await;
        let agents_result: Result<Vec<Agent>, _> = configs
            .into_iter()
            .map(|config| agent_factory.new_agent(sensor_id, &config.agent_name, &config.domain))
            .collect();
        Ok(agents_result?)
    }

    async fn register_sensor_dao(
        &self,
        sensor_dao: SensorDao,
        configs: &Vec<AgentConfigDao>,
    ) -> Result<i32, ObserverError> {
        let agent_factory = self.agent_factory.lock().await;
        let sensor_id = sensor_dao.id();
        let sensor: SensorHandle = SensorHandle::from(sensor_dao, configs, &agent_factory)?;

        self.container_ref.write().await.insert_sensor(sensor);
        self.mqtt_client
            .write()
            .await
            .subscribe_sensor(sensor_id)
            .await?;
        Ok(sensor_id)
    }
}

pub struct SensorContainer {
    sensors: HashMap<i32, Mutex<SensorHandle>>,
}

impl SensorContainer {
    fn new() -> Self {
        SensorContainer {
            sensors: HashMap::new(),
        }
    }

    pub async fn sensors(&self, sensor_id: i32) -> Option<MutexGuard<'_, SensorHandle>> {
        let sensor_mutex = self.sensors.get(&sensor_id)?;
        Some(sensor_mutex.lock().await)
    }

    fn insert_sensor(&mut self, sensor: SensorHandle) {
        //info!(APP_LOGGING, "Inserted new sensor: {}", sensor.id);
        self.sensors.insert(sensor.id(), Mutex::new(sensor));
    }

    fn remove_sensor(&mut self, sensor_id: i32) -> () {
        if let None = self.sensors.remove(&sensor_id) {
            warn!(APP_LOGGING, "Coulnd't remove sensor: {}", sensor_id);
        } else {
            info!(APP_LOGGING, "Removed new sensor: {}", sensor_id);
        }
    }
}
