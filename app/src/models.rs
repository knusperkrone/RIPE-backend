use crate::config::CONFIG;
use crate::error::DBError;
use crate::plugin::Agent;
use std::string::String;

pub mod dao {
    use chrono::{DateTime, NaiveDateTime, Utc};
    use ripe_core::SensorDataMessage;

    #[derive(sqlx::FromRow)]
    pub struct SensorDao {
        id: i32,
        key_b64: String,
        name: String,
    }

    impl SensorDao {
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
                sensor_id: sensor_id,
                domain: domain,
                agent_impl: agent_impl,
                state_json: state_json,
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

    #[derive(sqlx::FromRow)]
    pub struct SensorDataDao {
        //id: i32,
        //sensor_id: i32,
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

use dao::*;

pub async fn establish_db_connection() -> Option<sqlx::PgPool> {
    let database_url = CONFIG.database_url();
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .ok()
}

pub(crate) async fn check_schema(conn: &sqlx::PgPool) -> Result<(), DBError> {
    sqlx::query("SELECT * FROM sensors LIMIT 1")
        .execute(conn)
        .await?;
    Ok(())
}

/// CREATE sensors
pub async fn create_new_sensor(
    conn: &sqlx::PgPool,
    key_b64: String,
    name_opt: &Option<String>,
) -> Result<SensorDao, DBError> {
    let name: String;
    if let Some(opt_name) = name_opt {
        name = opt_name.clone();
    } else {
        // generate name from current sensor count
        let rows: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM sensors")
            .fetch_one(conn)
            .await?;
        name = format!("Sensor {}", &rows.0);
    }

    Ok(sqlx::query_as::<_, SensorDao>(
        "INSERT INTO sensors (key_b64, name) VALUES ($1, $2) RETURNING *",
    )
    .bind(key_b64)
    .bind(name)
    .fetch_one(conn)
    .await?)
}

/// READ sensors
pub async fn get_sensors(conn: &sqlx::PgPool) -> Result<Vec<SensorDao>, DBError> {
    Ok(sqlx::query_as::<_, SensorDao>("SELECT * FROM sensors")
        .fetch_all(conn)
        .await?)
}

/// DELETE agent_config/sensors
pub async fn delete_sensor(conn: &sqlx::PgPool, remove_id: i32) -> Result<(), DBError> {
    sqlx::query("DELETE FROM agent_configs WHERE sensor_id = $1")
        .bind(remove_id)
        .execute(conn)
        .await?;
    sqlx::query("DELETE FROM sensors WHERE id = $1")
        .bind(remove_id)
        .execute(conn)
        .await?;
    Ok(())
}

/// CREATE agent_configs
pub async fn create_agent_config(
    conn: &sqlx::PgPool,
    config: &AgentConfigDao,
) -> Result<(), DBError> {
    sqlx::query(
        r#"INSERT INTO agent_configs
                (sensor_id, domain, agent_impl, state_json)
                VALUES ($1, $2, $3, $4)"#,
    )
    .bind(config.sensor_id())
    .bind(config.domain())
    .bind(config.agent_impl())
    .bind(config.state_json())
    .execute(conn)
    .await?;
    Ok(())
}

// READ agent_config
pub async fn get_agent_config(
    conn: &sqlx::PgPool,
    sensor_dao: &SensorDao,
) -> Result<Vec<AgentConfigDao>, DBError> {
    Ok(sqlx::query_as::<_, AgentConfigDao>(
        "SELECT * FROM agent_configs WHERE sensor_id = $1 ORDER BY domain ASC",
    )
    .bind(sensor_dao.id())
    .fetch_all(conn)
    .await?)
}

// UPDATE agent_config
pub async fn update_agent_config(
    conn: &sqlx::PgPool,
    update: &AgentConfigDao,
) -> Result<(), DBError> {
    sqlx::query("UPDATE agent_configs SET state_json = $1 WHERE sensor_id = $2 AND domain = $3")
        .bind(update.state_json())
        .bind(update.sensor_id())
        .bind(update.domain())
        .execute(conn)
        .await?;
    Ok(())
}

/// DELETE agent_configs
pub async fn delete_sensor_agent(
    conn: &sqlx::PgPool,
    remove_id: i32,
    agent: Agent,
) -> Result<(), DBError> {
    sqlx::query(
        "DELETE FROM agent_configs WHERE sensor_id = $1 AND agent_impl = $2 AND domain = $3",
    )
    .bind(remove_id)
    .bind(agent.agent_name())
    .bind(agent.domain())
    .execute(conn)
    .await?;
    Ok(())
}

pub async fn insert_sensor_data(
    conn: &sqlx::PgPool,
    sensor_id: i32,
    dto: ripe_core::SensorDataMessage,
    overleap: chrono::Duration,
) -> Result<(), DBError> {
    let update_result = sqlx::query(r#"
        UPDATE sensor_data 
        SET battery = $3, moisture = $4, temperature = $5, carbon = $6, conductivity = $7, light = $8
        WHERE sensor_id = $1 AND timestamp > NOW() - $2::text::interval"#,
    )
    .bind(sensor_id)
    .bind(format!("'{} milliseconds'", overleap.num_milliseconds())) 
    .bind(dto.battery)
    .bind(dto.moisture)
    .bind(dto.temperature)
    .bind(dto.carbon)
    .bind(dto.conductivity)
    .bind(dto.light)
    .execute(conn)
    .await?;

    if update_result.rows_affected() == 0 {
        sqlx::query(
            r#"INSERT INTO sensor_data
                (sensor_id, timestamp, battery, moisture, temperature, carbon, conductivity, light)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"#,
        )
        .bind(sensor_id)
        .bind(dto.timestamp)
        .bind(dto.battery)
        .bind(dto.moisture)
        .bind(dto.temperature)
        .bind(dto.carbon)
        .bind(dto.conductivity)
        .bind(dto.light)
        .execute(conn)
        .await?;
    }
    Ok(())
}

pub async fn get_latest_sensor_data(
    conn: &sqlx::PgPool,
    search_sensor_id: i32,
    search_key_b64: &String,
) -> Result<Option<SensorDataDao>, DBError> {
    Ok(sqlx::query_as::<_, SensorDataDao>(
        r#"SELECT * FROM sensor_data 
                    JOIN sensors ON (sensor_data.sensor_id = sensors.id)
                WHERE sensors.id = $1 AND sensors.key_b64 = $2
                ORDER BY timestamp DESC LIMIT 1"#,
    )
    .bind(search_sensor_id)
    .bind(search_key_b64)
    .fetch_optional(conn)
    .await?)
}

pub async fn get_latest_sensor_data_unchecked(
    conn: &sqlx::PgPool,
    search_sensor_id: i32,
) -> Result<Option<SensorDataDao>, DBError> {
    Ok(sqlx::query_as::<_, SensorDataDao>(
        r#"SELECT * FROM sensor_data 
                    JOIN sensors ON (sensor_data.sensor_id = sensors.id)
                WHERE sensors.id = $1"#,
    )
    .bind(search_sensor_id)
    .fetch_optional(conn)
    .await?)
}

#[cfg(test)]
mod test {
    use chrono::Utc;
    use ripe_core::SensorDataMessage;

    use super::*;

    #[tokio::test]
    async fn test_db_connection() {
        establish_db_connection().await;
    }

    #[tokio::test]
    async fn crud_sensors() {
        let conn = establish_db_connection().await.unwrap();

        // create
        let sensor = create_new_sensor(&conn, "123456".to_owned(), &None)
            .await
            .unwrap();

        // read
        assert_ne!(get_sensors(&conn).await.unwrap().is_empty(), true);

        // delete
        delete_sensor(&conn, sensor.id()).await.unwrap();
    }

    #[tokio::test]
    async fn crud_agent_configs() {
        let conn = establish_db_connection().await.unwrap();
        let sensor = create_new_sensor(&conn, "123456".to_owned(), &None)
            .await
            .unwrap();
        let mut dao = AgentConfigDao {
            sensor_id: sensor.id(),
            state_json: "{}".to_owned(),
            domain: "test_domain".to_owned(),
            agent_impl: "agent_impl".to_owned(),
        };

        // create
        create_agent_config(&conn, &dao).await.unwrap();

        // read
        let mut actual_config = get_agent_config(&conn, &sensor)
            .await
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(dao.sensor_id(), actual_config.sensor_id());
        assert_eq!(dao.domain(), actual_config.domain());
        assert_eq!(dao.state_json(), actual_config.state_json());
        assert_eq!(dao.agent_impl(), actual_config.agent_impl());

        // update
        dao.state_json = "{\"key\": true}".to_owned();
        update_agent_config(&conn, &dao).await.unwrap();
        actual_config = get_agent_config(&conn, &sensor)
            .await
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(dao.sensor_id(), actual_config.sensor_id());
        assert_eq!(dao.domain(), actual_config.domain());
        assert_eq!(dao.state_json(), actual_config.state_json());
        assert_eq!(dao.agent_impl(), actual_config.agent_impl());

        // delete
        delete_sensor(&conn, sensor.id()).await.unwrap();
    }

    #[tokio::test]
    async fn crud_sensor_data() {
        let conn = establish_db_connection().await.unwrap();
        let sensor = create_new_sensor(&conn, "123456".to_owned(), &None)
            .await
            .unwrap();

        // create
        // insert
        insert_sensor_data(
            &conn,
            sensor.id(),
            SensorDataMessage {
                timestamp: Utc::now() - chrono::Duration::hours(2),
                ..Default::default()
            },
            chrono::Duration::minutes(5),
        )
        .await
        .unwrap();

        let rows: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM sensor_data WHERE sensor_id = $1")
            .bind(sensor.id())
            .fetch_one(&conn)
            .await
            .unwrap();
        assert_eq!(1, rows.0);

        for _ in 0..2 {
            // insert||update
            insert_sensor_data(
                &conn,
                sensor.id(),
                SensorDataMessage {
                    ..Default::default()
                },
                chrono::Duration::minutes(5),
            )
            .await
            .unwrap();

            let rows: (i64,) =
                sqlx::query_as("SELECT COUNT(*) FROM sensor_data WHERE sensor_id = $1")
                    .bind(sensor.id())
                    .fetch_one(&conn)
                    .await
                    .unwrap();
            assert_eq!(2, rows.0);
        }

        // read
        get_latest_sensor_data(&conn, sensor.id(), sensor.key_b64())
            .await
            .unwrap()
            .unwrap();
        get_latest_sensor_data_unchecked(&conn, sensor.id())
            .await
            .unwrap()
            .unwrap();
    }
}
