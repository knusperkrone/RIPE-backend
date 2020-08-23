use super::handle::SensorHandle;
use super::observer::SensorCache;
use crate::error::MQTTError;
use crate::logging::APP_LOGGING;
use crate::models::dto::SensorMessageDto;
use dotenv::dotenv;
use iftem_core::SensorDataMessage;
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

    pub async fn subscribe_sensor(&mut self, sensor: &SensorHandle) -> Result<(), MQTTError> {
        let mut topics = self.build_topics(sensor);
        let mut sub = Subscribe::new(topics.pop().unwrap(), QoS::AtLeastOnce);
        for topic in topics {
            sub.add(topic, QoS::AtLeastOnce);
        }

        self.requests_tx.send(Request::Subscribe(sub)).await?;
        info!(
            APP_LOGGING,
            "Subscribed topics: {:?}",
            self.build_topics(sensor)
        );
        Ok(())
    }

    pub async fn unsubscribe_sensor(&mut self, sensor: &SensorHandle) {
        let mut _topics = self.build_topics(sensor);
        warn!(
            APP_LOGGING,
            "Cannot unsubscribe yet, due rumq-clients alpha state!"
        );
    }

    pub async fn on_sensor_message(
        &mut self,
        container_lock: &Arc<RwLock<SensorCache>>,
        msg: Publish,
    ) -> Result<Option<(i32, SensorDataMessage)>, MQTTError> {
        // parse message
        let path: Vec<&str> = msg.topic_name.splitn(4, '/').collect();
        if path.len() != 4 {
            return Err(MQTTError::PathError(format!(
                "Couldn't split topic: {}",
                msg.topic_name
            )));
        } else if path[0] != MqttSensorClient::SENSOR_TOPIC {
            return Err(MQTTError::PathError(format!("Invalid topic: {}", path[0])));
        }

        let sensor_id: i32;
        let endpoint = path[1];
        let key = path[3];
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
                let sensor_dto = serde_json::from_str::<SensorDataMessage>(&payload)?;
                let container = container_lock.read().await;
                let mut sensor = container
                    .sensors(sensor_id, key)
                    .await
                    .ok_or(MQTTError::NoSensor())?;
                sensor.on_data(&sensor_dto);
                Ok(Some((sensor_id, sensor_dto)))
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

    fn build_topics(&self, sensor: &SensorHandle) -> Vec<String> {
        vec![
            format!(
                "{}/{}/{}/{}",
                MqttSensorClient::SENSOR_TOPIC,
                MqttSensorClient::DATA_TOPIC,
                sensor.id(),
                sensor.key_b64(),
            ),
            format!(
                "{}/{}/{}/{}",
                MqttSensorClient::SENSOR_TOPIC,
                MqttSensorClient::LOG_TOPIC,
                sensor.id(),
                sensor.key_b64(),
            ),
        ]
    }
}
