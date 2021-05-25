use std::time::Duration;

use crate::config::CONFIG;
use crate::error::MQTTError;
use crate::logging::APP_LOGGING;
use crate::sensor::handle::SensorHandle;
use iftem_core::SensorDataMessage;
use paho_mqtt::{AsyncClient, ConnectOptionsBuilder, CreateOptionsBuilder, Message};
use tokio::sync::mpsc::UnboundedSender;

#[cfg(test)]
mod test;

const MQTTV5: u32 = 5;
const QOS: i32 = 1;

type MqttSender = UnboundedSender<(i32, SensorDataMessage)>;

pub struct MqttSensorClient {
    cli: AsyncClient,
}

impl MqttSensorClient {
    pub const TESTAMENT_TOPIC: &'static str = "ripe/master";
    pub const SENSOR_TOPIC: &'static str = "sensor";
    pub const CMD_TOPIC: &'static str = "cmd";
    pub const DATA_TOPIC: &'static str = "data";
    pub const LOG_TOPIC: &'static str = "log";

    pub fn new(mqtt_name: String, sender: MqttSender) -> Self {
        let cli = Self::create_client(mqtt_name, sender);

        MqttSensorClient { cli }
    }

    pub async fn connect(&self) {
        Self::do_connect(&self.cli).await;
    }

    pub fn broker(&self) -> Option<String> {
        if self.cli.is_connected() {
            Some(CONFIG.current_mqtt_broker())
        } else {
            None
        }
    }

    pub async fn subscribe_sensor(&self, sensor: &SensorHandle) -> Result<(), MQTTError> {
        if cfg!(test) && !self.cli.is_connected() {
            return Ok(());
        }

        let topics = Self::build_topics(sensor);
        self.cli.subscribe_many(&topics, &vec![QOS, QOS]).await?;

        debug!(APP_LOGGING, "Subscribed topics {:?}", topics);
        Ok(())
    }

    pub async fn unsubscribe_sensor(&self, sensor: &SensorHandle) -> Result<(), MQTTError> {
        if cfg!(test) && !self.cli.is_connected() {
            return Ok(());
        }

        let topics = Self::build_topics(sensor);
        self.cli.unsubscribe_many(&topics).await?;

        debug!(APP_LOGGING, "Unsubscribed topics: {:?}", topics);
        Ok(())
    }

    pub async fn send_cmd(&self, sensor: &SensorHandle) -> Result<(), MQTTError> {
        if cfg!(test) && !self.cli.is_connected() {
            return Ok(());
        }

        let cmd_topic = Self::build_topic(sensor, Self::CMD_TOPIC);
        let mut cmds = sensor.format_cmds();
        debug!(
            APP_LOGGING,
            "Send command to sensor {} - {:?}", cmd_topic, cmds
        );
        let payload: Vec<u8> = cmds.drain(..).map(|i| i.to_ne_bytes()[0]).collect();

        let publ = Message::new_retained(cmd_topic, payload, QOS);
        self.cli.publish(publ).await?;
        Ok(())
    }

    /// Parses and dispatches a mqtt message
    ///
    /// Returns nothing on submitted command, or the submitted sensor data
    fn on_sensor_message(
        sensor_data_sender: &UnboundedSender<(i32, SensorDataMessage)>,
        msg: Message,
    ) -> Result<(), MQTTError> {
        // parse message
        let path: Vec<&str> = msg.topic().splitn(4, '/').collect();
        if path.len() != 4 {
            return Err(MQTTError::PathError(format!(
                "Couldn't split topic: {}",
                msg.topic()
            )));
        } else if path[0] != Self::SENSOR_TOPIC {
            return Err(MQTTError::PathError(format!("Invalid topic: {}", path[0])));
        }

        let endpoint = path[1];
        let sensor_id: i32 = path[2].parse().or(Err(MQTTError::PathError(format!(
            "Couldn't parse sensor_id: {}",
            path[2]
        ))))?;
        let payload: &str = std::str::from_utf8(msg.payload()).or(Err(MQTTError::PayloadError(
            "Couldn't decode payload".to_string(),
        )))?;

        match endpoint {
            Self::DATA_TOPIC => {
                let sensor_dto = serde_json::from_str::<SensorDataMessage>(payload)?;

                // propagate event to rest of the app
                if let Err(e) = sensor_data_sender.send((sensor_id, sensor_dto)) {
                    crit!(APP_LOGGING, "Failed broadcast SensorDataMessage: {}", e);
                }
                Ok(())
            }
            Self::LOG_TOPIC => {
                info!(
                    APP_LOGGING,
                    "[Sensor({})] logs: {}",
                    sensor_id,
                    payload.to_string()
                );
                Ok(())
            }
            _ => Err(MQTTError::PathError(format!(
                "Invalid endpoint: {}",
                endpoint
            ))),
        }
    }

    /*
     * Helpers
     */

    fn create_client(mqtt_name: String, sender: MqttSender) -> AsyncClient {
        let options = CreateOptionsBuilder::new()
            .client_id(mqtt_name.clone())
            
            .mqtt_version(MQTTV5)
            .finalize();
        let mut cli = AsyncClient::new(options).unwrap();

        // Setup callbacks
        let owned_sender = sender.clone();
        cli.set_message_callback(move |_cli, msg| {
            if let Some(msg) = msg {
                debug!(APP_LOGGING, "Received topic: {}", msg.topic());
                if let Err(e) = Self::on_sensor_message(&owned_sender, msg) {
                    error!(APP_LOGGING, "Received message threw error: {}", e);
                }
            }
        });
        let rt = tokio::runtime::Handle::current();
        cli.set_connection_lost_callback(move |cli| {
            if let Err(e) = cli.reconnect().wait_for(Duration::from_secs(3)) {
                error!(
                    APP_LOGGING,
                    "Failed reconnected to broker due {}, resuming on other broker", e,
                );

                // needs tu run in async(!) tokio context
                let owned_cli = cli.clone();
                rt.spawn(async move {
                    Self::do_connect(&owned_cli).await;
                });
            } else {
                info!(APP_LOGGING, "Reconnected to previous MQTT broker");
            }
        });
        cli
    }

    async fn do_connect(cli: &AsyncClient) {
        loop {
            let mqtt_uri = vec![CONFIG.next_mqtt_broker().clone()];
            let conn_opts = ConnectOptionsBuilder::new()
                .server_uris(&mqtt_uri)
                .mqtt_version(MQTTV5)
                .will_message(Message::new(Self::TESTAMENT_TOPIC, vec![], 2))
                .finalize();

            if let Err(e) = cli.connect(conn_opts).wait_for(Duration::from_secs(2)) {
                error!(
                    APP_LOGGING,
                    "Coulnd't connect to broker {} with {}", mqtt_uri[0], e
                );
            } else {
                info!(APP_LOGGING, "connected to broker {}", mqtt_uri[0]);
                break;
            }
        }
    }

    fn build_topics(sensor: &SensorHandle) -> Vec<String> {
        vec![
            Self::build_topic(sensor, Self::DATA_TOPIC),
            Self::build_topic(sensor, Self::LOG_TOPIC),
        ]
    }

    fn build_topic(sensor: &SensorHandle, topic: &str) -> String {
        format!(
            "{}/{}/{}/{}",
            Self::SENSOR_TOPIC,
            topic,
            sensor.id(),
            sensor.key_b64(),
        )
    }
}
