use crate::error::MQTTError;
use crate::logging::APP_LOGGING;
use crate::sensor::{handle::SensorHandle, observer::SensorCache};
use dotenv::dotenv;
use iftem_core::SensorDataMessage;
use mqtt_async_client::client::{
    Client, Publish, QoS, ReadResult, Subscribe, SubscribeTopic, Unsubscribe, UnsubscribeTopic,
};
use std::env;
use std::sync::Arc;
use tokio::sync::RwLock;

#[cfg(test)]
mod test;

pub struct MqttSensorClient {
    tx_client: Client,
}

impl MqttSensorClient {
    pub const SENSOR_TOPIC: &'static str = "sensor";
    pub const CMD_TOPIC: &'static str = "cmd";
    pub const DATA_TOPIC: &'static str = "data";
    pub const LOG_TOPIC: &'static str = "log";

    pub fn new() -> (Self, Client) {
        dotenv().ok();
        let mqtt_name = env::var("MQTT_NAME").expect("MQTT_NAME must be set");
        let mqtt_url = env::var("MQTT_URL").expect("MQTT_URL must be set");
        let mqtt_port = env::var("MQTT_PORT")
            .expect("MQTT_PORT must be set")
            .parse()
            .unwrap();

        let rx_client = Client::builder()
            .set_username(Some(mqtt_name.clone() + "_wx"))
            .set_host(mqtt_url.clone())
            .set_port(mqtt_port)
            .build()
            .unwrap();
        let tx_client = Client::builder()
            .set_username(Some(mqtt_name + "_tx"))
            .set_host(mqtt_url)
            .set_port(mqtt_port)
            .build()
            .unwrap();

        (
            MqttSensorClient {
                tx_client: tx_client,
            },
            rx_client,
        )
    }

    pub async fn subscribe_sensor(&mut self, sensor: &SensorHandle) -> Result<(), MQTTError> {
        let topics = self
            .build_topics(sensor)
            .drain(..)
            .map(|t| SubscribeTopic {
                topic_path: t,
                qos: QoS::ExactlyOnce,
            })
            .collect::<Vec<SubscribeTopic>>();
        let sub = Subscribe::new(topics);

        let _ = self.tx_client.connect().await;
        self.tx_client.subscribe(sub).await?;

        info!(
            APP_LOGGING,
            "Subscribed topics: {:?}",
            self.build_topics(sensor)
        );
        Ok(())
    }

    pub async fn unsubscribe_sensor(&mut self, sensor: &SensorHandle) -> Result<(), MQTTError> {
        let topics = self
            .build_topics(sensor)
            .drain(..)
            .map(|t| UnsubscribeTopic::new(t))
            .collect::<Vec<UnsubscribeTopic>>();
        let unsub = Unsubscribe::new(topics);

        let _ = self.tx_client.connect().await;
        self.tx_client.unsubscribe(unsub).await?;

        info!(
            APP_LOGGING,
            "Unsubscribed topics: {:?}",
            self.build_topics(sensor)
        );
        Ok(())
    }

    /// Parses and dispatches a mqtt message
    ///
    /// Returns nothing on submitted command, or the submitted sensor data
    pub async fn on_sensor_message(
        &mut self,
        container_lock: &Arc<RwLock<SensorCache>>,
        msg: ReadResult,
    ) -> Result<Option<(i32, SensorDataMessage)>, MQTTError> {
        // parse message
        let path: Vec<&str> = msg.topic().splitn(4, '/').collect();
        if path.len() != 4 {
            return Err(MQTTError::PathError(format!(
                "Couldn't split topic: {}",
                msg.topic()
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

        let payload: &str;
        if let Ok(string) = std::str::from_utf8(msg.payload()) {
            payload = string;
        } else {
            return Err(MQTTError::PayloadError(
                "Couldn't decode payload".to_string(),
            ));
        }

        match endpoint {
            MqttSensorClient::DATA_TOPIC => {
                let sensor_dto = serde_json::from_str::<SensorDataMessage>(payload)?;
                let container = container_lock.read().await;
                let mut sensor = container
                    .sensor(sensor_id, key)
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

    pub async fn send_cmd(&mut self, sensor: &SensorHandle) -> Result<(), MQTTError> {
        let cmd_topic = self.build_topic(sensor, MqttSensorClient::CMD_TOPIC);

        let mut cmds = sensor.format_cmds();
        let payload = cmds.drain(..).map(|i| i.to_ne_bytes()[0]).collect();

        let mut publ = Publish::new(cmd_topic, payload);
        publ.set_retain(true);

        let _ = self.tx_client.connect().await;
        self.tx_client.publish(&publ).await?;

        info!(
            APP_LOGGING,
            "Send to topic {}, {:?}",
            publ.topic(),
            publ.payload()
        );
        Ok(())
    }

    fn build_topics(&self, sensor: &SensorHandle) -> Vec<String> {
        vec![
            self.build_topic(sensor, MqttSensorClient::DATA_TOPIC),
            self.build_topic(sensor, MqttSensorClient::LOG_TOPIC),
        ]
    }

    fn build_topic(&self, sensor: &SensorHandle, topic: &str) -> String {
        format!(
            "{}/{}/{}/{}",
            MqttSensorClient::SENSOR_TOPIC,
            topic,
            sensor.id(),
            sensor.key_b64(),
        )
    }
}
