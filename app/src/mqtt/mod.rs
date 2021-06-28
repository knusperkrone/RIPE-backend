use std::sync::Arc;
use std::time::Duration;

use crate::config::CONFIG;
use crate::error::MQTTError;
use crate::logging::APP_LOGGING;
use crate::sensor::handle::SensorHandle;
use paho_mqtt::{AsyncClient, ConnectOptionsBuilder, CreateOptionsBuilder, Message};
use ripe_core::SensorDataMessage;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

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
    pub async fn connect(&self) -> bool {
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

    pub async fn connect(self: &Arc<MqttSensorClientInner>) -> bool {
        let write_res = self.write_cli().await;
        if write_res.is_err() {
            return false;
        }
        let mut connect_cli = write_res.unwrap();

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
            let future_self = disconnect_self.clone();
            rt.block_on(async move {
                Self::try_reconnect(future_self, callback_cli).await;
            });
        });
        drop(connect_cli);

        Self::do_connect(self.clone()).await;
        true
    }

    pub async fn broker(&self) -> Option<String> {
        if let Ok(cli) = self.read_cli().await {
            if cli.is_connected() {
                return Some(CONFIG.current_mqtt_broker());
            }
        }
        None
    }

    pub async fn subscribe_sensor(&self, sensor: &SensorHandle) -> Result<(), MQTTError> {
        let cli = self.read_cli().await?;
        if cfg!(test) && !cli.is_connected() {
            return Ok(());
        }

        let topics = Self::build_topics(sensor);
        if let Err(_) = tokio::time::timeout(
            self.default_timeout(),
            cli.subscribe_many(&topics, &vec![QOS, QOS]),
        )
        .await
        {
            return Err(MQTTError::TimeoutError());
        }

        debug!(APP_LOGGING, "Subscribed topics {:?}", topics);
        Ok(())
    }

    pub async fn unsubscribe_sensor(&self, sensor: &SensorHandle) -> Result<(), MQTTError> {
        let cli = self.read_cli().await?;
        if cfg!(test) && !cli.is_connected() {
            return Ok(());
        }

        let topics = Self::build_topics(sensor);
        if let Err(_) =
            tokio::time::timeout(self.default_timeout(), cli.unsubscribe_many(&topics)).await
        {
            return Err(MQTTError::TimeoutError());
        }

        debug!(APP_LOGGING, "Unsubscribed topics: {:?}", topics);
        Ok(())
    }

    pub async fn send_cmd(&self, sensor: &SensorHandle) -> Result<(), MQTTError> {
        let cli = self.read_cli().await?;
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
        if let Err(_) = tokio::time::timeout(self.default_timeout(), cli.publish(publ)).await {
            return Err(MQTTError::TimeoutError());
        }
        Ok(())
    }

    async fn try_reconnect(self: Arc<Self>, cli: &AsyncClient) {
        if let Ok(_) = tokio::time::timeout(Duration::from_secs(5), cli.reconnect()).await {
            info!(APP_LOGGING, "Reconnected to previous MQTT broker");
            return;
        }
        error!(
            APP_LOGGING,
            "Failed reconnected on current broker due, resuming on other broker",
        );

        loop {
            // create a new instance due an internal double free error
            if let Ok(mut inner_cli) = self.write_cli().await {
                inner_cli.set_connection_lost_callback(|_| {});
                *inner_cli = Self::create_client(); // needs async context
                break;
            } else {
                crit!(APP_LOGGING, "Failed to acquire mqtt-cli write lock")
            }
        }
        while !MqttSensorClientInner::connect(&self).await {
            error!(APP_LOGGING, "Failed connecting inside of an lost context",);
        }
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

    fn default_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_millis(2500)
    }

    async fn read_cli(&self) -> Result<RwLockReadGuard<'_, AsyncClient>, MQTTError> {
        tokio::time::timeout(self.default_timeout(), self.cli.read())
            .await
            .map_err(|_| MQTTError::ReadLockError())
    }

    async fn write_cli(&self) -> Result<RwLockWriteGuard<'_, AsyncClient>, MQTTError> {
        let duration = std::time::Duration::from_secs(1);
        tokio::time::timeout(duration, self.cli.write())
            .await
            .map_err(|_| MQTTError::WriteLockError())
    }

    fn create_client() -> AsyncClient {
        if let Err(_) = tokio::runtime::Handle::try_current() {
            panic!("PahoMqtt needs async context here");
        }
        CreateOptionsBuilder::new().create_client().unwrap()
    }

    async fn do_connect(self: Arc<Self>) {
        loop {
            let mqtt_uri = vec![CONFIG.next_mqtt_broker().clone()];
            let conn_opts = ConnectOptionsBuilder::new()
                .server_uris(&mqtt_uri)
                .keep_alive_interval(Duration::from_secs(3))
                .will_message(Message::new(Self::TESTAMENT_TOPIC, vec![], 2))
                .finalize();

            if let Ok(cli) = self.read_cli().await {
                info!(APP_LOGGING, "Attempt connecting on broker {}", mqtt_uri[0]);
                if let Err(e) =
                    tokio::time::timeout(self.default_timeout(), cli.connect(conn_opts)).await
                {
                    error!(
                        APP_LOGGING,
                        "Coulnd't connect to broker {} with {}", mqtt_uri[0], e
                    );
                } else {
                    info!(APP_LOGGING, "connected to broker {}", mqtt_uri[0]);
                    break;
                }
            } else {
                crit!(
                    APP_LOGGING,
                    "Failed aquiring lock to connect on broker {}",
                    mqtt_uri[0]
                );
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
