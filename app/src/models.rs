use crate::error::DBError;
use crate::logging::APP_LOGGING;
use crate::schema::*;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use dotenv::dotenv;
use iftem_core::SensorDataMessage;
use std::env;
use std::fmt::Debug;
use std::string::String;

pub mod dao {
    use super::*;
    use chrono::{DateTime, NaiveDateTime, Utc};

    #[derive(Insertable)]
    #[table_name = "sensors"]
    pub(super) struct NewSensor {
        pub name: String,
        pub key_b64: String,
    }

    #[derive(Identifiable, Queryable, PartialEq, Debug)]
    #[table_name = "sensors"]
    pub struct SensorDao {
        id: i32,
        key_b64: String,
        name: String,
    }

    impl SensorDao {
        pub fn new(id: i32, key_b64: String, name: String) -> Self {
            SensorDao { id, key_b64, name }
        }

        pub fn id(&self) -> i32 {
            self.id
        }

        pub fn key_b64(&self) -> &String {
            &self.key_b64
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

    #[derive(Insertable)]
    #[table_name = "sensor_data"]
    pub struct NewSensorData {
        sensor_id: i32,
        timestamp: NaiveDateTime,
        battery: Option<f64>,
        moisture: Option<f64>,
        temperature: Option<f64>,
        carbon: Option<i32>,
        conductivity: Option<i32>,
        light: Option<i32>,
    }

    impl NewSensorData {
        pub fn new(sensor_id: i32, other: iftem_core::SensorDataMessage) -> Self {
            NewSensorData {
                sensor_id: sensor_id,
                timestamp: other.timestamp.naive_utc(),
                battery: other.battery,
                moisture: other.moisture,
                temperature: other.temperature,
                carbon: other.carbon,
                conductivity: other.conductivity,
                light: other.light,
            }
        }
    }

    #[derive(Identifiable, Queryable, PartialEq, Debug)]
    #[table_name = "sensor_data"]
    pub struct SensorDataDao {
        id: i32,
        sensor_id: i32,
        timestamp: NaiveDateTime,
        battery: Option<f64>,
        moisture: Option<f64>,
        temperature: Option<f64>,
        carbon: Option<i32>,
        conductivity: Option<i32>,
        light: Option<i32>,
    }

    impl Into<SensorDataMessage> for SensorDataDao {
        fn into(self) -> SensorDataMessage {
            SensorDataMessage {
                timestamp: DateTime::<Utc>::from_utc(self.timestamp, Utc),
                battery: self.battery,
                moisture: self.moisture,
                temperature: self.temperature,
                carbon: self.carbon,
                conductivity: self.conductivity,
                light: self.light,
            }
        }
    }
}

pub mod dto {
    use iftem_core::{AgentMessage, AgentState, AgentUI, SensorDataMessage};
    use serde::{Deserialize, Serialize, Serializer};

    #[derive(Debug, Copy, Clone)]
    pub enum AgentPayload {
        State(AgentState),
        Bool(bool),
        Int(i32),
    }

    impl AgentPayload {
        pub fn from(message: AgentMessage) -> Result<Self, ()> {
            match message {
                AgentMessage::State(s) => Ok(AgentPayload::State(s)),
                AgentMessage::Bool(b) => Ok(AgentPayload::Bool(b)),
                AgentMessage::Int(i) => Ok(AgentPayload::Int(i)),
                AgentMessage::Task(_) => Err(()),
            }
        }
    }

    impl Serialize for AgentPayload {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            match self {
                AgentPayload::Bool(b) => serializer.serialize_bool(*b),
                AgentPayload::Int(i) => serializer.serialize_i32(*i),
                AgentPayload::State(_) => serializer.serialize_none(),
            }
        }
    }

    #[derive(Serialize, Deserialize, Debug)]
    pub struct AgentRegisterDto {
        pub domain: String,
        pub agent_name: String,
    }

    #[derive(Serialize, Deserialize, Debug)]
    pub struct AgentStatusDto {
        pub domain: String,
        pub agent_name: String,
        pub ui: AgentUI,
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

    #[derive(Debug, Serialize)]
    pub struct SensorMessageDto {
        #[serde(skip_serializing)]
        pub sensor_id: i32,
        pub domain: String,
        pub payload: AgentPayload,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct SensorStatusDto {
        pub name: String,
        pub data: SensorDataMessage,
        pub agents: Vec<AgentStatusDto>,
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
    key_b64: String,
    name_opt: &Option<String>,
) -> Result<SensorDao, DBError> {
    let name = name_opt.clone().unwrap_or_else(|| {
        use crate::schema::sensors::dsl::*;
        let count = sensors.count().get_result::<i64>(conn).unwrap_or(0) + 1;
        // Not thread safe - but also not critical
        format!("Sensor {}", count)
    });

    let new_sensor = NewSensor { name, key_b64 };
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

pub fn insert_sensor_data(
    conn: &PgConnection,
    sensor_id: i32,
    dto: iftem_core::SensorDataMessage,
) -> Result<(), DBError> {
    let insert = NewSensorData::new(sensor_id, dto);
    diesel::insert_into(sensor_data::table)
        .values(insert)
        .execute(conn)?;
    Ok(())
}

pub fn get_latest_sensor_data(
    conn: &PgConnection,
    search_sensor_id: i32,
    search_key_b64: &String,
) -> Result<Option<SensorDataDao>, DBError> {
    use crate::schema::sensor_data::dsl::*;
    use crate::schema::sensors::dsl::key_b64 as dsl_key_b64;
    let mut result: Vec<(SensorDataDao, SensorDao)> = sensor_data
        .inner_join(sensors::table)
        .filter(sensor_id.eq(search_sensor_id))
        .filter(dsl_key_b64.eq(search_key_b64))
        .order(timestamp.desc())
        .limit(1)
        .load(conn)?;

    if let Some((data, _)) = result.pop() {
        Ok(Some(data.into()))
    } else {
        Ok(None)
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
        let sensor = create_new_sensor(&conn, "123456".to_owned(), &None);
        assert!(sensor.is_ok(), true);

        let deleted = delete_sensor(&conn, sensor.unwrap().id());
        assert!(deleted.is_ok(), true);
    }

    #[test]
    fn test_insert_get_delete_sensor() {
        let conn = establish_db_connection();
        let sensor = create_new_sensor(&conn, "123456".to_owned(), &None);
        assert!(sensor.is_ok(), true);

        assert_ne!(get_sensors(&conn).is_empty(), true);

        let deleted = delete_sensor(&conn, sensor.unwrap().id());
        assert!(deleted.is_ok(), true);
    }
}
