use crate::logging::APP_LOGGING;
use crate::schema::*;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use dotenv::dotenv;
use std::env;
use std::string::String;

pub fn establish_connection() -> PgConnection {
    dotenv().ok();
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    PgConnection::establish(&database_url).expect(&format!("Error connecting to {}", database_url))
}

pub fn get_sensors(conn: &PgConnection) -> Vec<SensorDao> {
    use crate::schema::sensors::dsl::*;
    if let Ok(result) = sensors.load(conn) {
        result
    } else {
        Vec::new()
    }
}

pub fn create_new_sensor(
    conn: &PgConnection,
    name_opt: &Option<String>,
    config: &Vec<NewSensorAgentConfig>,
) -> Option<SensorDao> {
    let name = name_opt.clone().unwrap_or_else(|| {
        use crate::schema::sensors::dsl::*;
        let count = sensors.count().get_result::<i64>(conn).unwrap_or(0) + 1;

        format!("Sensor {}", count)
    });

    let new_sensor = NewSensor { name: name };
    let sensor_result: QueryResult<SensorDao> = diesel::insert_into(sensors::table)
        .values(&new_sensor)
        .get_result(conn);
    if let Ok(sensor) = sensor_result {
        Some(sensor)
    } else {
        None
    }
}

pub fn delete_sensor(conn: &PgConnection, remove_id: i32) -> Result<(), String> {
    use crate::schema::sensor_agent_config::dsl::*;
    use crate::schema::sensors::dsl::*;
    if let Err(_) =
        diesel::delete(sensor_agent_config.filter(sensor_id.eq(sensor_id))).execute(conn)
    {
        return Err(format!(
            "Fail removing sensor actions for id: {}",
            remove_id
        ));
    }
    info!(APP_LOGGING, "Removed sensor actions for id: {}", remove_id);

    if let Ok(_) = diesel::delete(sensors.filter(id.eq(remove_id))).execute(conn) {
        info!(APP_LOGGING, "Removed sensor for id: {}", remove_id);
        Ok(())
    } else {
        Err(format!("Failed removing sensor for id: {}", remove_id))
    }
}

#[derive(Insertable)]
#[table_name = "sensors"]
struct NewSensor {
    pub name: String,
}

#[derive(Identifiable, Queryable, Debug)]
#[table_name = "sensors"]
pub struct SensorDao {
    pub id: i32,
    pub name: String,
}

#[derive(Insertable, std::fmt::Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[table_name = "sensor_agent_config"]
pub struct NewSensorAgentConfig {
    pub action: String,
    pub agent_impl: String,
    pub config_json: String,
}

#[derive(Queryable, Associations)]
#[belongs_to(parent = SensorDao)]
#[table_name = "sensor_agent_config"]
pub struct SensorAgentConfigDao {
    sensor_id: i32,
    pub action: String,
    pub agent_impl: String,
    pub config_json: String,
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_db_connection() {
        establish_connection();
    }

    #[test]
    fn test_insert_remove_sensor() {
        let conn = establish_connection();
        let sensor = create_new_sensor(&conn, &None, &vec![]);
        assert!(sensor.is_some(), true);

        let deleted = delete_sensor(&conn, sensor.unwrap().id);
        assert!(deleted.is_ok(), true);
    }

    #[test]
    fn test_insert_get_delete_sensor() {
        let conn = establish_connection();
        let sensor = create_new_sensor(&conn, &None, &vec![]);
        assert!(sensor.is_some(), true);

        assert_ne!(get_sensors(&conn).is_empty(), true);

        let deleted = delete_sensor(&conn, sensor.unwrap().id);
        assert!(deleted.is_ok(), true);
    }
}
