use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use crate::config::CONFIG;
use crate::error::MQTTError;
use crate::logging::APP_LOGGING;
use crate::sensor::handle::SensorHandle;
use crate::sensor::SensorMessage;
use futures::future::{AbortHandle, Abortable};
use futures::StreamExt;
use paho_mqtt::{AsyncClient, ConnectOptionsBuilder, CreateOptionsBuilder, Message, SslOptions};
use parking_lot::Mutex;
use ripe_core::SensorDataMessage;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

#[cfg(test)]
mod test;

const QOS: i32 = 1;

pub struct Broker {
    pub tcp: Option<String>,
    pub wss: Option<String>,
}

type MqttSender = UnboundedSender<(i32, SensorMessage)>;

pub struct MqttSensorClient {
    inner: Arc<MqttSensorClientInner>,
}

struct MqttSensorClientInner {
    cli: RwLock<AsyncClient>,
    listen_abort_handle: Mutex<Option<AbortHandle>>,
    is_connected: AtomicBool,
    sender: MqttSender,
    timeout_ms: u64,
}

impl MqttSensorClient {
    pub fn new(sender: MqttSender) -> Self {
        let cli = MqttSensorClientInner::create_client();
        MqttSensorClient {
            inner: Arc::new(MqttSensorClientInner {
                cli: RwLock::new(cli),
                listen_abort_handle: Mutex::new(None),
                timeout_ms: CONFIG.mqtt_timeout_ms(),
                is_connected: AtomicBool::new(false),
                sender,
            }),
        }
    }

    pub async fn connect(&self) {
        MqttSensorClientInner::connect(self.inner.clone()).await
    }

    pub fn broker(&self) -> Broker {
        if self.inner.is_connected() {
            Broker {
                tcp: Some(CONFIG.mqtt_broker_internal()),
                wss: Some(CONFIG.mqtt_broker_external()),
            }
        } else {
            Broker {
                tcp: None,
                wss: None,
            }
        }
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

impl Drop for MqttSensorClientInner {
    fn drop(&mut self) {
        if let Some(handle) = self.listen_abort_handle.lock().take() {
            handle.abort();
        }
    }
}

impl MqttSensorClientInner {
    pub const TESTAMENT_TOPIC: &'static str = "ripe/master";
    pub const SENSOR_TOPIC: &'static str = "sensor";
    pub const CMD_TOPIC: &'static str = "cmd";
    pub const DATA_TOPIC: &'static str = "data";
    pub const LOG_TOPIC: &'static str = "log";

    pub async fn connect(self: Arc<MqttSensorClientInner>) {
        let write_res = self.write_cli().await;
        if write_res.is_err() {
            crit!(APP_LOGGING, "Failed aquiring lock on connect");
            return;
        }
        let mut connect_cli = write_res.unwrap();

        // Message callback
        let mut mqtt_stream = connect_cli.get_stream(16_384);
        drop(connect_cli);

        Self::do_connect(&self.clone()).await;

        let (abort_handle, abort_registration) = AbortHandle::new_pair();

        let mut current_abort = self.listen_abort_handle.lock();
        if let Some(handle) = current_abort.take() {
            warn!(APP_LOGGING, "MQTT already has a listen task - aborting..");
            handle.abort();
        }
        *current_abort = Some(abort_handle);

        let future_self = self.clone();
        tokio::spawn(Abortable::new(
            async move {
                while let Some(msg_opt) = mqtt_stream.next().await {
                    if let Some(msg) = msg_opt {
                        debug!(
                            APP_LOGGING,
                            "Received topic: {}, {:?}",
                            msg.topic(),
                            std::str::from_utf8(msg.payload())
                        );
                        if let Err(e) = Self::on_sensor_message(&future_self.sender, msg) {
                            error!(APP_LOGGING, "Received message threw error: {}", e);
                        }
                    } else {
                        // Try reconnect
                        let is_reconnected = if let Ok(cli) = future_self.read_cli().await {
                            cli.reconnect().wait_for(Duration::from_secs(5)).is_ok()
                        } else {
                            false
                        };

                        if is_reconnected {
                            info!(APP_LOGGING, "Reconnected to previous MQTT broker");
                        } else {
                            Self::do_connect(&future_self.clone()).await;
                        }
                        while let Err(e) = future_self.sender.send((0, SensorMessage::Reconnect)) {
                            warn!(APP_LOGGING, "Failed broadcast reconnect {}", e);
                        }
                    }
                }
                crit!(APP_LOGGING, "Ended MQTT message loop");
            },
            abort_registration,
        ));
    }

    pub(crate) fn is_connected(&self) -> bool {
        self.is_connected.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub async fn subscribe_sensor(&self, sensor: &SensorHandle) -> Result<(), MQTTError> {
        let cli = self.read_cli().await?;
        if cfg!(test) && !cli.is_connected() {
            return Ok(());
        }

        let topics = Self::build_topics(sensor);
        if let Err(_) = cli
            .subscribe_many(&topics, &vec![QOS, QOS])
            .wait_for(self.default_timeout())
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
        if let Err(_) = cli
            .unsubscribe_many(&topics)
            .wait_for(self.default_timeout())
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
        if let Err(_) = cli.publish(publ).wait_for(self.default_timeout()) {
            return Err(MQTTError::TimeoutError());
        }
        Ok(())
    }

    /// Parses and dispatches a mqtt message
    ///
    /// Returns nothing on submitted command, or the submitted sensor data
    fn on_sensor_message(sensor_data_sender: &MqttSender, msg: Message) -> Result<(), MQTTError> {
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
                if let Err(e) =
                    sensor_data_sender.send((sensor_id, SensorMessage::Data(sensor_dto)))
                {
                    crit!(APP_LOGGING, "Failed broadcast SensorDataMessage: {}", e);
                }
                Ok(())
            }
            Self::LOG_TOPIC => {
                debug!(
                    APP_LOGGING,
                    "[Sensor({})] logs: {}",
                    sensor_id,
                    payload.to_string()
                );
                if let Err(e) =
                    sensor_data_sender.send((sensor_id, SensorMessage::Log(payload.to_string())))
                {
                    crit!(APP_LOGGING, "Failed broadcast SensorLogMessage: {}", e);
                }
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
        std::time::Duration::from_millis(self.timeout_ms.into())
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

    async fn do_connect(self: &Arc<Self>) {
        self.is_connected
            .store(false, std::sync::atomic::Ordering::Relaxed);
        loop {
            let mqtt_uri = vec![CONFIG.mqtt_broker_internal().clone()];
            let conn_opts = ConnectOptionsBuilder::new_ws()
                .server_uris(&mqtt_uri)
                .ssl_options(SslOptions::new())
                .keep_alive_interval(Duration::from_secs(3))
                .will_message(Message::new(Self::TESTAMENT_TOPIC, vec![], 2))
                .finalize();

            if let Ok(cli) = self.read_cli().await {
                info!(APP_LOGGING, "Attempt connecting on broker {}", mqtt_uri[0]);
                if let Err(e) = cli.connect(conn_opts).wait_for(Duration::from_secs(5)) {
                    error!(
                        APP_LOGGING,
                        "Coulnd't connect to broker {} with {}", mqtt_uri[0], e
                    );
                } else {
                    self.is_connected
                        .store(true, std::sync::atomic::Ordering::Relaxed);
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
