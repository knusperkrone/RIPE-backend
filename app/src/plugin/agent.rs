use super::AgentLib;
use crate::logging::APP_LOGGING;
use crate::{models::agent::AgentConfigDao, sensor::handle::SensorMQTTCommand};
use chrono_tz::Tz;
use parking_lot::{Mutex, RwLock};
use ripe_core::{
    AgentConfigType, AgentMessage, AgentStreamReceiver, AgentTrait, AgentUI, FutBuilder,
    SensorDataMessage,
};
use std::{
    collections::HashMap,
    string::String,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc,
    },
};
use tokio::{sync::mpsc::UnboundedSender, task::JoinHandle};
use uuid::Uuid;

static TERMINATED: AtomicBool = AtomicBool::new(false);
static TASK_COUNTER: AtomicU32 = AtomicU32::new(0);

pub fn register_sigint_handler() {
    // Set termination handler
    ctrlc::set_handler(|| {
        if TERMINATED.load(Ordering::Relaxed) {
            std::process::exit(0);
        }

        let task_count = TASK_COUNTER.load(Ordering::Relaxed);
        if task_count == 0 {
            std::process::exit(0);
        }

        TERMINATED.store(true, Ordering::Relaxed);
        info!(
            APP_LOGGING,
            "Waiting for {} agent tasks to finish..", task_count
        );
    })
    .unwrap();
}

/*
 * Agent impl
 */

static MAX_TASK_COUNT: usize = 5;

pub struct Agent {
    inner: Arc<AgentInner>,
    agent_proxy: Box<dyn AgentTrait>,
    iac_abort_handle: JoinHandle<()>,
}

pub struct AgentInner {
    sensor_id: i32,
    domain: Arc<String>,
    agent_name: Arc<String>,
    agent_lib: AgentLib,
    repeat_task_handle: RwLock<Option<JoinHandle<()>>>,
    task_handles: Mutex<HashMap<Uuid, JoinHandle<()>>>,
}

impl Drop for Agent {
    fn drop(&mut self) {
        // stop listening
        self.iac_abort_handle.abort();

        // stop tasks
        if let Some(repeat_handle) = self.inner.repeat_task_handle.write().as_ref() {
            repeat_handle.abort();
        }
        let handles = self.inner.task_handles.lock();
        for handle in handles.values() {
            handle.abort();
        }

        debug!(
            APP_LOGGING,
            "Lib references {}",
            Arc::strong_count(&self.inner.agent_lib.0)
        );
    }
}

impl std::fmt::Debug for Agent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        write!(f, "{:?}", "AgentProxy")
    }
}

/// A agent, is the lower level wrapper around a AgentTrait implementation
/// It holds all necessary meta-information, for sending messages to the sensor
/// All global messages, are handled via the with iac_sender
impl Agent {
    pub fn new(
        iac_sender: UnboundedSender<SensorMQTTCommand>,
        plugin_receiver: AgentStreamReceiver,
        sensor_id: i32,
        domain: String,
        agent_lib: AgentLib,
        agent_name: String,
        mut agent_proxy: Box<dyn AgentTrait>,
    ) -> Self {
        let task_handles = Mutex::new(HashMap::with_capacity(MAX_TASK_COUNT));
        let repeat_task_handle = RwLock::new(None);
        let inner = Arc::new(AgentInner {
            sensor_id,
            domain: Arc::new(domain),
            agent_name: Arc::new(agent_name),
            agent_lib,
            repeat_task_handle,
            task_handles,
        });

        agent_proxy.init();

        Agent {
            inner: inner.clone(),
            agent_proxy,
            iac_abort_handle: tokio::spawn(Agent::dispatch_iac(inner, iac_sender, plugin_receiver)),
        }
    }

    pub fn handle_cmd(&mut self, cmd: i64) {
        info!(
            APP_LOGGING,
            "{}-{} agent cmd: {}", self.inner.sensor_id, self.inner.domain, cmd
        );
        self.agent_proxy.handle_cmd(cmd);
    }

    pub fn handle_data(&mut self, data: &SensorDataMessage) {
        self.agent_proxy.handle_data(data);
    }

    pub fn cmd(&self) -> i32 {
        self.agent_proxy.cmd()
    }

    pub fn render_ui(&self, data: &SensorDataMessage, timezone: Tz) -> AgentUI {
        self.agent_proxy.render_ui(data, timezone)
    }

    pub fn config(&self, timezone: Tz) -> HashMap<String, (String, AgentConfigType)> {
        let mut config = self.agent_proxy.config(timezone);
        let mut transformed = HashMap::with_capacity(config.len());
        for (k, (v0, v1)) in config.drain() {
            transformed.insert(k.to_owned(), (v0.to_owned(), v1));
        }
        transformed
    }

    pub fn set_config(&mut self, config: &HashMap<String, AgentConfigType>, timezone: Tz) -> bool {
        self.agent_proxy.set_config(config, timezone)
    }

    pub fn deserialize(&self) -> AgentConfigDao {
        let deserialized = self.agent_proxy.deserialize();
        AgentConfigDao::new(
            self.inner.sensor_id,
            (*self.inner.domain).clone(),
            (*self.inner.agent_name).clone(),
            deserialized,
        )
    }

    pub fn agent_name(&self) -> &String {
        &self.inner.agent_name
    }

    pub fn domain(&self) -> &String {
        &self.inner.domain
    }

    /*
     * Helpers
     */

    async fn dispatch_iac(
        agent: Arc<AgentInner>,
        iac_sender: UnboundedSender<SensorMQTTCommand>,
        mut plugin_receiver: AgentStreamReceiver,
    ) {
        info!(
            APP_LOGGING,
            "[{}][{}]Iac ready", agent.sensor_id, agent.domain
        );

        while let Ok(payload_opt) = plugin_receiver.recv().await {
            if payload_opt.is_none() {
                continue;
            }
            match payload_opt.unwrap() {
                AgentMessage::OneshotTask(agent_task_builder) => {
                    Agent::dispatch_oneshot_task(agent.clone(), agent_task_builder).await;
                }
                AgentMessage::RepeatedTask(delay, interval_task) => {
                    Agent::dispatch_repating_task(agent.clone(), delay, interval_task).await;
                }
                AgentMessage::Command(command) => {
                    debug!(
                        APP_LOGGING,
                        "[{}][{}] Received command {:?}", agent.sensor_id, agent.domain, command
                    );
                    // Notify main loop over agent
                    let mut msg = SensorMQTTCommand {
                        sensor_id: agent.sensor_id,
                        domain: agent.domain.clone(),
                        payload: command,
                    };
                    let mut tries = 3;
                    while tries != 0 {
                        if let Err(e) = iac_sender.send(msg) {
                            error!(APP_LOGGING, "[{}/3] Failed iac send: {}", tries, e);
                            tries -= 1;
                            msg = e.0;
                        } else {
                            break;
                        }
                    }
                    debug!(APP_LOGGING, "Handled receive command {:?}", command);
                }
            };
            debug!(
                APP_LOGGING,
                "[{}][{}]Iac ready for next message", agent.sensor_id, agent.domain
            );
        }
        info!(
            APP_LOGGING,
            "[{}][{}]Iac ended", agent.sensor_id, agent.domain
        );
    }

    async fn dispatch_oneshot_task(agent: Arc<AgentInner>, agent_task: Box<dyn FutBuilder>) {
        if TERMINATED.load(Ordering::Relaxed) {
            info!(
                APP_LOGGING,
                "Task for {} {} was declined as app recv SIGINT", agent.sensor_id, agent.agent_name
            );
            return;
        } else if agent.task_handles.lock().len() > MAX_TASK_COUNT {
            info!(APP_LOGGING, "Task was declined due too many running tasks");
            return;
        }
        info!(
            APP_LOGGING,
            "[{}][{}] Dispatching oneshot task for", agent.sensor_id, agent.agent_name
        );

        // Run as task a sender messenges are blocked otherwise
        let task_id = Uuid::new_v4();
        let future_agent = agent.clone();
        let task_agent = agent.clone();
        let one_shot_future = async move {
            let mut task_count = TASK_COUNTER.fetch_add(1, Ordering::Relaxed) + 1;
            debug!(
                APP_LOGGING,
                "Spawning new task for sensor: {} - active tasks: {}", agent.sensor_id, task_count
            );

            // This is actual vodoo
            let handle = tokio::runtime::Handle::current();
            let res = tokio::join!(tokio::spawn(agent_task.build_future(handle)));
            if let (Err(err),) = res {
                error!(
                    APP_LOGGING,
                    "Task for {} {} failed: {:?}", agent.sensor_id, agent.agent_name, err
                );
            }

            task_count = TASK_COUNTER.fetch_sub(1, Ordering::Relaxed) - 1;
            info!(
                APP_LOGGING,
                "Ended new oneshot task for sensor: {} - active tasks: {}",
                agent.sensor_id,
                task_count
            );
            if task_count == 0 && TERMINATED.load(Ordering::Relaxed) {
                std::process::exit(0);
            }

            // remove handle from the list
            let mut handles = future_agent.task_handles.lock();
            handles.remove(&task_id);
        };

        // Start abortable task, safe handle and remove handle after completion
        let handle = tokio::task::spawn(one_shot_future);
        task_agent.task_handles.lock().insert(task_id, handle);
    }

    async fn dispatch_repating_task(
        agent: Arc<AgentInner>,
        delay: std::time::Duration,
        interval_task: Box<dyn FutBuilder>,
    ) {
        let repeat_agent = agent.clone();
        if agent.repeat_task_handle.read().is_some() {
            warn!(
                APP_LOGGING,
                "[{}][{}] already has a repeating task", agent.sensor_id, agent.agent_name,
            );
            return;
        }
        info!(
            APP_LOGGING,
            "[{}][{}] Dispatching repeating task", agent.sensor_id, agent.agent_name
        );

        let repeating_future = async move {
            debug!(
                APP_LOGGING,
                "Starting repeating task for sensor: {}, {} - sleep duration: {:?}",
                repeat_agent.sensor_id,
                repeat_agent.agent_name,
                delay,
            );

            loop {
                let _ = TASK_COUNTER.fetch_add(1, Ordering::Relaxed);
                let handle = tokio::runtime::Handle::current();
                let result = tokio::join!(tokio::spawn(interval_task.build_future(handle)));

                let task_count = TASK_COUNTER.fetch_sub(1, Ordering::Relaxed) - 1;
                if task_count == 0 && TERMINATED.load(Ordering::Relaxed) {
                    std::process::exit(0);
                }

                match result {
                    (Ok(is_finished),) => {
                        if is_finished {
                            break;
                        }
                    }
                    (Err(_),) => {
                        warn!(
                            APP_LOGGING,
                            "Repeating task for sensor: {} was aborted", repeat_agent.sensor_id
                        );
                        break;
                    }
                };
                tokio::time::sleep(delay).await;
            }

            info!(
                APP_LOGGING,
                "Ended repeating task for sensor: {}", repeat_agent.sensor_id
            );
            *repeat_agent.repeat_task_handle.write() = None;
        };

        let handle = tokio::spawn(repeating_future);
        *agent.repeat_task_handle.write() = Some(handle);
    }
}
