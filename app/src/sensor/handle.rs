use crate::logging::APP_LOGGING;
use crate::models::dao::{AgentConfigDao, SensorDao};
use crate::plugin::Agent;
use crate::{
    error::{DBError, ObserverError},
    plugin::AgentFactory,
};
use ripe_core::{error::AgentError, AgentConfigType, SensorDataMessage};
use std::{collections::HashMap, vec::Vec};

#[derive(Debug)]
pub struct SensorMQTTCommand {
    pub sensor_id: i32,
    pub domain: String,
    pub payload: i32,
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
        let mut agents: Vec<Agent> = actions
            .into_iter()
            .map(|config| {
                factory.create_agent(
                    sensor.id(),
                    config.agent_impl(),
                    config.domain(),
                    Some(config.state_json()),
                )
            })
            .filter_map(Result::ok)
            .collect();
        agents.sort_by(|a, b| a.domain().cmp(b.domain()));

        debug!(
            APP_LOGGING,
            "Loaded sensor \"{}\" with {} agents",
            sensor.name(),
            agents.len()
        );

        Ok(SensorHandle {
            dao: sensor,
            agents,
            has_pending_update: false,
        })
    }

    pub fn update(&mut self, updated_libs: &Vec<String>, factory: &AgentFactory) {
        for updated_lib in updated_libs {
            if let Some(outdated) = self
                .agents
                .iter_mut()
                .find(|a| a.agent_name() == updated_lib)
            {
                // update new agent
                if let Ok(updated) = factory.create_agent(
                    self.dao.id(),
                    &updated_lib,
                    outdated.domain(),
                    Some(outdated.deserialize().state_json()),
                ) {
                    *outdated = updated;
                    debug!(
                        APP_LOGGING,
                        "Sensor {} updated: {}",
                        self.dao.id(),
                        updated_lib
                    );
                } else {
                    error!(
                        APP_LOGGING,
                        "Sensor {} failed updating: {}",
                        self.dao.id(),
                        updated_lib
                    );
                }
            }
        }
    }

    pub fn handle_data(&mut self, data: &SensorDataMessage) {
        self.agents.iter_mut().for_each(|a| a.handle_data(data))
    }

    pub fn format_cmds(&self) -> Vec<i32> {
        self.agents.iter().map(|a| a.cmd()).collect()
    }

    pub fn handle_agent_cmd(
        &mut self,
        domain: &String,
        payload: i64,
    ) -> Result<&Agent, ObserverError> {
        for agent in self.agents.iter_mut() {
            if agent.domain() == domain {
                agent.handle_cmd(payload);
                return Ok(agent);
            }
        }
        Err(DBError::SensorNotFound(self.dao.id()).into())
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

    pub fn agent_config(
        &self,
        domain: &String,
    ) -> Option<HashMap<String, (String, AgentConfigType)>> {
        for agent in self.agents.iter() {
            if agent.domain() == domain {
                return Some(agent.config());
            }
        }
        None
    }

    pub fn set_agent_config(
        &mut self,
        domain: &String,
        config: HashMap<String, AgentConfigType>,
    ) -> Result<&Agent, ObserverError> {
        for agent in self.agents.iter_mut() {
            if agent.domain() == domain {
                if !agent.set_config(&config) {
                    return Err(AgentError::InvalidConfig(
                        serde_json::to_string(&config).unwrap_or("serde_error".to_owned()),
                    )
                    .into());
                }
                return Ok(agent);
            }
        }
        Err(DBError::SensorNotFound(self.dao.id()).into())
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
