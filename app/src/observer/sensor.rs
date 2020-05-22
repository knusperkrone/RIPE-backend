use crate::agent::{plugin::AgentFactory, Agent};
use crate::logging::APP_LOGGING;
use crate::error::PluginError;
use crate::models::dao::{AgentConfigDao, SensorDao};
use iftem_core::{error::AgentError, SensorDataDto};
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
            "Sensor \"{}\" with agents: {:?}",
            sensor.name(),
            agents
        );

        Ok(SensorHandle {
            dao: sensor,
            agents,
        })
    }

    pub fn on_data(&mut self, data: &SensorDataDto) {
        self.agents.iter_mut().for_each(|a| a.on_data(data))
    }

    pub fn reload(&mut self, factory: &AgentFactory) -> Result<(), PluginError> {
        self.agents.iter_mut().for_each(|a| a.reload(factory).unwrap());
        Ok(())
    }

    pub fn id(&self) -> i32 {
        self.dao.id()
    }
}