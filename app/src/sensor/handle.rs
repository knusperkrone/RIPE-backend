use crate::error::PluginError;
use crate::logging::APP_LOGGING;
use crate::models::dao::{AgentConfigDao, SensorDao};
use crate::plugin::agent::{Agent, AgentFactory};
use iftem_core::{error::AgentError, SensorDataMessage};
use std::vec::Vec;

#[derive(Debug)]
pub struct SensorHandleMessage {
    pub sensor_id: i32,
    pub domain: String,
    pub payload: u32,
}

pub struct SensorHandle {
    pub dao: SensorDao,
    pub agents: Vec<Agent>,
    pub has_pending_update: bool,
}

impl SensorHandle {
    pub fn from(
        sensor: SensorDao,
        actions: &Vec<AgentConfigDao>,
        factory: &AgentFactory,
    ) -> Result<SensorHandle, AgentError> {
        // TODO: Filter invalid!
        let mut agents: Vec<Agent> = actions
            .into_iter()
            .map(|config| factory.restore_agent(sensor.id(), config))
            .filter_map(Result::ok)
            .collect();
        agents.sort_by(|a, b| a.domain().cmp(b.domain()));

        debug!(
            APP_LOGGING,
            "Sensor \"{}\" with {} agents",
            sensor.name(),
            agents.len()
        );

        Ok(SensorHandle {
            dao: sensor,
            agents,
            has_pending_update: false,
        })
    }

    pub fn on_data(&mut self, data: &SensorDataMessage) {
        self.agents.iter_mut().for_each(|a| a.on_data(data))
    }

    pub fn format_cmds(&self) -> Vec<i32> {
        self.agents.iter().map(|a| a.cmd()).collect()
    }

    pub fn reload(&mut self, factory: &AgentFactory) -> Result<(), PluginError> {
        for agent in self.agents.iter_mut() {
            if agent.reload_agent(factory).is_err() {
                agent.set_needs_update(true);
                self.has_pending_update = true;
            }
        }
        Ok(())
    }

    pub fn reload_agents(&mut self, loaded_libs: &Vec<String>, factory: &AgentFactory) {
        for agent in self.agents.iter_mut() {
            if loaded_libs.contains(agent.agent_name()) {
                if agent.reload_agent(factory).is_err() {
                    agent.set_needs_update(true);
                    self.has_pending_update = true;
                }
            }
        }
    }

    pub fn reload_pending_agents(&mut self, factory: &AgentFactory) {
        if !self.has_pending_update {
            return;
        }
        
        self.has_pending_update = false;
        for agent in self.agents.iter_mut() {
            if agent.needs_update() {
                if agent.reload_agent(factory).is_err() {
                    self.has_pending_update = true;
                }
            }
        }
    }

    pub fn add_agent(&mut self, agent: Agent) {
        if 0 == self
            .agents
            .iter_mut()
            .filter(|a| a.agent_name() == agent.agent_name() && a.domain() == agent.domain())
            .count()
        {
            self.agents.push(agent);
            self.agents.sort_by(|a, b| a.domain().cmp(b.domain()));
        }
    }

    pub fn remove_agent(&mut self, name: &String, domain: &String) -> Option<Agent> {
        for i in 0..self.agents.len() {
            let curr = &self.agents[i];
            if curr.agent_name() == name && curr.domain() == domain {
                return Some(self.agents.remove(i));
            }
        }
        None
    }

    pub fn agents(&self) -> &Vec<Agent> {
        &self.agents
    }

    pub fn id(&self) -> i32 {
        self.dao.id()
    }

    pub fn name(&self) -> &String {
        &self.dao.name()
    }

    pub fn key_b64(&self) -> &String {
        &self.dao.key_b64()
    }
}
