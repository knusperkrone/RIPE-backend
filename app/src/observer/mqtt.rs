use crate::error::MQTTError;
use crate::logging::APP_LOGGING;
use crate::models::dto::SensorMessageDto;
use crate::observer::SensorContainer;
use dotenv::dotenv;
use iftem_core::SensorDataDto;
use rumq_client::{self, eventloop, MqttEventLoop, MqttOptions, Publish, QoS, Request, Subscribe};
use std::env;
use std::sync::Arc;
use tokio::sync::{
    mpsc::{channel, Sender},
    RwLock,
};

pub struct MqttSensorClient {
    requests_tx: Sender<Request>,
}

impl MqttSensorClient {
    pub const SENSOR_TOPIC: &'static str = "sensor";
    pub const CMD_TOPIC: &'static str = "cmd";
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

        self.requests_tx.send(Request::Subscribe(sub)).await?;
        info!(
            APP_LOGGING,
            "Subscribed topics: {:?}",
            self.build_topics(sensor_id)
        );
        Ok(())
    }

    pub async fn unsubscribe_sensor(&mut self, sensor_id: i32) {
        let mut _topics = self.build_topics(sensor_id);
        warn!(
            APP_LOGGING,
            "Cannot unsubscribe yet, due rumq-clients alpha state!"
        );
    }

    pub async fn on_sensor_message(
        &mut self,
        container_lock: Arc<RwLock<SensorContainer>>,
        msg: Publish,
    ) -> Result<Option<()>, MQTTError> {
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
        let endpoint = path[1];
        if let Ok(parsed_id) = path[2].parse() {
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

        match endpoint {
            MqttSensorClient::DATA_TOPIC => {
                let sensor_dto = serde_json::from_str::<SensorDataDto>(&payload)?;
                let container = container_lock.read().await;
                let mut sensor = container
                    .sensors(sensor_id)
                    .await
                    .ok_or(MQTTError::NoSensor())?;
                sensor.on_data(&sensor_dto);
                Ok(None) // TODO: Return data
            }
            MqttSensorClient::LOG_TOPIC => {
                info!(
                    APP_LOGGING,
                    "Log sensor[{}]: {}",
                    sensor_id,
                    payload.to_string()
                );
                Ok(None)
            }
            _ => Err(MQTTError::PathError(format!(
                "Invalid endpoint: {}",
                endpoint
            ))),
        }
    }

    pub async fn send_cmd(&mut self, sensor_command: &SensorMessageDto) -> Result<(), MQTTError> {
        let cmd_topic = format!(
            "{}/{}/{}",
            MqttSensorClient::SENSOR_TOPIC,
            MqttSensorClient::CMD_TOPIC,
            sensor_command.sensor_id,
        );

        let payload = serde_json::to_string(&sensor_command).unwrap();
        let tmp = Publish::new(&cmd_topic, QoS::ExactlyOnce, payload);
        let publish = Request::Publish(tmp);
        self.requests_tx.send(publish).await?;
        info!(
            APP_LOGGING,
            "Send command {} - {:?}", cmd_topic, sensor_command
        );
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
    use super::super::sensor::SensorHandle;
    use super::*;
    use crate::agent::{mock::MockAgent, Agent};
    use crate::models::dao::SensorDao;
    use iftem_core::AgentMessage;

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
        let result = client
            .on_sensor_message(mocked_container, mocked_message)
            .await;
        assert_ne!(result.is_ok(), true);
    }

    #[actix_rt::test]
    async fn test_valid_mqtt_path() {
        // prepare
        let sensor_id = 0;
        let (sender, _) = tokio::sync::mpsc::channel::<SensorMessageDto>(2);
        let (_, receiver) = tokio::sync::mpsc::channel::<AgentMessage>(2);
        let mut container = SensorContainer::new();
        let (mut client, _) = MqttSensorClient::new();
        let mock_sensor = SensorHandle {
            dao: SensorDao::new(sensor_id, "mock".to_owned()),
            agents: vec![Agent::new(
                sender,
                receiver,
                sensor_id,
                "MockDomain".to_owned(),
                Box::new(MockAgent::new()),
            )],
        };
        container.insert_sensor(mock_sensor);
        let mocked_container = Arc::new(RwLock::new(container));

        // execute
        let mocked_message = Publish {
            dup: false,
            retain: false,
            qos: QoS::ExactlyOnce,
            pkid: None,
            topic_name: format!("sensor/data/{}", sensor_id),
            payload: serde_json::to_vec(&SensorDataDto::default()).unwrap(),
        };

        let result = client
            .on_sensor_message(mocked_container.clone(), mocked_message)
            .await;

        // validate
        assert_eq!(result.is_ok(), true);
        let container = mocked_container.read().await;
        let _sensor = container.sensors(sensor_id).await;
        // TODO: check agent
    }
}
