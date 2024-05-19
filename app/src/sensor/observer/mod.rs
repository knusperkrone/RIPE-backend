use super::container::SensorContainer;
use super::handle::{SensorHandle, SensorMQTTCommand};
use super::SensorMessage;
use crate::config::CONFIG;
use crate::error::{DBError, ObserverError};
use crate::models::{
    agent::{self as agent_model, AgentConfigDao},
    agent_command::{self as agent_command_model},
    sensor::{self as sensor_model, SensorDao},
    sensor_data::{self as sensor_data_model},
    sensor_log::{self as sensor_log_model},
};
use crate::mqtt::MqttSensorClient;
use crate::plugin::AgentFactory;

use chrono::Utc;
use notify::{Config, PollWatcher, Watcher};
use ripe_core::AgentUI;
use ripe_core::SensorDataMessage;
use sqlx::PgPool;
use std::fmt::Debug;
use std::iter::zip;
use std::{path::Path, sync::Arc, time::Duration};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, error, info, info_span, warn, Instrument};

pub mod agent;
pub mod controller;

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

impl Debug for ConcurrentObserver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConcurrentObserver").finish()
    }
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
            db_conn,
        };
        Arc::new(observer)
    }

    #[tracing::instrument]
    pub async fn init(&self) {
        self.load_plugins().await;
        self.mqtt_client.connect().await;
        if let Err(e) = self.populate_agents().await {
            error!("{}", e);
            panic!();
        }
    }

    #[tracing::instrument]
    pub async fn load_plugins(&self) {
        let mut agent_factory = self.agent_factory.write().await;
        agent_factory.load_new_plugins(self.plugin_dir.as_path());
    }

    /// Starts the paho-thread, registers all callbacks
    /// After a successful connection, the agent's get inited from the database
    /// On each message event the according sensor get's called and the data get's persistet
    /// Blocks caller thread in infinite loop
    pub async fn dispatch_mqtt_receive_loop(self: Arc<ConcurrentObserver>) {
        let reveiver_res = self.data_receveiver.try_lock();
        if reveiver_res.is_err() {
            error!("dispatch_mqtt_receive_loop() already called!");
            return;
        }
        let mut receiver = reveiver_res.unwrap();

        loop {
            info!("Start capturing sensor data events");
            while let Some((sensor_id, msg)) = receiver.recv().await {
                match msg {
                    SensorMessage::Data(tracing, data) => {
                        let span = info_span!(parent: &tracing, "data");
                        let _enter = span.enter();
                        self.persist_sensor_data(sensor_id, data)
                            .instrument(span.clone())
                            .await
                    }
                    SensorMessage::Log(tracing, log) => {
                        let span = info_span!(parent: &tracing, "logging");
                        let _enter = span.enter();
                        self.persist_sensor_log(sensor_id, log)
                            .instrument(span.clone())
                            .await
                    }
                    SensorMessage::Reconnected(tracing) => {
                        let span = info_span!(parent: &tracing, "logging");
                        let _enter = span.enter();

                        let resubscribe_count =
                            self.resubscribe_sensors().instrument(span.clone()).await;
                        let all_count = self.container.read().await.len();
                        if resubscribe_count != all_count {
                            info!("Resubscribed {}/{} sensors", resubscribe_count, all_count)
                        } else {
                            info!("Resubscribed all sensors",)
                        }
                    }
                };
            }
            error!("Failed listening sensor data events");
        }
    }

    /// Dispatches the inter-agent-communication (iac) stream
    /// Each agent sends it's mqtt command over this channel
    /// Blocks caller thread in infinite loop
    pub async fn dispatch_iac_loop(self: Arc<ConcurrentObserver>) {
        let receiver_res = self.iac_receiver.try_lock();
        if receiver_res.is_err() {
            error!("dispatch_mqtt_send_loop() already called!");
            return;
        }

        let mut receiver = receiver_res.unwrap();
        let max_retries = CONFIG.mqtt_send_retries();
        loop {
            info!("Start capturing iac events");
            while let Some(item) = receiver.recv().await {
                let span = info_span!(parent: &item.tracing, "iac_message_received");
                let _enter = span.enter();

                let container = self.container.read().await;
                let sensor_opt = container
                    .sensor_unchecked(item.sensor_id)
                    .instrument(span.clone())
                    .await;
                if let Some(sensor) = sensor_opt {
                    let mut is_sent = false;
                    for i in 0..max_retries {
                        match self
                            .mqtt_client
                            .send_cmd(&sensor)
                            .instrument(span.clone())
                            .await
                        {
                            Ok(_) => {
                                if let Err(e) = agent_command_model::insert(
                                    &self.db_conn,
                                    item.sensor_id,
                                    &item.domain,
                                    item.payload,
                                )
                                .await
                                {
                                    error!("Failed persisting agent command: {}", e);
                                }
                                is_sent = true;
                                break;
                            }
                            Err(e) => {
                                warn!(
                                    "[{}/{}] Failed sending command {:?} with {}",
                                    i, max_retries, item, e
                                )
                            }
                        }
                    }

                    if !is_sent {
                        error!(
                            "Failed sending command after {} retries: {:?}",
                            max_retries, item
                        );
                    }
                }
            }
        }
    }

    /// Dispatches a interval checking loop
    /// checks if the plugin libary files got updated
    /// Blocks caller thread in infinite loop
    pub async fn dispatch_plugin_refresh_loop(self: Arc<ConcurrentObserver>) {
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

        if let Err(e) = watcher.watch(plugin_dir, notify::RecursiveMode::NonRecursive) {
            error!("Cannot watch plugin dir: {}", e);
            return;
        }

        info!("Start watching plugin dir: {:?}", plugin_dir);
        loop {
            if rx.try_recv().is_ok() {
                let mut agent_factory = self.agent_factory.write().await;
                let lib_names = agent_factory.load_new_plugins(plugin_dir);
                if !lib_names.is_empty() {
                    info!("Updating plugins {:?}", lib_names);

                    let now = Utc::now();
                    let container = self.container.write().await;

                    for sensor_mtx in container.sensors() {
                        let mut sensor = sensor_mtx.lock().await;
                        sensor.update(&lib_names, &agent_factory);

                        if let Ok(Some(data)) =
                            sensor_data_model::get_latest_unchecked(&self.db_conn, sensor.id())
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
            if let Err(e) = sensor_data_model::insert(
                &self.db_conn,
                sensor_id,
                data,
                chrono::Duration::minutes(30),
            )
            .await
            {
                error!(sensor_id = sensor_id, "Failed persiting sensor data: {}", e);
            }
        } else {
            warn!(sensor_id = sensor_id, "Sensor not found");
        }
    }

    async fn persist_sensor_log(&self, sensor_id: i32, log_msg: std::string::String) {
        let max_logs = CONFIG.mqtt_log_count();
        if let Err(e) = sensor_log_model::upsert(&self.db_conn, sensor_id, log_msg, max_logs).await
        {
            error!("Failed persiting sensor log: {}", e);
        }
    }

    async fn resubscribe_sensors(&self) -> usize {
        let mut count = 0;
        let container = self.container.read().await;
        for sensor_mtx in container.sensors() {
            let sensor = sensor_mtx.lock().await;
            if let Err(e) = self.subscribe_sensor(&sensor).await {
                error!(sensr_id = sensor.id(), "Failed observing sensor: {}", e);
            } else {
                debug!(sensor_id = sensor.id(), "Resubscribed sensor");
                count += 1;
            }
        }
        count
    }

    async fn populate_agents(&self) -> Result<(), DBError> {
        // TODO: Stream
        let start = Utc::now();
        let mut sensor_daos = Vec::new();
        for sensor_dao in sensor_model::read(&self.db_conn).await? {
            let agent_configs = agent_model::get(&self.db_conn, &sensor_dao).await.unwrap();
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
                error!("{}", e)
            }
        }

        let duration = Utc::now() - start;
        info!(
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
        let sensor = SensorHandle::from(sensor_dao, configs.unwrap_or_default(), agent_factory)?;

        self.subscribe_sensor(&sensor).await?;
        self.container.write().await.insert_sensor(sensor);
        Ok(sensor_id)
    }

    async fn subscribe_sensor(&self, sensor: &SensorHandle) -> Result<(), ObserverError> {
        self.mqtt_client.subscribe_sensor(sensor).await?;
        for _ in 0..CONFIG.mqtt_send_retries() {
            if let Err(e) = self.mqtt_client.send_cmd(sensor).await {
                error!(
                    sensor_id = sensor.id(),
                    "Failed sending initial mqtt command: {}", e
                );
            } else {
                debug!(sensor_id = sensor.id(), "Sent initial mqtt command");
                for (agent, command) in zip(sensor.agents(), sensor.format_cmds()) {
                    if let Err(e) = agent_command_model::insert(
                        &self.db_conn,
                        sensor.id(),
                        agent.domain(),
                        command,
                    )
                    .await
                    {
                        error!("Failed persisting initial agent command: {}", e);
                    }
                }
                break;
            }
        }
        Ok(())
    }
}
