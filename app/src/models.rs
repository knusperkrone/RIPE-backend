use crate::error::DBError;
use crate::logging::APP_LOGGING;
use crate::schema::{agent_configs, sensors};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use dotenv::dotenv;
use std::env;
use std::fmt::Debug;
use std::string::String;

pub mod dao {
    use super::*;

    #[derive(Insertable)]
    #[table_name = "sensors"]
    pub(super) struct NewSensor {
        pub name: String,
    }

    #[derive(Identifiable, Queryable, PartialEq, Debug)]
    #[table_name = "sensors"]
    pub struct SensorDao {
        id: i32,
        name: String,
    }

    impl SensorDao {
        pub fn new(id: i32, name: String) -> Self {
            SensorDao { id: id, name: name }
        }

        pub fn id(&self) -> i32 {
            self.id
        }

        pub fn name(&self) -> &String {
            &self.name
        }
    }

    #[derive(Insertable, Queryable, PartialEq, Debug)]
    #[table_name = "agent_configs"]
    pub struct AgentConfigDao {
        sensor_id: i32,
        domain: String,
        agent_impl: String,
        state_json: String,
    }

    impl AgentConfigDao {
        pub fn new(sensor_id: i32, domain: String, agent_impl: String, state_json: String) -> Self {
            AgentConfigDao {
                sensor_id: sensor_id,
                domain: domain,
                agent_impl: agent_impl,
                state_json: state_json,
            }
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
}

pub mod dto {
    use plugins_core::AgentPayload;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Debug)]
    pub struct AgentRegisterDto {
        pub domain: String,
        pub agent_name: String,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct RegisterRequestDto {
        pub agents: Vec<AgentRegisterDto>,
        pub name: Option<String>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct RegisterResponseDto {
        pub id: i32,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct UnregisterRequestDto {
        pub id: i32,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct UnregisterResponseDto {
        pub id: i32,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct ErrorResponseDto {
        pub error: String,
    }

    #[derive(Serialize)]
    pub struct SensorMessageDto {
        #[serde(skip_serializing)]
        pub sensor_id: i32,
        pub domain: String,
        pub payload: AgentPayload,
    }
}

use dao::*;

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
        .filter(sensor_id.eq(sensor_dao.id()))
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
) -> Result<SensorDao, DBError> {
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
    Ok(sensor_dao)
}

pub fn create_sensor_agents(
    conn: &PgConnection,
    configs: Vec<AgentConfigDao>,
) -> Result<Vec<AgentConfigDao>, DBError> {
    let config_daos: Vec<AgentConfigDao> = diesel::insert_into(agent_configs::table)
        .values(configs)
        .get_results(conn)?;
    Ok(config_daos)
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
        let sensor = create_new_sensor(&conn, &None);
        assert!(sensor.is_ok(), true);

        let deleted = delete_sensor(&conn, sensor.unwrap().id());
        assert!(deleted.is_ok(), true);
    }

    #[test]
    fn test_insert_get_delete_sensor() {
        let conn = establish_db_connection();
        let sensor = create_new_sensor(&conn, &None);
        assert!(sensor.is_ok(), true);

        assert_ne!(get_sensors(&conn).is_empty(), true);

        let deleted = delete_sensor(&conn, sensor.unwrap().id());
        assert!(deleted.is_ok(), true);
    }
}
