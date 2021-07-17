use super::logging::PLUGIN_LOGGING;
use super::AgentLib;
use crate::logging::APP_LOGGING;
use crate::{models::dao::AgentConfigDao, sensor::handle::SensorMQTTCommand};
use chrono_tz::Tz;
use futures::future::AbortHandle;
use futures::future::Abortable;
use parking_lot::{Mutex, RwLock};
use ripe_core::{
    AgentConfigType, AgentMessage, AgentTrait, AgentUI, FutBuilder, SensorDataMessage,
};
use std::sync::atomic::AtomicBool;
use std::{
    collections::HashMap,
    string::String,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
};
use tokio::sync::mpsc::{Receiver, UnboundedSender};

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
}

pub struct AgentInner {
    sensor_id: i32,
    domain: String,
    agent_name: String,
    agent_lib: AgentLib,
    iac_abort_handle: AbortHandle,
    repeat_task_handle: RwLock<Option<AbortHandle>>,
    task_handles: Mutex<Vec<(usize, AbortHandle)>>,
}

impl Drop for AgentInner {
    fn drop(&mut self) {
        self.iac_abort_handle.abort();
        // stop tasks
        if let Some(repeat_handle) = self.repeat_task_handle.write().as_ref() {
            repeat_handle.abort();
        }
        let mut handles = self.task_handles.lock();
        for handle in handles.drain(..) {
            handle.1.abort();
        }

        debug!(
            APP_LOGGING,
            "Lib references {}",
            Arc::strong_count(&self.agent_lib.0)
        );
    }
}

impl std::fmt::Debug for Agent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        write!(f, "{:?}", self.agent_proxy)
    }
}

/// A agent, is the lower level wrapper around a AgentTrait implementation
/// It holds all necessary meta-information, for sending messages to the sensor
/// All global messages, are handled via the with iac_sender
impl Agent {
    pub fn new(
        iac_sender: UnboundedSender<SensorMQTTCommand>,
        plugin_receiver: Receiver<AgentMessage>,
        sensor_id: i32,
        domain: String,
        agent_lib: AgentLib,
        agent_name: String,
        agent_proxy: Box<dyn AgentTrait>,
    ) -> Self {
        let task_handles = Mutex::new(Vec::new());
        let repeat_task_handle = RwLock::new(None);
        let (abort_handle, abort_registration) = AbortHandle::new_pair();
        let inner = Arc::new(AgentInner {
            sensor_id,
            domain,
            agent_name,
            agent_lib,
            iac_abort_handle: abort_handle,
            repeat_task_handle,
            task_handles,
        });

        let ipc_future = Abortable::new(
            Agent::dispatch_iac(inner.clone(), iac_sender, plugin_receiver),
            abort_registration,
        );
        tokio::spawn(async move { ipc_future.await });

        Agent { inner, agent_proxy }
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
            self.inner.domain.clone(),
            self.inner.agent_name.clone(),
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
        mut plugin_receiver: Receiver<AgentMessage>,
    ) {
        debug!(
            APP_LOGGING,
            "[{}][{}]Iac ready", agent.sensor_id, agent.domain
        );

        while let Some(payload) = plugin_receiver.recv().await {
            match payload {
                AgentMessage::OneshotTask(agent_task_builder) => {
                    debug!(
                        PLUGIN_LOGGING,
                        "[{}][{}]OneShotTask", agent.sensor_id, agent.domain
                    );
                    Agent::dispatch_oneshot_task(agent.clone(), agent_task_builder).await;
                }
                AgentMessage::RepeatedTask(delay, interval_task) => {
                    debug!(
                        PLUGIN_LOGGING,
                        "[{}][{}]RepeatedTask", agent.sensor_id, agent.domain
                    );
                    Agent::dispatch_repating_task(agent.clone(), delay, interval_task).await;
                }
                AgentMessage::Command(command) => {
                    debug!(PLUGIN_LOGGING, "Received command {:?}", command);
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
                }
            };
        }
        // plugin update
        debug!(APP_LOGGING, "dispatch_iac endend for {}", agent.sensor_id);
    }

    async fn dispatch_oneshot_task(agent: Arc<AgentInner>, agent_task: Box<dyn FutBuilder>) {
        if TERMINATED.load(Ordering::Relaxed) {
            info!(
                PLUGIN_LOGGING,
                "Task for {} {} was declined as app recv SIGINT", agent.sensor_id, agent.agent_name
            );
        } else if agent.task_handles.lock().len() > MAX_TASK_COUNT {
            info!(
                PLUGIN_LOGGING,
                "Task was declined due too many running tasks"
            );
        } else {
            // Run as task a sender messenges are blocked otherwise
            let (abort_handle, abort_registration) = AbortHandle::new_pair();
            let abortable_agent = agent.clone();
            let task_future = Abortable::new(
                async move {
                    let mut task_count = TASK_COUNTER.fetch_add(1, Ordering::Relaxed) + 1;
                    debug!(
                        PLUGIN_LOGGING,
                        "Spawning new task for sensor: {} - active tasks: {}",
                        agent.sensor_id,
                        task_count
                    );

                    let runtime = tokio::runtime::Handle::current();
                    agent_task.build_future(runtime).await;

                    task_count = TASK_COUNTER.fetch_sub(1, Ordering::Relaxed) - 1;
                    debug!(
                        PLUGIN_LOGGING,
                        "Ended new oneshot task for sensor: {} - active tasks: {}",
                        agent.sensor_id,
                        task_count
                    );
                    if task_count == 0 && TERMINATED.load(Ordering::Relaxed) {
                        std::process::exit(0);
                    }
                },
                abort_registration,
            );

            // Start abortable task, safe handle and remove handle after completion
            let future_agent = abortable_agent.clone();
            tokio::spawn(async move {
                let handle_addr = std::ptr::addr_of!(abort_handle) as usize;
                {
                    let mut handles = future_agent.task_handles.lock();
                    handles.push((handle_addr, abort_handle));
                }
                let _ = task_future.await;

                // remove all aborted handles from the list
                let mut handles = future_agent.task_handles.lock();
                for i in 0..handles.len() {
                    if handles[i].0 == handle_addr {
                        handles.remove(i);
                    }
                    break;
                }
            });
        }
    }

    async fn dispatch_repating_task(
        agent: Arc<AgentInner>,
        delay: std::time::Duration,
        interval_task: Box<dyn FutBuilder>,
    ) {
        let repeat_agent = agent.clone();
        let mut handle = agent.repeat_task_handle.write();
        if handle.is_none() {
            let (abort_handle, abort_registration) = AbortHandle::new_pair();
            let repeating_future = Abortable::new(
                async move {
                    debug!(
                        PLUGIN_LOGGING,
                        "Starting repeating task for sensor: {}, {} - sleep duration: {:?}",
                        repeat_agent.sensor_id,
                        repeat_agent.agent_name,
                        delay,
                    );
                    loop {
                        let _ = TASK_COUNTER.fetch_add(1, Ordering::Relaxed);

                        let runtime = tokio::runtime::Handle::current();
                        let is_finished = interval_task.build_future(runtime).await;
                        let task_count = TASK_COUNTER.fetch_sub(1, Ordering::Relaxed) - 1;
                        if task_count == 0 && TERMINATED.load(Ordering::Relaxed) {
                            std::process::exit(0);
                        }

                        if is_finished {
                            break;
                        }
                        tokio::time::sleep(delay.into()).await;
                    }

                    debug!(
                        PLUGIN_LOGGING,
                        "Ended repeating task for sensor: {}", repeat_agent.sensor_id
                    );
                    let mut handle = repeat_agent.repeat_task_handle.write();
                    *handle = None;
                },
                abort_registration,
            );
            tokio::spawn(repeating_future);
            *handle = Some(abort_handle);
        } else {
            warn!(
                APP_LOGGING,
                "Sensor {}, agent {} already has a repeating task!",
                agent.sensor_id,
                agent.agent_name,
            );
        }
    }
}
