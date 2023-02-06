use super::container::SensorContainer;
use super::handle::{SensorHandle, SensorMQTTCommand};
use super::SensorMessage;
use crate::config::CONFIG;
use crate::logging::APP_LOGGING;
use crate::models::{
    self,
    dao::{AgentConfigDao, SensorDao},
};
use crate::mqtt::MqttSensorClient;
use crate::plugin::AgentFactory;

use crate::error::{DBError, ObserverError};
use chrono::Utc;
use notify::{Config, PollWatcher, Watcher};
use ripe_core::AgentUI;
use ripe_core::SensorDataMessage;
use sqlx::PgPool;
use std::{path::Path, sync::Arc, time::Duration};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};
use tokio::sync::{Mutex, RwLock};

pub mod agent;
pub mod sensor;

pub struct Sensor {
    pub id: i32,
    pub key: String,
}

pub struct Agent {
    pub domain: String,
    pub name: String,
    pub ui: AgentUI,
}

pub struct ConcurrentObserver {
    pub(crate) mqtt_client: MqttSensorClient,
    pub(crate) container: RwLock<SensorContainer>,
    pub(crate) agent_factory: RwLock<AgentFactory>,
    pub(crate) db_conn: PgPool,
    plugin_dir: std::path::PathBuf,
    iac_receiver: Mutex<UnboundedReceiver<SensorMQTTCommand>>,
    data_receveiver: Mutex<UnboundedReceiver<(i32, SensorMessage)>>,
}

impl ConcurrentObserver {
    pub fn new(plugin_dir: &Path, db_conn: PgPool) -> Arc<Self> {
        let (iac_sender, iac_receiver) = unbounded_channel::<SensorMQTTCommand>();
        let (sensor_data_sender, data_receiver) = unbounded_channel::<(i32, SensorMessage)>();
        let agent_factory = AgentFactory::new(iac_sender);
        let container = SensorContainer::new();
        let mqtt_client = MqttSensorClient::new(sensor_data_sender);

        let observer = ConcurrentObserver {
            mqtt_client,
            plugin_dir: plugin_dir.to_owned(),
            container: RwLock::new(container),
            agent_factory: RwLock::new(agent_factory),
            iac_receiver: Mutex::new(iac_receiver),
            data_receveiver: Mutex::new(data_receiver),
            db_conn: db_conn,
        };
        Arc::new(observer)
    }

    pub async fn init(&self) {
        self.load_plugins().await;
        self.mqtt_client.connect().await;
        if let Err(e) = self.populate_agents().await {
            crit!(APP_LOGGING, "{}", e);
            panic!();
        }
    }

    pub async fn load_plugins(&self) {
        let mut agent_factory = self.agent_factory.write().await;
        agent_factory.load_new_plugins(self.plugin_dir.as_path());
    }

    /// Starts the paho-thread, registers all callbacks
    /// After a successful connection, the agent's get inited from the database
    /// On each message event the according sensor get's called and the data get's persistet
    /// Blocks caller thread in infinite loop
    pub async fn dispatch_mqtt_receive_loop(self: Arc<ConcurrentObserver>) -> () {
        let reveiver_res = self.data_receveiver.try_lock();
        if reveiver_res.is_err() {
            error!(APP_LOGGING, "dispatch_mqtt_receive_loop() already called!");
            return;
        }
        let mut receiver = reveiver_res.unwrap();

        loop {
            info!(APP_LOGGING, "Start capturing sensor data events");
            while let Some((sensor_id, msg)) = receiver.recv().await {
                match msg {
                    SensorMessage::Data(data) => self.persist_sensor_data(sensor_id, data).await,
                    SensorMessage::Log(log) => self.persist_sensor_log(sensor_id, log).await,
                    SensorMessage::Reconnect => {
                        let resubscribe_count = self.resubscribe_sensors().await;
                        let all_count = self.container.read().await.len();
                        if resubscribe_count != all_count {
                            info!(
                                APP_LOGGING,
                                "Resubscribed {}/{} sensors", resubscribe_count, all_count
                            )
                        } else {
                            info!(APP_LOGGING, "Resubscribed all sensors",)
                        }
                    }
                };
            }
            crit!(APP_LOGGING, "Failed listening sensor data events");
        }
    }

    /// Dispatches the inter-agent-communication (iac) stream
    /// Each agent sends it's mqtt command over this channel
    /// Blocks caller thread in infinite loop
    pub async fn dispatch_iac_loop(self: Arc<ConcurrentObserver>) -> () {
        let receiver_res = self.iac_receiver.try_lock();
        if receiver_res.is_err() {
            error!(APP_LOGGING, "dispatch_mqtt_send_loop() already called!");
            return;
        }

        let mut receiver = receiver_res.unwrap();
        let max_retries = CONFIG.mqtt_send_retries();
        loop {
            info!(APP_LOGGING, "Start capturing iac events");
            while let Some(item) = receiver.recv().await {
                let container = self.container.read().await;
                let sensor_opt = container.sensor_unchecked(item.sensor_id).await;
                if sensor_opt.is_some() {
                    let sensor = sensor_opt.unwrap();
                    for i in 0..max_retries {
                        match self.mqtt_client.send_cmd(&sensor).await {
                            Ok(_) => break,
                            Err(e) => {
                                error!(
                                    APP_LOGGING,
                                    "[{}/{}] Failed sending command {:?} with {}",
                                    i,
                                    max_retries,
                                    item,
                                    e
                                )
                            }
                        }
                    }
                }
            }
        }
    }

    /// Dispatches a interval checking loop
    /// checks if the plugin libary files got updated
    /// Blocks caller thread in infinite loop
    pub async fn dispatch_plugin_refresh_loop(self: Arc<ConcurrentObserver>) -> () {
        let plugin_dir = self.plugin_dir.as_path();

        // watch plugin dir
        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher = PollWatcher::new(
            tx,
            Config::default()
                .with_poll_interval(Duration::from_secs(5))
                .with_compare_contents(true),
        )
        .unwrap();

        if let Err(e) = watcher.watch(&plugin_dir, notify::RecursiveMode::NonRecursive) {
            crit!(APP_LOGGING, "Cannot watch plugin dir: {}", e);
            return;
        }

        info!(APP_LOGGING, "Start watching plugin dir: {:?}", plugin_dir);
        loop {
            if rx.try_recv().is_ok() {
                let mut agent_factory = self.agent_factory.write().await;
                let lib_names = agent_factory.load_new_plugins(plugin_dir);
                if !lib_names.is_empty() {
                    info!(APP_LOGGING, "Updating plugins {:?}", lib_names);

                    let now = Utc::now();
                    let container = self.container.write().await;

                    for sensor_mtx in container.sensors() {
                        let mut sensor = sensor_mtx.lock().await;
                        sensor.update(&lib_names, &agent_factory);

                        if let Ok(Some(data)) =
                            models::get_latest_sensor_data_unchecked(&self.db_conn, sensor.id())
                                .await
                        {
                            let casted: SensorDataMessage = data.into();
                            if (casted.timestamp - now) < chrono::Duration::minutes(60) {
                                sensor.handle_data(&casted); // last hour
                            }
                        }
                    }
                }
            }
            let timeout = if cfg!(prod) { 5 } else { 1 };
            tokio::time::sleep(Duration::from_secs(timeout)).await;
        }
    }

    async fn persist_sensor_data(&self, sensor_id: i32, data: SensorDataMessage) {
        if let Some(mut sensor) = self
            .container
            .write()
            .await
            .sensor_unchecked(sensor_id)
            .await
        {
            sensor.handle_data(&data);
            if let Err(e) = models::insert_sensor_data(
                &self.db_conn,
                sensor_id,
                data,
                chrono::Duration::minutes(30),
            )
            .await
            {
                error!(APP_LOGGING, "Failed persiting sensor data: {}", e);
            }
        } else {
            warn!(APP_LOGGING, "Sensor not found: {}", sensor_id);
        }
    }

    async fn persist_sensor_log(&self, sensor_id: i32, log_msg: std::string::String) {
        let max_logs = CONFIG.mqtt_log_count();
        if let Err(e) = models::insert_sensor_log(&self.db_conn, sensor_id, log_msg, max_logs).await
        {
            error!(APP_LOGGING, "Failed persiting sensor log: {}", e);
        }
    }

    async fn resubscribe_sensors(&self) -> usize {
        let mut count = 0;
        let container = self.container.read().await;
        for sensor_mtx in container.sensors() {
            let sensor = sensor_mtx.lock().await;
            if let Err(e) = self.subscribe_sensor(&sensor).await {
                error!(
                    APP_LOGGING,
                    "Failed observing sensor {} - {}",
                    sensor.id(),
                    e
                );
            } else {
                debug!(APP_LOGGING, "Resubscribed sensor {}", sensor.id());
                count += 1;
            }
        }
        count
    }

    async fn populate_agents(&self) -> Result<(), DBError> {
        // TODO: Stream
        let start = Utc::now();
        let mut sensor_daos = Vec::new();
        for sensor_dao in models::get_sensors(&self.db_conn).await? {
            let agent_configs = models::get_agent_config(&self.db_conn, &sensor_dao)
                .await
                .unwrap();
            sensor_daos.push((sensor_dao, agent_configs));
        }

        let mut count = sensor_daos.len();
        let agent_factory = self.agent_factory.read().await;
        for (sensor_dao, agent_configs) in sensor_daos {
            if let Err(e) = self
                .insert_sensor(&agent_factory, sensor_dao, Some(agent_configs))
                .await
            {
                count -= 1;
                error!(APP_LOGGING, "{}", e)
            }
        }

        let duration = Utc::now() - start;
        info!(
            APP_LOGGING,
            "Restored {} sensors in {} ms",
            count,
            duration.num_milliseconds()
        );
        Ok(())
    }

    async fn insert_sensor(
        &self,
        agent_factory: &AgentFactory,
        sensor_dao: SensorDao,
        configs: Option<Vec<AgentConfigDao>>,
    ) -> Result<i32, ObserverError> {
        let sensor_id = sensor_dao.id();
        let sensor = SensorHandle::from(sensor_dao, configs.unwrap_or(vec![]), &agent_factory)?;

        self.subscribe_sensor(&sensor).await?;
        self.container.write().await.insert_sensor(sensor);
        Ok(sensor_id)
    }

    async fn subscribe_sensor(&self, sensor: &SensorHandle) -> Result<(), ObserverError> {
        self.mqtt_client.subscribe_sensor(sensor).await?;
        for _ in 0..CONFIG.mqtt_send_retries() {
            if let Err(e) = self.mqtt_client.send_cmd(sensor).await {
                error!(APP_LOGGING, "Failed sending initial mqtt command: {}", e);
            } else {
                debug!(
                    APP_LOGGING,
                    "Sent initial mqtt command to sensor {}",
                    sensor.id()
                );
                break;
            }
        }
        Ok(())
    }
}
