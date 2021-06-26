use std::sync::Arc;
use std::time::Duration;

use crate::config::CONFIG;
use crate::error::MQTTError;
use crate::logging::APP_LOGGING;
use crate::sensor::handle::SensorHandle;
use paho_mqtt::{AsyncClient, ConnectOptionsBuilder, CreateOptionsBuilder, Message};
use ripe_core::SensorDataMessage;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::RwLock;

#[cfg(test)]
mod test;

const QOS: i32 = 1;

type MqttSender = UnboundedSender<(i32, SensorDataMessage)>;

pub struct MqttSensorClient {
    inner: Arc<MqttSensorClientInner>,
}

struct MqttSensorClientInner {
    cli: RwLock<AsyncClient>,
    sender: MqttSender,
}

impl MqttSensorClient {
    pub fn new(sender: MqttSender) -> Self {
        let cli = MqttSensorClientInner::create_client();
        MqttSensorClient {
            inner: Arc::new(MqttSensorClientInner {
                cli: RwLock::new(cli),
                sender,
            }),
        }
    }
    pub async fn connect(&self) {
        MqttSensorClientInner::connect(&self.inner).await
    }

    pub async fn broker(&self) -> Option<String> {
        self.inner.broker().await
    }

    pub async fn subscribe_sensor(&self, sensor: &SensorHandle) -> Result<(), MQTTError> {
        self.inner.subscribe_sensor(sensor).await
    }

    pub async fn unsubscribe_sensor(&self, sensor: &SensorHandle) -> Result<(), MQTTError> {
        self.inner.unsubscribe_sensor(sensor).await
    }

    pub async fn send_cmd(&self, sensor: &SensorHandle) -> Result<(), MQTTError> {
        self.inner.send_cmd(sensor).await
    }
}

impl MqttSensorClientInner {
    pub const TESTAMENT_TOPIC: &'static str = "ripe/master";
    pub const SENSOR_TOPIC: &'static str = "sensor";
    pub const CMD_TOPIC: &'static str = "cmd";
    pub const DATA_TOPIC: &'static str = "data";
    pub const LOG_TOPIC: &'static str = "log";

    pub async fn connect(self: &Arc<MqttSensorClientInner>) {
        let mut connect_cli = self.cli.write().await;

        // Message callback
        let connect_self = self.clone();
        connect_cli.set_message_callback(move |_cli, msg| {
            if let Some(msg) = msg {
                debug!(
                    APP_LOGGING,
                    "Received topic: {}, {:?}",
                    msg.topic(),
                    std::str::from_utf8(msg.payload())
                );
                if let Err(e) = Self::on_sensor_message(&connect_self.sender, msg) {
                    error!(APP_LOGGING, "Received message threw error: {}", e);
                }
            }
        });

        // Disconnect callback
        let rt = tokio::runtime::Handle::current();
        let disconnect_self = self.clone();
        connect_cli.set_connection_lost_callback(move |callback_cli| {
            error!(APP_LOGGING, "Lost connection to MQTT broker");
            if let Err(e) = callback_cli.reconnect().wait_for(Duration::from_secs(3)) {
                let future_self = disconnect_self.clone();
                rt.block_on(async move {
                    {
                        // create a new instance due an internal double free error
                        let mut cli = future_self.cli.write().await;
                        *cli = Self::create_client(); // needs async context
                    }

                    error!(
                        APP_LOGGING,
                        "Failed reconnected to broker due {}, resuming on other broker", e,
                    );
                    MqttSensorClientInner::connect(&future_self).await;
                });
            } else {
                info!(APP_LOGGING, "Reconnected to previous MQTT broker");
            }
        });

        Self::do_connect(&connect_cli).await;
    }

    pub async fn broker(&self) -> Option<String> {
        if self.cli.read().await.is_connected() {
            Some(CONFIG.current_mqtt_broker())
        } else {
            None
        }
    }

    pub async fn subscribe_sensor(&self, sensor: &SensorHandle) -> Result<(), MQTTError> {
        let cli = self.cli.read().await;
        if cfg!(test) && !cli.is_connected() {
            return Ok(());
        }

        let topics = Self::build_topics(sensor);
        cli.subscribe_many(&topics, &vec![QOS, QOS]).await?;

        debug!(APP_LOGGING, "Subscribed topics {:?}", topics);
        Ok(())
    }

    pub async fn unsubscribe_sensor(&self, sensor: &SensorHandle) -> Result<(), MQTTError> {
        let cli = self.cli.read().await;
        if cfg!(test) && !cli.is_connected() {
            return Ok(());
        }

        let topics = Self::build_topics(sensor);
        cli.unsubscribe_many(&topics).await?;

        debug!(APP_LOGGING, "Unsubscribed topics: {:?}", topics);
        Ok(())
    }

    pub async fn send_cmd(&self, sensor: &SensorHandle) -> Result<(), MQTTError> {
        let cli = self.cli.read().await;
        if cfg!(test) && !cli.is_connected() {
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
        cli.publish(publ).await?;
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

    fn create_client() -> AsyncClient {
        if let Err(_) = tokio::runtime::Handle::try_current() {
            panic!("PahoMqtt needs async context here");
        }
        CreateOptionsBuilder::new().create_client().unwrap()
    }

    async fn do_connect(cli: &tokio::sync::RwLockWriteGuard<'_, AsyncClient>) {
        if let Err(_) = tokio::runtime::Handle::try_current() {
            panic!("PahoMqtt needs async context here");
        }

        loop {
            let mqtt_uri = vec![CONFIG.next_mqtt_broker().clone()];
            let conn_opts = ConnectOptionsBuilder::new()
                .server_uris(&mqtt_uri)
                .keep_alive_interval(Duration::from_secs(3))
                .will_message(Message::new(Self::TESTAMENT_TOPIC, vec![], 2))
                .finalize();

            info!(APP_LOGGING, "Attempt connecting on broker {}", mqtt_uri[0]);
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
