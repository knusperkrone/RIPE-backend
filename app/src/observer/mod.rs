use crate::agent::{plugin::AgentFactory, Agent, AgentRegisterConfig};
use crate::error::ObserverError;
use crate::logging::APP_LOGGING;
use crate::models::{self, establish_db_connection, AgentConfigDao, NewAgentConfig, SensorDao};
use crate::sensor::SensorHandle;
use diesel::pg::PgConnection;
use mqtt::MqttSensorClient;
use rumq_client::Publish;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, RwLock};
use tokio::stream::StreamExt;

mod mqtt;

pub struct ConcurrentSensorObserver {
    container_ref: Arc<RwLock<SensorContainer>>,
    agent_factory: Mutex<AgentFactory>,
    mqtt_client: RwLock<MqttSensorClient>,
    db_conn: Mutex<PgConnection>,
}

impl ConcurrentSensorObserver {
    pub fn new() -> (Arc<Self>, impl futures::Future) {
        let container = RwLock::new(SensorContainer::new());
        let (client, eventloop) = MqttSensorClient::new();
        let db_conn = establish_db_connection();
        let observer = ConcurrentSensorObserver {
            agent_factory: Mutex::new(AgentFactory::new()),
            container_ref: Arc::new(container),
            mqtt_client: RwLock::new(client),
            db_conn: Mutex::new(db_conn),
        };

        let observer_arc = Arc::new(observer);
        let dispatcher_future =
            ConcurrentSensorObserver::dispatch_mqtt(observer_arc.clone(), eventloop);

        (observer_arc, dispatcher_future)
    }

    pub async fn on_message(&self, msg: Publish) {
        let mut client = self.mqtt_client.write().unwrap();
        match client.on_message(self.container_ref.clone(), msg).await {
            Ok(Some(_data)) => {
                // TODO: Persist data
            }
            Ok(None) => (),
            Err(e) => warn!(APP_LOGGING, "On message{}", e),
        }
    }

    async fn dispatch_mqtt(
        self: Arc<ConcurrentSensorObserver>,
        mut eventloop: rumq_client::MqttEventLoop,
    ) -> () {
        let mut reconnects: i32 = 0;
        let mut stream = eventloop.connect().await.unwrap();

        info!(APP_LOGGING, "MQTT Connected");
        self.populate().await; // Subscribing to persisted sensors

        loop {
            while let Some(item) = stream.next().await {
                match item {
                    rumq_client::Notification::Publish(msg) => {
                        info!(APP_LOGGING, "MQTT Reveived topic: {}", msg.topic_name);
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

    pub async fn register_new_sensor(
        &self,
        name: &Option<String>,
        configs: &Vec<AgentRegisterConfig>,
    ) -> Result<i32, ObserverError> {
        // Transform configs
        let agent_factory = self.agent_factory.lock().unwrap();

        let agents_opts: Result<Vec<Agent>, _> = configs
            .into_iter()
            .map(|config| agent_factory.new_agent(config))
            .collect();
        if agents_opts.is_err() {
            return Err(ObserverError::from(agents_opts.err().unwrap()));
        }
        let agents: Vec<Agent> = agents_opts.ok().unwrap();
        let agent_config: Vec<NewAgentConfig> = agents.iter().map(|x| x.deserialize()).collect();

        let conn = self.db_conn.lock().unwrap();
        let (sensor_dao, agent_daos) = models::create_new_sensor(&conn, name, agent_config)?;
        let dao_id = sensor_dao.id;
        match self.register_sensor_dao(sensor_dao, &agent_daos).await {
            Ok(id) => Ok(id),
            Err(err) => {
                models::delete_sensor(&conn, dao_id)?;
                Err(ObserverError::from(err))
            }
        }
    }

    pub async fn remove_sensor(&self, sensor_id: i32) -> Result<i32, ObserverError> {
        let conn = self.db_conn.lock().unwrap();
        models::delete_sensor(&conn, sensor_id)?;

        self.container_ref.write().unwrap().remove_sensor(sensor_id);
        self.mqtt_client
            .write()
            .unwrap()
            .unsubscribe_sensor(sensor_id)
            .await;
        Ok(sensor_id)
    }

    async fn populate(&self) {
        let db_conn = self.db_conn.lock().unwrap();
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

    async fn register_sensor_dao(
        &self,
        sensor_dao: SensorDao,
        configs: &Vec<AgentConfigDao>,
    ) -> Result<i32, ObserverError> {
        let agent_factory = self.agent_factory.lock().unwrap();
        let sensor_id = sensor_dao.id;
        let sensor: SensorHandle = SensorHandle::from(sensor_dao, configs, &agent_factory)?;

        self.container_ref.write().unwrap().insert_sensor(sensor);
        self.mqtt_client
            .write()
            .unwrap()
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

    pub fn sensors(&self, sensor_id: i32) -> Option<MutexGuard<SensorHandle>> {
        let sensor_mutex = self.sensors.get(&sensor_id)?;
        Some(sensor_mutex.lock().unwrap())
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
