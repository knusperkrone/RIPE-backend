use crate::error::PluginError;
use crate::logging::APP_LOGGING;
use crate::models::dao::{AgentConfigDao, SensorDao};
use crate::plugin::agent::{Agent, AgentFactory};
use iftem_core::{error::AgentError, SensorDataMessage};
use std::vec::Vec;

pub struct SensorHandle {
    pub dao: SensorDao,
    pub agents: Vec<Agent>,
}

impl SensorHandle {
    pub fn from(
        sensor: SensorDao,
        actions: &Vec<AgentConfigDao>,
        factory: &AgentFactory,
    ) -> Result<SensorHandle, AgentError> {
        // TODO: Filter invalid!
        let agents: Vec<Agent> = actions
            .into_iter()
            .map(|config| factory.restore_agent(sensor.id(), config))
            .filter_map(Result::ok)
            .collect();
        debug!(
            APP_LOGGING,
            "Sensor \"{}\" with {} agents",
            sensor.name(),
            agents.len()
        );

        Ok(SensorHandle {
            dao: sensor,
            agents,
        })
    }

    pub fn on_data(&mut self, data: &SensorDataMessage) {
        self.agents.iter_mut().for_each(|a| a.on_data(data))
    }

    pub fn reload(&mut self, factory: &AgentFactory) -> Result<(), PluginError> {
        self.agents
            .iter_mut()
            .for_each(|a| a.reload_agent(factory).unwrap());
        Ok(())
    }

    pub fn add_agent(&mut self, agent: Agent) {
        if 0 == self
            .agents
            .iter_mut()
            .filter(|a| a.agent_name() == agent.agent_name() && a.domain() == agent.domain())
            .count()
        {
            self.agents.push(agent);
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
