use crate::error::MQTTError;
use crate::logging::APP_LOGGING;
use crate::sensor::{handle::SensorHandle, observer::SensorCache};
use dotenv::dotenv;
use iftem_core::SensorDataMessage;
use paho_mqtt::{AsyncClient, ConnectOptionsBuilder, CreateOptionsBuilder, Message};
use std::env;
use std::sync::{Arc, Mutex, MutexGuard, RwLock};
use tokio::sync::mpsc::UnboundedSender;

#[cfg(test)]
mod test;

const MQTTV5: u32 = 5;
const QOS: i32 = 1;

pub struct MqttSensorClient {
    mqtt_client: Arc<Mutex<AsyncClient>>,
}

impl MqttSensorClient {
    pub const SENSOR_TOPIC: &'static str = "sensor";
    pub const CMD_TOPIC: &'static str = "cmd";
    pub const DATA_TOPIC: &'static str = "data";
    pub const LOG_TOPIC: &'static str = "log";

    pub fn new(
        data_sender: UnboundedSender<(i32, SensorDataMessage)>,
        container_ref: Arc<RwLock<SensorCache>>,
    ) -> Self {
        dotenv().ok();
        let mqtt_name: String = env::var("MQTT_NAME").expect("MQTT_NAME must be set");
        let mqtt_url: String = env::var("MQTT_URL").expect("MQTT_URL must be set");
        let mqtt_port: i32 = env::var("MQTT_PORT")
            .expect("MQTT_PORT must be set")
            .parse()
            .unwrap();

        let mqtt_uri = format!("tcp://{}:{}", mqtt_url, mqtt_port);
        info!(APP_LOGGING, "MQTT config: {}", mqtt_uri);
        // Create a client to the specified host, no persistence
        let create_opts = CreateOptionsBuilder::new()
            .mqtt_version(MQTTV5)
            .client_id(mqtt_name)
            .server_uri(mqtt_uri)
            .finalize();
        let raw_mqtt_client = AsyncClient::new(create_opts).unwrap();
        let shared_mqtt_client_mtx = Arc::new(Mutex::new(raw_mqtt_client));

        {
            let diconnect_mqtt_client_mtx = shared_mqtt_client_mtx.clone();
            let container_message_ref = container_ref.clone();

            let mut mqtt_client = shared_mqtt_client_mtx.lock().unwrap();
            mqtt_client.set_message_callback(move |_cli, msg| {
                if let Some(msg) = msg {
                    // TODO: Message Channel!
                    debug!(APP_LOGGING, "Received msg for topic: {}", msg.topic());
                    let _ = MqttSensorClient::on_sensor_message(
                        &data_sender,
                        &container_message_ref,
                        msg,
                    );
                }
            });
            mqtt_client.set_connection_lost_callback(move |_cli| {
                error!(APP_LOGGING, "Lost connected to MQTT");
                let cli_mtx = diconnect_mqtt_client_mtx.clone();
                let cli = cli_mtx.lock().unwrap();
                if let Err(e) = cli.reconnect().wait_for(std::time::Duration::from_secs(5)) {
                    panic!("Coulnd't reconnect to mqtt: {}", e);
                } else {
                    info!(APP_LOGGING, "Reconnected to MQTT");
                }
            });
        }

        MqttSensorClient {
            mqtt_client: shared_mqtt_client_mtx,
        }
    }

    pub async fn connect(&self) {
        let cli_mtx = &self.mqtt_client;
        let cli = cli_mtx.lock().unwrap();

        let conn_opts = ConnectOptionsBuilder::new()
            .clean_start(true)
            .mqtt_version(MQTTV5)
            .finalize();
        if let Err(e) = cli.connect(conn_opts).await {
            panic!("Coulnd't connect to MQTT: {}", e);
        } else {
            info!(APP_LOGGING, "MQTT connected!");
        }
    }

    pub async fn subscribe_sensor(&mut self, sensor: &SensorHandle) -> Result<(), MQTTError> {
        let cli_mtx = &self.mqtt_client;
        let cli = cli_mtx.lock().unwrap();
        if cfg!(test) && !cli.is_connected() {
            return Ok(());
        }
        MqttSensorClient::do_subscribe_sensor(&cli, sensor).await
    }

    pub async fn unsubscribe_sensor(&mut self, sensor: &SensorHandle) -> Result<(), MQTTError> {
        let cli_mtx = &self.mqtt_client;
        let cli = cli_mtx.lock().unwrap();
        if cfg!(test) && !cli.is_connected() {
            return Ok(());
        }
        MqttSensorClient::do_unsubscribe_sensor(&cli, sensor).await
    }

    pub async fn send_cmd(&mut self, sensor: &SensorHandle) -> Result<(), MQTTError> {
        let cli_mtx = &self.mqtt_client;
        let cli = cli_mtx.lock().unwrap();
        if cfg!(test) && !cli.is_connected() {
            return Ok(());
        }
        MqttSensorClient::do_send_cmd(&cli, sensor).await
    }

    async fn do_subscribe_sensor(
        mqtt_client: &MutexGuard<'_, AsyncClient>,
        sensor: &SensorHandle,
    ) -> Result<(), MQTTError> {
        let topics = MqttSensorClient::build_topics(sensor);
        mqtt_client.subscribe_many(&topics, &vec![QOS, QOS]).await?;

        info!(APP_LOGGING, "Subscribed topics {:?}", topics);
        Ok(())
    }

    async fn do_unsubscribe_sensor(
        mqtt_client: &MutexGuard<'_, AsyncClient>,
        sensor: &SensorHandle,
    ) -> Result<(), MQTTError> {
        let topics = MqttSensorClient::build_topics(sensor);
        mqtt_client.unsubscribe_many(&topics).await?;

        info!(APP_LOGGING, "Unsubscribed topics: {:?}", topics);
        Ok(())
    }

    async fn do_send_cmd(
        mqtt_client: &MutexGuard<'_, AsyncClient>,
        sensor: &SensorHandle,
    ) -> Result<(), MQTTError> {
        let cmd_topic = MqttSensorClient::build_topic(sensor, MqttSensorClient::CMD_TOPIC);
        info!(APP_LOGGING, "Send command to sensor {}", cmd_topic);

        let mut cmds = sensor.format_cmds();
        let payload: Vec<u8> = cmds.drain(..).map(|i| i.to_ne_bytes()[0]).collect();

        let publ = Message::new_retained(cmd_topic, payload, QOS);
        mqtt_client.publish(publ).await?;

        info!(APP_LOGGING, "Send command to sensor {}", sensor.id());
        Ok(())
    }

    /// Parses and dispatches a mqtt message
    ///
    /// Returns nothing on submitted command, or the submitted sensor data
    fn on_sensor_message(
        sender: &UnboundedSender<(i32, SensorDataMessage)>,
        container_lock: &RwLock<SensorCache>,
        msg: Message,
    ) -> Result<(), MQTTError> {
        info!(APP_LOGGING, "MSG {}", msg.topic());
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
                let container = container_lock.read().unwrap();
                let mut sensor = container
                    .sensor(sensor_id, key)
                    .ok_or(MQTTError::NoSensor())?;
                sensor.on_data(&sensor_dto);
                if let Err(e) = sender.send((sensor_id, sensor_dto)) {
                    warn!(APP_LOGGING, "Failed sending SensorDataMessage: {}", e);
                }
                Ok(())
            }
            MqttSensorClient::LOG_TOPIC => {
                info!(
                    APP_LOGGING,
                    "Log sensor[{}]: {}",
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

    fn build_topics(sensor: &SensorHandle) -> Vec<String> {
        vec![
            MqttSensorClient::build_topic(sensor, MqttSensorClient::DATA_TOPIC),
            MqttSensorClient::build_topic(sensor, MqttSensorClient::LOG_TOPIC),
        ]
    }

    fn build_topic(sensor: &SensorHandle, topic: &str) -> String {
        format!(
            "{}/{}/{}/{}",
            MqttSensorClient::SENSOR_TOPIC,
            topic,
            sensor.id(),
            sensor.key_b64(),
        )
    }
}
