use crate::error::DBError;
use crate::logging::APP_LOGGING;
use crate::schema::{agent_configs, sensors};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use dotenv::dotenv;
use plugins_core::AgentConfig;
use std::env;
use std::fmt::Debug;
use std::string::String;

pub fn establish_db_connection() -> PgConnection {
    dotenv().ok();
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    PgConnection::establish(&database_url).expect(&format!("Error connecting to {}", database_url))
}

pub fn get_sensors(conn: &PgConnection) -> Vec<SensorDao> {
    use crate::schema::sensors::dsl::*;
    match sensors.load(conn) {
        Ok(result) => result,
        Err(msg) => {
            error!(APP_LOGGING, "Coulnd't find sensors: {}", msg);
            Vec::new()
        }
    }
}

pub fn get_agent_config(conn: &PgConnection, sensor_dao: &SensorDao) -> Vec<AgentConfigDao> {
    use crate::schema::agent_configs::dsl::*;
    match agent_configs
        .filter(sensor_id.eq(sensor_dao.id))
        .load::<AgentConfigDao>(conn)
    {
        Ok(result) => result,
        Err(msg) => {
            error!(APP_LOGGING, "Coulnd't find sensors: {}", msg);
            Vec::new()
        }
    }
}

pub fn create_new_sensor(
    conn: &PgConnection,
    name_opt: &Option<String>,
    mut configs: Vec<NewAgentConfig>,
) -> Result<(SensorDao, Vec<AgentConfigDao>), DBError> {
    let name = name_opt.clone().unwrap_or_else(|| {
        use crate::schema::sensors::dsl::*;
        let count = sensors.count().get_result::<i64>(conn).unwrap_or(0) + 1;
        // Not thread safe - but also not critical
        format!("Sensor {}", count)
    });

    let new_sensor = NewSensor { name };
    let sensor_dao: SensorDao = diesel::insert_into(sensors::table)
        .values(&new_sensor)
        .get_result(conn)?;
    let config_daos: Vec<AgentConfigDao> = configs
        .drain(..)
        .map(|c| AgentConfigDao::from(sensor_dao.id, c))
        .collect();

    // Safe config
    let agent_daos = diesel::insert_into(agent_configs::table)
        .values(config_daos)
        .get_results::<AgentConfigDao>(conn)?;
    Ok((sensor_dao, agent_daos))
}

fn delete_sensor_config(conn: &PgConnection, remove_id: i32) -> Result<(), DBError> {
    use crate::schema::agent_configs::dsl::*;
    diesel::delete(agent_configs.filter(sensor_id.eq(sensor_id))).execute(conn)?;
    info!(APP_LOGGING, "Unpersisted sensor config: {}", remove_id);
    Ok(())
}

pub fn delete_sensor(conn: &PgConnection, remove_id: i32) -> Result<(), DBError> {
    use crate::schema::sensors::dsl::*;
    delete_sensor_config(conn, remove_id)?;
    let count = diesel::delete(sensors.filter(id.eq(remove_id))).execute(conn)?;
    if count == 0 {
        Err(DBError::SensorNotFound(remove_id))
    } else {
        info!(APP_LOGGING, "Unpersisted sensor: {}", remove_id);
        Ok(())
    }
}

#[derive(Insertable)]
#[table_name = "sensors"]
struct NewSensor {
    pub name: String,
}

#[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct NewAgentConfig {
    pub domain: String,
    pub agent_impl: String,
    pub state_json: String,
}

impl From<AgentConfig> for NewAgentConfig {
    fn from(other: AgentConfig) -> Self {
        NewAgentConfig {
            domain: other.domain,
            agent_impl: other.name,
            state_json: other.state_json,
        }
    }
}

#[derive(Identifiable, Queryable, PartialEq, Debug)]
#[table_name = "sensors"]
pub struct SensorDao {
    pub id: i32,
    pub name: String,
}

#[derive(Insertable, Queryable, PartialEq, Debug)]
#[table_name = "agent_configs"]
pub struct AgentConfigDao {
    pub sensor_id: i32,
    pub domain: String,
    pub agent_impl: String,
    pub state_json: String,
}

impl AgentConfigDao {
    fn from(sensor_id: i32, user_config: NewAgentConfig) -> Self {
        AgentConfigDao {
            sensor_id: sensor_id,
            domain: user_config.domain,
            agent_impl: user_config.agent_impl,
            state_json: user_config.state_json,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_db_connection() {
        establish_db_connection();
    }

    #[test]
    fn test_insert_remove_sensor() {
        let conn = establish_db_connection();
        let sensor = create_new_sensor(&conn, &None, vec![]);
        assert!(sensor.is_ok(), true);

        let deleted = delete_sensor(&conn, sensor.unwrap().0.id);
        assert!(deleted.is_ok(), true);
    }

    #[test]
    fn test_insert_get_delete_sensor() {
        let conn = establish_db_connection();
        let sensor = create_new_sensor(&conn, &None, vec![]);
        assert!(sensor.is_ok(), true);

        assert_ne!(get_sensors(&conn).is_empty(), true);

        let deleted = delete_sensor(&conn, sensor.unwrap().0.id);
        assert!(deleted.is_ok(), true);
    }
}
