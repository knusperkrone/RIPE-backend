use super::sensor;
use crate::error::DBError;
use crate::plugin::Agent;

#[derive(sqlx::FromRow)]
pub struct AgentConfigDao {
    pub(crate) sensor_id: i32,
    pub(crate) domain: String,
    pub(crate) agent_impl: String,
    pub(crate) state_json: String,
}

impl AgentConfigDao {
    pub fn new(sensor_id: i32, domain: String, agent_impl: String, state_json: String) -> Self {
        AgentConfigDao {
            sensor_id,
            domain,
            agent_impl,
            state_json,
        }
    }

    pub fn sensor_id(&self) -> i32 {
        self.sensor_id
    }

    pub fn domain(&self) -> &String {
        &self.domain
    }

    pub fn agent_impl(&self) -> &String {
        &self.agent_impl
    }

    pub fn state_json(&self) -> &String {
        &self.state_json
    }
}

pub async fn insert(conn: &sqlx::PgPool, config: &AgentConfigDao) -> Result<(), DBError> {
    sql_stmnt!(
        r#"INSERT INTO agent_configs
                (sensor_id, domain, agent_impl, state_json)
                VALUES ($1, $2, $3, $4)"#,
        config.sensor_id(),
        config.domain(),
        config.agent_impl(),
        config.state_json()
    )
    .execute(conn)
    .await?;
    Ok(())
}

// READ agent_config
pub async fn get(
    conn: &sqlx::PgPool,
    sensor_dao: &sensor::SensorDao,
) -> Result<Vec<AgentConfigDao>, DBError> {
    Ok(sql_stmnt!(
        AgentConfigDao,
        "SELECT * FROM agent_configs WHERE sensor_id = $1 ORDER BY domain ASC",
        sensor_dao.id()
    )
    .fetch_all(conn)
    .await?)
}

// UPDATE agent_config
pub async fn update(conn: &sqlx::PgPool, update: &AgentConfigDao) -> Result<(), DBError> {
    sql_stmnt!(
        "UPDATE agent_configs SET state_json = $1 WHERE sensor_id = $2 AND domain = $3",
        update.state_json(),
        update.sensor_id(),
        update.domain()
    )
    .execute(conn)
    .await?;
    Ok(())
}

/// DELETE agent_configs
pub async fn delete(conn: &sqlx::PgPool, remove_id: i32, agent: Agent) -> Result<(), DBError> {
    sql_stmnt!(
        "DELETE FROM agent_configs WHERE sensor_id = $1 AND agent_impl = $2 AND domain = $3",
        remove_id,
        agent.agent_name(),
        agent.domain()
    )
    .execute(conn)
    .await?;
    Ok(())
}
