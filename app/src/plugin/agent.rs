use crate::error::PluginError;
use crate::logging::APP_LOGGING;
use crate::plugin::AgentFactory;
use crate::{models::dao::AgentConfigDao, sensor::handle::SensorHandleMessage};
use futures::future::{AbortHandle, Abortable};
use iftem_core::{
    AgentConfigType, AgentMessage, AgentState, AgentTrait, AgentUI, FutBuilder, SensorDataMessage,
};
use libloading::Library;
use std::{
    collections::HashMap,
    string::String,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, RwLock,
    },
};
use tokio::sync::mpsc::{Receiver, Sender, UnboundedSender};

static TERMINATED: AtomicU32 = AtomicU32::new(0);
static TASK_COUNTER: AtomicU32 = AtomicU32::new(0);

pub fn register_sigint_handler() {
    // Set termination handler
    ctrlc::set_handler(|| {
        let count = TERMINATED.fetch_add(1, Ordering::Relaxed);
        if count == 1 {
            info!(APP_LOGGING, "Force killing");
            std::process::exit(0);
        }

        let task_count = TASK_COUNTER.load(Ordering::Relaxed);
        if task_count == 0 {
            std::process::exit(0);
        }
        info!(APP_LOGGING, "Waiting for {} tasks to finish", task_count);
    })
    .unwrap();
}

pub struct Agent {
    sensor_id: i32,
    domain: String,
    plugin_sender: Sender<AgentMessage>,
    agent_name: String,
    abort_handle: AbortHandle,
    repeat_abort_handle: Arc<RwLock<Option<AbortHandle>>>,
    agent_proxy: Box<dyn AgentTrait>,
    needs_update: bool,
    library_ref: Arc<Library>, // Reference counter, to not free lib .text section
}

impl std::fmt::Debug for Agent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        write!(f, "{:?}", self.agent_proxy)
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        self.abort_handle.abort();
        if let Some(repeat_handle) = self.repeat_abort_handle.write().unwrap().as_ref() {
            repeat_handle.abort();
        }
    }
}

/// A agent, is the lower level wrapper around a AgentTrait implementation
/// It holds all necessary meta-information, for sending messages to the sensor
/// All messages, are handled via the iac-stream
impl Agent {
    pub fn new(
        agent_sender: UnboundedSender<SensorHandleMessage>,
        plugin_sender: Sender<AgentMessage>,
        plugin_receiver: Receiver<AgentMessage>,
        sensor_id: i32,
        domain: String,
        agent_name: String,
        agent_proxy: Box<dyn AgentTrait>,
        library_ref: Arc<Library>,
    ) -> Self {
        let repeat_abort_handle = Arc::new(RwLock::new(None));
        let (abort_handle, abort_registration) = AbortHandle::new_pair();
        let ipc_future = Abortable::new(
            Agent::dispatch_iac(
                sensor_id,
                domain.clone(),
                agent_sender,
                plugin_receiver,
                repeat_abort_handle.clone(),
            ),
            abort_registration,
        );
        tokio::spawn(async move { ipc_future.await });

        Agent {
            sensor_id,
            domain,
            plugin_sender,
            agent_name,
            abort_handle,
            repeat_abort_handle,
            agent_proxy,
            library_ref,
            needs_update: false,
        }
    }

    pub fn reload_agent(&mut self, factory: &AgentFactory) -> Result<(), PluginError> {
        let agent_state = self.agent_proxy.state();
        if agent_state != AgentState::Ready && agent_state != AgentState::Disabled {
            info!(
                APP_LOGGING,
                "Failed reloading active agent: {}", self.sensor_id
            );
            return Err(PluginError::AgentStateError(agent_state));
        }

        // TODO: FIX RACE condition for tasks
        // TODO: Kill running tasks

        info!(
            APP_LOGGING,
            "Reloading agent: {} - {}", self.sensor_id, self.domain
        );
        let state_json = self.agent_proxy.deserialize().state_json;
        unsafe {
            let (libary, proxy) = factory
                .build_proxy_agent(&self.agent_name, Some(&state_json), &self.plugin_sender)
                .ok_or_else(|| PluginError::Duplicate(self.agent_name.clone()))?;
            self.library_ref = libary;
            self.agent_proxy = proxy;
        }
        self.needs_update = false;
        Ok(())
    }

    pub fn on_cmd(&mut self, cmd: i64) {
        info!(
            APP_LOGGING,
            "{}-{} agent cmd: {}", self.sensor_id, self.domain, cmd
        );
        self.agent_proxy.on_cmd(cmd);
    }

    pub fn on_data(&mut self, data: &SensorDataMessage) {
        self.agent_proxy.on_data(data);
    }

    pub fn cmd(&self) -> i32 {
        self.agent_proxy.cmd()
    }

    pub fn render_ui(&self, data: &SensorDataMessage) -> AgentUI {
        self.agent_proxy.render_ui(data)
    }

    pub fn config(&self) -> HashMap<String, (String, AgentConfigType)> {
        let mut config = self.agent_proxy.config();
        let mut transformed = HashMap::with_capacity(config.len());
        for (k, (v0, v1)) in config.drain() {
            transformed.insert(k.to_owned(), (v0.to_owned(), v1));
        }
        transformed
    }

    pub fn on_config(&mut self, config: &HashMap<String, AgentConfigType>) -> bool {
        self.agent_proxy.on_config(config)
    }

    pub fn deserialize(&self) -> AgentConfigDao {
        let config = self.agent_proxy.deserialize();
        AgentConfigDao::new(
            self.sensor_id,
            self.domain.clone(),
            config.name,
            config.state_json,
        )
    }

    pub fn agent_name(&self) -> &String {
        &self.agent_name
    }

    pub fn domain(&self) -> &String {
        &self.domain
    }

    pub fn set_needs_update(&mut self, needs_update: bool) {
        self.needs_update = needs_update;
    }

    pub fn needs_update(&self) -> bool {
        self.needs_update
    }

    /*
     * Helpers
     */

    async fn dispatch_iac(
        sensor_id: i32,
        domain: String,
        agent_sender: UnboundedSender<SensorHandleMessage>,
        mut plugin_receiver: Receiver<AgentMessage>,
        repeat_abort_handle: Arc<RwLock<Option<AbortHandle>>>,
    ) {
        while let Some(payload) = plugin_receiver.recv().await {
            match payload {
                AgentMessage::OneshotTask(agent_task_builder) => {
                    Agent::dispatch_oneshot_task(sensor_id, agent_task_builder).await;
                }
                AgentMessage::RepeatedTask(delay, interval_task) => {
                    Agent::dispatch_repating_task(
                        sensor_id,
                        delay,
                        interval_task,
                        repeat_abort_handle.clone(),
                    )
                    .await;
                }
                AgentMessage::Command(command) => {
                    // Notify main loop over agent
                    let mut msg = SensorHandleMessage {
                        sensor_id: sensor_id,
                        domain: domain.clone(),
                        payload: command,
                    };

                    let mut tries = 3;
                    while tries != 0 {
                        if let Err(e) = agent_sender.send(msg) {
                            error!(APP_LOGGING, "[{}/3] Error sending payload: {}", tries, e);
                            tries -= 1;
                            msg = e.0;
                        } else {
                            break;
                        }
                    }
                }
            };
        }
        crit!(APP_LOGGING, "dispatch_iac endend for {}", sensor_id);
    }

    async fn dispatch_oneshot_task(sensor_id: i32, agent_task: Box<dyn FutBuilder>) {
        if TERMINATED.load(Ordering::Relaxed) != 0 {
            info!(APP_LOGGING, "Task wasn't started as app is cancelled");
        } else {
            tokio::spawn(async move {
                let mut task_count = TASK_COUNTER.fetch_add(1, Ordering::Relaxed) + 1;
                info!(
                    APP_LOGGING,
                    "Spawning new task for sensor: {} - active tasks: {}", sensor_id, task_count
                );

                agent_task.build_future().await;

                task_count = TASK_COUNTER.fetch_sub(1, Ordering::Relaxed) - 1;
                info!(
                    APP_LOGGING,
                    "Ended new oneshot task for sensor: {} - active tasks: {}",
                    sensor_id,
                    task_count
                );
                if TERMINATED.load(Ordering::Relaxed) != 0 && task_count == 0 {
                    std::process::exit(0);
                }
            });
        }
    }

    async fn dispatch_repating_task(
        sensor_id: i32,
        delay: std::time::Duration,
        interval_task: Box<dyn FutBuilder>,
        repeat_abort_handle: Arc<RwLock<Option<AbortHandle>>>,
    ) {
        let repeat_handle_ref = repeat_abort_handle.clone();
        let mut handle = repeat_abort_handle.write().unwrap();
        if handle.is_none() {
            let (abort_handle, abort_registration) = AbortHandle::new_pair();
            let repeating_future = Abortable::new(
                async move {
                    info!(APP_LOGGING, "Started repeating task for {}", sensor_id);

                    info!(
                        APP_LOGGING,
                        "Spawning repeating task for sensor: {} - sleep duration: {:?}",
                        sensor_id,
                        delay,
                    );
                    loop {
                        let _ = TASK_COUNTER.fetch_add(1, Ordering::Relaxed);

                        let is_finished = interval_task.build_future().await;
                        let task_count = TASK_COUNTER.fetch_sub(1, Ordering::Relaxed) - 1;
                        if task_count == 0 && TERMINATED.load(Ordering::Relaxed) != 0 {
                            std::process::exit(0);
                        }

                        if is_finished {
                            break;
                        }
                        tokio::time::delay_for(delay.into()).await;
                    }

                    info!(APP_LOGGING, "Ended repeating task for {}", sensor_id);
                    let mut handle = repeat_handle_ref.write().unwrap();
                    *handle = None;
                },
                abort_registration,
            );
            tokio::spawn(repeating_future);
            *handle = Some(abort_handle);
        } else {
            warn!(APP_LOGGING, "Already scheduled a repeating task!");
        }
    }
}
