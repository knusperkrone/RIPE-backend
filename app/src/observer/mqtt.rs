use crate::error::MQTTError;
use crate::logging::APP_LOGGING;
use crate::observer::SensorContainer;
use crate::sensor::{SensorDataDto, SensorHandleData, SensorMessage};
use dotenv::dotenv;
use plugins_core::SensorData;
use rumq_client::{self, eventloop, MqttEventLoop, MqttOptions, Publish, QoS, Request, Subscribe};
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
        let mut sub = Subscribe::new(topics.pop().unwrap(), QoS::AtLeastOnce);
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
        &mut self,
        container_lock: Arc<RwLock<SensorContainer>>,
        msg: Publish,
    ) -> Result<Option<SensorHandleData>, MQTTError> {
        // parse message
        let path: Vec<&str> = msg.topic_name.splitn(3, '/').collect();
        if path.len() != 3 {
            return Err(MQTTError::PathError(format!(
                "Couldn't split topic: {}",
                msg.topic_name
            )));
        } else if path[0] != MqttSensorClient::SENSOR_TOPIC {
            return Err(MQTTError::PathError(format!("Invalid topic: {}", path[0])));
        }

        let sensor_id: i32;
        if let Ok(parsed_id) = path[1].parse() {
            sensor_id = parsed_id;
        } else {
            return Err(MQTTError::PathError(format!(
                "Couldn't parse sensor_id: {}",
                path[0]
            )));
        }

        let payload: String;
        if let Ok(string) = String::from_utf8(msg.payload) {
            payload = string;
        } else {
            return Err(MQTTError::PayloadError(
                "Couldn't decode payload".to_string(),
            ));
        }

        let endpoint = path[2];
        match endpoint {
            MqttSensorClient::DATA_TOPIC => {
                let sensor_dto = serde_json::from_str::<SensorDataDto>(&payload.as_str())?;
                let sensor_data: SensorData = sensor_dto.into();
                let container = container_lock.read().unwrap();
                let mut sensor = container.sensors(sensor_id).ok_or(MQTTError::NoSensor())?;
                let messages = sensor.on_data(&sensor_data);
                self.send(sensor_id, messages).await?;
                Ok(Some(SensorHandleData::new(sensor_id, sensor_data)))
            }
            MqttSensorClient::LOG_TOPIC => {
                // TODO: LOG!
                Ok(None)
            }
            _ => Err(MQTTError::PathError(format!(
                "Invalid endpoint: {}",
                endpoint
            ))),
        }
    }

    async fn send(
        &mut self,
        sensor_id: i32,
        sensor_commands: Vec<SensorMessage>,
    ) -> Result<(), MQTTError> {
        let cmd_topic = format!(
            "{}/{}/{}",
            MqttSensorClient::SENSOR_TOPIC,
            MqttSensorClient::DATA_TOPIC,
            sensor_id
        );

        for command in sensor_commands {
            let payload = serde_json::to_string(&command).unwrap();
            info!(APP_LOGGING, "Sending message: {}", payload);
            let tmp = Publish::new(&cmd_topic, QoS::ExactlyOnce, payload);
            let publish = Request::Publish(tmp);
            self.requests_tx.send(publish).await?;
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
    use crate::models::SensorDao;
    use crate::sensor::SensorHandle;

    #[actix_rt::test]
    async fn test_mqtt_connection() {
        let (_client, mut eventloop) = MqttSensorClient::new();
        let _ = eventloop.connect().await.unwrap();
    }

    #[actix_rt::test]
    async fn test_invalid_mqtt_path() {
        // prepare
        let container = SensorContainer::new();
        let mocked_container = Arc::new(RwLock::new(container));
        let (mut client, _) = MqttSensorClient::new();

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
        let result = client.on_message(mocked_container, mocked_message).await;
        assert_ne!(result.is_ok(), true);
    }

    #[actix_rt::test]
    async fn test_valid_mqtt_path() {
        // prepare
        let sensor_id = 0;
        let mut container = SensorContainer::new();
        let (mut client, _) = MqttSensorClient::new();
        let mock_sensor = SensorHandle {
            dao: SensorDao {
                id: sensor_id,
                name: "mock".to_string(),
            },
            agents: vec![],
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
            payload: serde_json::to_vec(&SensorDataDto::default()).unwrap(),
        };

        let result = client
            .on_message(mocked_container.clone(), mocked_message)
            .await;

        // validate

        /*
        // TODO:
        assert_eq!(result.is_ok(), true);
        let container = mocked_container.read().unwrap();
        let sensor = container.sensors(sensor_id).unwrap();
        if let Agent::MockAgent(sensor) = &sensor.agents[0] {
            assert_eq!(sensor.last_action.is_some(), true)
        } else {
            panic!("Invalid agent!");
        }
        */
    }
}
