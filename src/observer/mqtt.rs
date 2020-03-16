use crate::error::MQTTError;
use crate::logging::APP_LOGGING;
use crate::observer::SensorContainer;
use crate::sensor::SensorData;
use dotenv::dotenv;
use rumq_client::{self, eventloop, MqttEventLoop, MqttOptions, Publish, QoS, Request};
use std::env;
use std::sync::{Arc, RwLock};
use tokio::sync::mpsc::{channel, Sender};

pub struct MqttSensorClient {
    requests_tx: Sender<Request>,
}

impl MqttSensorClient {
    pub const SENSOR_TOPIC: &'static str = "sensor";
    pub const DATA_TOPIC: &'static str = "data";
    pub const LOG_TOPIC: &'static str = "log";

    pub fn new() -> (Self, MqttEventLoop) {
        dotenv().ok();
        let mqtt_name = env::var("MQTT_NAME").expect("MQTT_NAME must be set");
        let mqtt_url = env::var("MQTT_URL").expect("MQTT_URL must be set");
        let mqtt_port = env::var("MQTT_PORT")
            .expect("MQTT_PORT must be set")
            .parse()
            .unwrap();

        let mqttoptions = MqttOptions::new(mqtt_name, mqtt_url, mqtt_port);
        let (requests_tx, requests_rx) = channel(std::i32::MAX as usize); // FIXME?
        let eventloop = eventloop(mqttoptions, requests_rx);
        let client = MqttSensorClient {
            requests_tx: requests_tx,
        };
        (client, eventloop)
    }

    pub async fn subscribe_sensor(&mut self, sensor_id: i32) -> Result<(), MQTTError> {
        let mut topics = self.build_topics(sensor_id);
        let mut sub = rumq_client::subscribe(topics.pop().unwrap(), QoS::AtLeastOnce);
        for topic in topics {
            sub.add(topic, QoS::AtLeastOnce);
        }

        if cfg!(test) {
            // WORKAROUND: Test instances are sometimes not attached to eventloop
            let result = self.requests_tx.send(Request::Subscribe(sub)).await;
            info!(APP_LOGGING, "[TEST] subscribe result: {:?}", result);
        } else {
            self.requests_tx.send(Request::Subscribe(sub)).await?;
            info!(
                APP_LOGGING,
                "Subscribed topics: {:?}",
                self.build_topics(sensor_id)
            );
        }
        Ok(())
    }

    pub async fn unsubscribe_sensor(&mut self, sensor_id: i32) {
        let mut _topics = self.build_topics(sensor_id);
        warn!(
            APP_LOGGING,
            "Cannot unsubscribe yet, due rumq-clients alpha state!"
        );
    }

    pub async fn on_message(
        container: Arc<RwLock<SensorContainer>>,
        msg: Publish,
    ) -> Result<(), String> {
        // parse message
        let path: Vec<&str> = msg.topic_name.splitn(3, '/').collect();
        if path.len() != 3 {
            return Err("Couldn't split message!".to_string());
        } else if path[0] != MqttSensorClient::SENSOR_TOPIC {
            return Err(format!("Invalid topic: {}", path[0]));
        }

        let sensor_id: i32;
        if let Ok(parsed_id) = path[1].parse() {
            sensor_id = parsed_id;
        } else {
            return Err(format!("Couldn't parse sensor_id: {}", path[0]));
        }

        let payload: String;
        if let Ok(string) = String::from_utf8(msg.payload) {
            payload = string;
        } else {
            return Err("Couldn't decode payload".to_string());
        }

        let endpoint = path[2];
        match endpoint {
            MqttSensorClient::DATA_TOPIC => {
                match serde_json::from_str::<SensorData>(&payload.as_str()) {
                    Ok(sensor_data) => {
                        if let Some(sensor) = container.read().unwrap().sensors(sensor_id) {
                            sensor.lock().unwrap().on_data(sensor_data);
                        } else {
                            return Err(format!("Did not found sensor: {}", sensor_id));
                        }
                    }
                    Err(e) => return Err(format!("Invalid JSON: {}", e)),
                }
            }
            MqttSensorClient::LOG_TOPIC => {
                // TODO: FILE LOGGER
            }
            _ => return Err(format!("Invalid endpoint: {}", endpoint)),
        }
        Ok(())
    }

    fn build_topics(&self, sensor_id: i32) -> Vec<String> {
        vec![
            format!(
                "{}/{}/{}",
                MqttSensorClient::SENSOR_TOPIC,
                MqttSensorClient::DATA_TOPIC,
                sensor_id
            ),
            format!(
                "{}/{}/{}",
                MqttSensorClient::SENSOR_TOPIC,
                MqttSensorClient::LOG_TOPIC,
                sensor_id
            ),
        ]
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::agent::mock::MockAgent;
    use crate::agent::Agent;
    use crate::models::SensorDao;
    use crate::sensor::SensorHandle;
    use futures_util::stream::StreamExt;

    #[actix_rt::test]
    async fn test_mqtt_connection() {
        let (_client, mut eventloop) = MqttSensorClient::new();
        let mut stream = eventloop.stream();
        if let Some(stream) = stream.next().await {
            if let rumq_client::Notification::StreamEnd(_) = stream {
                panic!("No mqtt connection!")
            }
        } else {
            panic!("No mqtt connection!")
        }
    }

    #[actix_rt::test]
    async fn test_invalid_mqtt_path() {
        // prepare
        let container = SensorContainer::new();
        let mocked_container = Arc::new(RwLock::new(container));

        // execute
        let mocked_message = Publish {
            dup: false,
            retain: false,
            qos: QoS::ExactlyOnce,
            pkid: None,
            topic_name: "sensor/data/0".to_string(),
            payload: vec![],
        };

        // validate
        let result = MqttSensorClient::on_message(mocked_container, mocked_message).await;
        assert_ne!(result.is_ok(), true);
    }

    #[actix_rt::test]
    async fn test_valid_mqtt_path() {
        // prepare
        let sensor_id = 0;
        let mut container = SensorContainer::new();
        let mock_sensor = SensorHandle {
            dao: SensorDao {
                id: sensor_id,
                name: "mock".to_string(),
            },
            agents: vec![MockAgent::new()],
        };
        container.insert_sensor(mock_sensor);
        let mocked_container = Arc::new(RwLock::new(container));

        // execute
        let mocked_message = Publish {
            dup: false,
            retain: false,
            qos: QoS::ExactlyOnce,
            pkid: None,
            topic_name: format!("sensor/{}/data", sensor_id),
            payload: serde_json::to_vec(&SensorData::default()).unwrap(),
        };

        let result = MqttSensorClient::on_message(mocked_container.clone(), mocked_message).await;

        // validate
        assert_eq!(result.is_ok(), true);
        let container = mocked_container.read().unwrap();
        let sensor = container.sensors(sensor_id).unwrap().lock().unwrap();
        if let Agent::MockAgent(sensor) = &sensor.agents[0] {
            assert_eq!(sensor.last_action.is_some(), true)
        } else {
            panic!("Invalid agent!");
        }
    }
}
