mod mqtt;

use crate::agent::AgentRegisterConfig;
use crate::logging::APP_LOGGING;
use crate::models::{establish_connection, SensorDao};
use crate::sensor::SensorHandle;
use diesel::pg::PgConnection;
use futures_util::stream::StreamExt;
use mqtt::MqttSensorClient;
use rumq_client::Publish;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

pub struct ConcurrentSensorObserver {
    container: Arc<RwLock<SensorContainer>>,
    mqtt_client: RwLock<MqttSensorClient>,
    db_conn: Mutex<PgConnection>,
}

impl ConcurrentSensorObserver {
    pub fn new() -> (Arc<Self>, impl futures::Future) {
        let container = RwLock::new(SensorContainer::new());
        let (client, eventloop) = MqttSensorClient::new();

        let db_conn = establish_connection();
        let persisted_sensors = crate::models::get_sensors(&db_conn);

        let mut observer = ConcurrentSensorObserver {
            container: Arc::new(container),
            mqtt_client: RwLock::new(client),
            db_conn: Mutex::new(db_conn),
        };

        for sensor in persisted_sensors {
            //observer.register_sensor_dao(sensor, actions: &Vec<AgentRegisterConfig>)
        }

        let observer_arc = Arc::new(observer);
        let dispatcher_future =
            ConcurrentSensorObserver::dispatch_mqtt(observer_arc.clone(), eventloop);

        (observer_arc, dispatcher_future)
    }

    async fn dispatch_mqtt(
        self_: Arc<ConcurrentSensorObserver>,
        mut eventloop: rumq_client::MqttEventLoop,
    ) {
        let mut stream = eventloop.stream();
        while let Some(item) = stream.next().await {
            match item {
                rumq_client::Notification::Connected => info!(APP_LOGGING, "MQTT Connected"),
                rumq_client::Notification::Publish(msg) => self_.on_message(msg).await,
                rumq_client::Notification::Suback(_) => (),
                _ => warn!(APP_LOGGING, "Received unexpected = {:?}", item),
            };
        }
    }

    pub async fn on_message(&self, msg: Publish) {
        if let Err(err) = MqttSensorClient::on_message(self.container.clone(), msg).await {
            error!(APP_LOGGING, "MQTT msg error: {}", err);
        }
    }

    async fn register_sensor_dao(
        &self,
        sensor_dao: SensorDao,
        actions: &Vec<AgentRegisterConfig>,
    ) -> Option<i32> {
        let sensor_id = sensor_dao.id;
        let sensor: SensorHandle;
        if let Some(new_sensor) = SensorHandle::from(sensor_dao, actions) {
            info!(APP_LOGGING, "Created sensor: {}", sensor_id);
            sensor = new_sensor;
        } else {
            error!(APP_LOGGING, "Invalid config for sensor: {}", sensor_id);
            return None;
        }

        self.container.write().unwrap().insert_sensor(sensor);
        self.mqtt_client
            .write()
            .unwrap()
            .subscribe_sensor(sensor_id)
            .await;

        info!(APP_LOGGING, "Inserted sensor: {}", sensor_id);
        Some(sensor_id)
    }

    pub async fn register_new_sensor(
        &self,
        name: &Option<String>,
        actions: &Vec<AgentRegisterConfig>,
    ) -> Option<i32> {
        // Create new database entry and insert
        let conn = self.db_conn.lock().unwrap();
        if let Some(sensor_dao) = crate::models::create_new_sensor(&conn, name) {
            info!(APP_LOGGING, "Created sensorDao: {}", sensor_dao.id);
            let dao_id = sensor_dao.id;
            if let Some(id) = self.register_sensor_dao(sensor_dao, actions).await {
                return Some(id);
            } else {
                if let Err(_) = crate::models::delete_sensor(&conn, dao_id) {
                    error!(APP_LOGGING, "Failed deleting invalid sensor: {}", dao_id);
                }
            }
        }
        return None;
    }

    pub async fn remove_sensor(&self, sensor_id: i32) -> bool {
        if self.container.write().unwrap().remove_sensor(sensor_id) {
            self.mqtt_client
                .write()
                .unwrap()
                .unsubscribe_sensor(sensor_id)
                .await;
            info!(APP_LOGGING, "Removed sensor: {}", sensor_id);
            true
        } else {
            warn!(APP_LOGGING, "Failed Removing sensor: {}", sensor_id);
            false
        }
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

    pub fn sensors(&self, sensor_id: i32) -> Option<&Mutex<SensorHandle>> {
        self.sensors.get(&sensor_id)
    }

    fn insert_sensor(&mut self, sensor: SensorHandle) {
        self.sensors.insert(sensor.id(), Mutex::new(sensor));
    }

    fn remove_sensor(&mut self, sensor_id: i32) -> bool {
        if let Some(_) = self.sensors.remove(&sensor_id) {
            true
        } else {
            false
        }
    }
}
