use async_recursion::async_recursion;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed};
use std::sync::Arc;

use crate::config::CONFIG;
use crate::error::MQTTError;
use crate::logging::APP_LOGGING;
use crate::sensor::handle::SensorHandle;
use crate::sensor::SensorMessage;
use futures::future::{AbortHandle, Abortable};
use ripe_core::SensorDataMessage;
use rumqttc::{
    AsyncClient, Event, EventLoop, LastWill, MqttOptions, Packet, Publish, QoS, Transport,
};

use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::{RwLock, RwLockReadGuard};

#[cfg(test)]
mod test;

const QOS: QoS = QoS::ExactlyOnce;

#[derive(Debug, serde::Serialize, serde::Deserialize, utoipa::ToSchema, Clone)]
#[serde(rename_all(serialize = "lowercase", deserialize = "lowercase"))]
pub enum MqttScheme {
    Tcp,
    Wss,
}

#[derive(Debug, Clone)]
pub struct BrokerCredentials {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone)]
pub struct MqttConnectionDetail {
    pub scheme: MqttScheme,
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone)]
pub struct MqttBroker {
    pub connection: MqttConnectionDetail,
    pub credentials: Option<BrokerCredentials>,
}

type MqttSender = UnboundedSender<(i32, SensorMessage)>;

pub struct MqttSensorClient {
    inner: Arc<MqttSensorClientInner>,
}

struct MqttSensorClientInner {
    cli: RwLock<Option<(AsyncClient, AbortHandle)>>,
    broker_index: AtomicUsize,
    is_connected: AtomicBool,
    sender: MqttSender,
    timeout_ms: u64,
}

impl MqttSensorClient {
    pub fn new(sender: MqttSender) -> Self {
        MqttSensorClient {
            inner: Arc::new(MqttSensorClientInner {
                cli: RwLock::new(None),
                broker_index: AtomicUsize::new(0),
                timeout_ms: CONFIG.mqtt_timeout_ms(),
                is_connected: AtomicBool::new(false),
                sender,
            }),
        }
    }

    pub async fn connect(&self) {
        MqttSensorClientInner::connect(self.inner.clone()).await
    }

    pub fn external_brokers(&self) -> Option<&Vec<MqttBroker>> {
        if self.inner.is_connected() {
            let brokers = CONFIG.brokers();
            Some(&brokers[self.inner.broker_index.load(Relaxed) % brokers.len()].external)
        } else {
            None
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
        /*
        use tokio::runtime::Handle;
        if let Ok(handle) = Handle::try_current() {
            let guard = handle.block_on(self.cli.read());
            if let Some((client, abort_handle)) = guard.as_ref() {
                abort_handle.abort();
                let _ = handle.block_on(client.disconnect());
            } else {
                warn!(
                    APP_LOGGING,
                    "No async context, client may leak connection ressources"
                );
            }
        }
         */
    }
}

impl MqttSensorClientInner {
    pub const TESTAMENT_TOPIC: &'static str = "ripe/master";
    pub const SENSOR_TOPIC: &'static str = "sensor";
    pub const CMD_TOPIC: &'static str = "cmd";
    pub const DATA_TOPIC: &'static str = "data";
    pub const LOG_TOPIC: &'static str = "log";

    #[async_recursion]
    pub async fn connect(self: Arc<MqttSensorClientInner>) {
        let brokers = CONFIG.brokers();
        let i = self.broker_index.fetch_add(1, Relaxed);
        let broker = &brokers[i % brokers.len()].internal;

        info!(
            APP_LOGGING,
            "Connecting to MQTT broker: {:?}://{}:{}",
            broker.connection.scheme,
            broker.connection.host,
            broker.connection.port
        );
        let mqttoptions = Self::build_connection(broker).expect("Invalid broker uri");
        let (client, mut eventloop) = AsyncClient::new(mqttoptions, 1024);

        match eventloop.poll().await {
            Ok(Event::Incoming(Packet::ConnAck(_))) => {
                info!(APP_LOGGING, "Connected to MQTT broker");
            }
            Err(e) => {
                error!(APP_LOGGING, "Couldn't connect to MQTT broker {}", e);
                return self.connect().await;
            }
            e => unreachable!("Unexpected event: {:?}", e),
        }

        let handle = self.clone().listen(eventloop);
        self.cli.write().await.replace((client, handle));
        self.is_connected.store(true, Relaxed);
    }

    fn listen(self: Arc<MqttSensorClientInner>, mut mqtt_stream: EventLoop) -> AbortHandle {
        let (abort_handle, abort_registration) = AbortHandle::new_pair();
        let future_self = self.clone();

        tokio::spawn(Abortable::new(
            async move {
                while let Ok(event) = mqtt_stream.poll().await {
                    if let Event::Incoming(Packet::Publish(msg)) = event {
                        debug!(
                            APP_LOGGING,
                            "Received topic: {}, {:?}",
                            msg.topic,
                            std::str::from_utf8(&msg.payload)
                        );
                        if let Err(e) = Self::on_sensor_message(&future_self.sender, msg) {
                            error!(APP_LOGGING, "Received message threw error: {}", e);
                        }
                    }
                }

                self.is_connected.store(false, Relaxed);
                error!(APP_LOGGING, "MQTT disconnected - reconnecting");

                Self::connect(future_self.clone()).await;
                while let Err(e) = future_self.sender.send((0, SensorMessage::Reconnect)) {
                    warn!(APP_LOGGING, "Failed broadcast reconnect {}", e);
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            },
            abort_registration,
        ));

        abort_handle
    }

    pub(crate) fn is_connected(&self) -> bool {
        self.is_connected.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub async fn subscribe_sensor(&self, sensor: &SensorHandle) -> Result<(), MQTTError> {
        if cfg!(test) {
            return Ok(());
        }

        let guard = self.client().await?;
        if let Some((client, _)) = guard.as_ref() {
            let topics = Self::build_topics(sensor);
            for topic in &topics {
                client.subscribe(topic, QOS).await?;
            }

            debug!(APP_LOGGING, "Subscribed topics {:?}", topics);
            Ok(())
        } else {
            Err(MQTTError::NotConnected())
        }
    }

    pub async fn unsubscribe_sensor(&self, sensor: &SensorHandle) -> Result<(), MQTTError> {
        if cfg!(test) {
            return Ok(());
        }

        let guard = self.client().await?;
        if let Some((client, _)) = guard.as_ref() {
            let topics = Self::build_topics(sensor);
            for topic in &topics {
                client.unsubscribe(topic).await?;
            }

            debug!(APP_LOGGING, "Unsubscribed topics {:?}", topics);
            Ok(())
        } else {
            Err(MQTTError::NotConnected())
        }
    }

    pub async fn send_cmd(&self, sensor: &SensorHandle) -> Result<(), MQTTError> {
        if cfg!(test) {
            return Ok(());
        }

        let guard = self.client().await?;
        if let Some((client, _)) = guard.as_ref() {
            let cmd_topic = Self::build_topic(sensor, Self::CMD_TOPIC);
            let mut cmds = sensor.format_cmds();
            let payload: Vec<u8> = cmds.drain(..).map(|i| i.to_ne_bytes()[0]).collect();
            client.publish(&cmd_topic, QOS, true, payload).await?;

            debug!(APP_LOGGING, "Published command {:?}", cmd_topic);
            Ok(())
        } else {
            Err(MQTTError::NotConnected())
        }
    }

    /// Parses and dispatches a mqtt message
    ///
    /// Returns nothing on submitted command, or the submitted sensor data
    fn on_sensor_message(sensor_data_sender: &MqttSender, msg: Publish) -> Result<(), MQTTError> {
        // parse message
        let path: Vec<&str> = msg.topic.splitn(4, '/').collect();
        if path.len() != 4 {
            return Err(MQTTError::Path(format!(
                "Couldn't split topic: {}",
                msg.topic
            )));
        } else if path[0] != Self::SENSOR_TOPIC {
            return Err(MQTTError::Path(format!("Invalid topic: {}", path[0])));
        }

        let endpoint = path[1];
        let sensor_id: i32 = path[2].parse().or(Err(MQTTError::Path(format!(
            "Couldn't parse sensor_id: {}",
            path[2]
        ))))?;
        let payload: &str = std::str::from_utf8(&msg.payload).or(Err(MQTTError::Payload(
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
            _ => Err(MQTTError::Path(format!("Invalid endpoint: {}", endpoint))),
        }
    }

    /*
     * Helpers
     */

    async fn client(
        &self,
    ) -> Result<RwLockReadGuard<Option<(AsyncClient, AbortHandle)>>, MQTTError> {
        if !self.is_connected() {
            Err(MQTTError::NotConnected())
        } else {
            tokio::time::timeout(self.default_timeout(), self.cli.read())
                .await
                .map_err(|_| MQTTError::ReadLock())
        }
    }

    fn default_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.timeout_ms)
    }

    fn build_connection(broker: &MqttBroker) -> Result<MqttOptions, MQTTError> {
        let mut options = match broker.connection.scheme {
            MqttScheme::Tcp => MqttOptions::new(
                CONFIG.mqtt_client_id(),
                &broker.connection.host,
                broker.connection.port,
            ),
            MqttScheme::Wss => {
                let uri = format!(
                    "wss://{}:{}",
                    broker.connection.host, broker.connection.port
                ); // Weird flex, but ok
                let mut options =
                    MqttOptions::new(CONFIG.mqtt_client_id(), uri, broker.connection.port);
                options.set_transport(Transport::wss_with_default_config());
                options
            }
        };

        options.set_keep_alive(std::time::Duration::from_secs(5));
        options.set_last_will(LastWill::new(
            Self::TESTAMENT_TOPIC,
            "master".as_bytes(),
            QOS,
            false,
        ));
        if let Some(credentials) = &broker.credentials {
            debug!(
                APP_LOGGING,
                "Authentication to broker as: {}", credentials.username
            );
            options.set_credentials(credentials.username.clone(), credentials.password.clone());
        }
        Ok(options)
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
