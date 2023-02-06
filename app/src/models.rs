use crate::config::CONFIG;
use crate::error::DBError;
use crate::plugin::Agent;
use std::string::String;

#[cfg(debug_assertions)]
macro_rules! sql_stmnt {
    ($ret:ident, $stmt:expr) => {
        sqlx::query_as!($ret, $stmt)
    };
    ($stmt:expr) => {
        sqlx::query!($stmt)
    };
    ($ret:ident, $stmt:expr, $($bind:expr),*) => {
        sqlx::query_as!($ret, $stmt,$($bind,)*)
    };
    ($stmt:expr, $($bind:expr),*) => {
        sqlx::query!($stmt, $($bind,)*)
    };
}

#[cfg(not(debug_assertions))]
macro_rules! sql_stmnt {
    ($ret:ident, $stmt:expr) => {
        sqlx::query_as::<_ ,$ret>($stmt)
    };
    ($stmt:expr) => {
        sqlx::query($stmt)
    };
    ($ret:ident, $stmt:expr, $($bind:expr),*) => {
        sqlx::query_as::<_ ,$ret>($stmt)$(.bind($bind))*
    };
    ($stmt:expr, $($bind:expr),*) => {
        sqlx::query($stmt)$(.bind($bind))*
    };
}

pub mod dao {
    use chrono::{DateTime, NaiveDateTime, Utc};
    use ripe_core::SensorDataMessage;

    #[derive(sqlx::FromRow)]
    pub(crate) struct CountRecord {
        pub count: Option<i64>,
    }

    impl CountRecord {
        pub fn count(self) -> i64 {
            self.count.unwrap_or(0)
        }
    }

    #[derive(sqlx::FromRow)]
    pub struct SensorDao {
        pub(crate) id: i32,
        pub(crate) key_b64: String,
        pub(crate) name: String,
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
        // pub(crate) id: i32,
        // pub(crate) sensor_id: i32,
        pub(crate) timestamp: NaiveDateTime,
        pub(crate) battery: Option<f64>,
        pub(crate) moisture: Option<f64>,
        pub(crate) temperature: Option<f64>,
        pub(crate) carbon: Option<i32>,
        pub(crate) conductivity: Option<i32>,
        pub(crate) light: Option<i32>,
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

    #[derive(sqlx::FromRow)]
    #[allow(dead_code)]
    pub struct SensorLogDao {
        pub(crate) id: i32,
        pub(crate) sensor_id: i32,
        pub(crate) time: NaiveDateTime,
        pub(crate) log: std::string::String,
    }

    impl SensorLogDao {
        pub fn time<T>(&self, tz: &T) -> chrono::DateTime<T>
        where
            T: chrono::TimeZone,
        {
            tz.from_utc_datetime(&self.time)
        }

        pub fn log(&self) -> &std::string::String {
            &self.log
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
    sql_stmnt!("SELECT count(*) as count FROM sensors")
        .fetch_one(conn)
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
        let rows = sql_stmnt!(CountRecord, "SELECT count(*) as count FROM sensors")
            .fetch_one(conn)
            .await?;
        name = format!("Sensor {}", &rows.count());
    }

    Ok(sql_stmnt!(
        SensorDao,
        "INSERT INTO sensors (key_b64, name) VALUES ($1, $2) RETURNING *",
        key_b64,
        name
    )
    .fetch_one(conn)
    .await?)
}

/// READ sensors
pub async fn get_sensors(conn: &sqlx::PgPool) -> Result<Vec<SensorDao>, DBError> {
    Ok(sql_stmnt!(SensorDao, "SELECT * FROM sensors")
        .fetch_all(conn)
        .await?)
}

/// DELETE agent_config/sensors
pub async fn delete_sensor(conn: &sqlx::PgPool, remove_id: i32) -> Result<(), DBError> {
    sql_stmnt!("DELETE FROM agent_configs WHERE sensor_id = $1", remove_id)
        .execute(conn)
        .await?;
    sql_stmnt!("DELETE FROM sensors WHERE id = $1", remove_id)
        .execute(conn)
        .await?;
    Ok(())
}

/// CREATE agent_configs
pub async fn create_agent_config(
    conn: &sqlx::PgPool,
    config: &AgentConfigDao,
) -> Result<(), DBError> {
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
pub async fn get_agent_config(
    conn: &sqlx::PgPool,
    sensor_dao: &SensorDao,
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
pub async fn update_agent_config(
    conn: &sqlx::PgPool,
    update: &AgentConfigDao,
) -> Result<(), DBError> {
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
pub async fn delete_sensor_agent(
    conn: &sqlx::PgPool,
    remove_id: i32,
    agent: Agent,
) -> Result<(), DBError> {
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

// CREATE/UPDATE sensor_data
pub async fn insert_sensor_data(
    conn: &sqlx::PgPool,
    sensor_id: i32,
    dto: ripe_core::SensorDataMessage,
    overleap: chrono::Duration,
) -> Result<(), DBError> {
    let update_result = sql_stmnt!(
        r#"UPDATE sensor_data 
            SET battery = $3, moisture = $4, temperature = $5, carbon = $6, conductivity = $7, light = $8
            WHERE sensor_id = $1 AND timestamp > NOW() - $2::text::interval"#,
        sensor_id,
        format!("'{} milliseconds'", overleap.num_milliseconds()),
        dto.battery,
        dto.moisture,
        dto.temperature,
        dto.carbon,
        dto.conductivity,
        dto.light
    )
    .execute(conn)
    .await?;

    if update_result.rows_affected() == 0 {
        sql_stmnt!(
            r#"INSERT INTO sensor_data
                (sensor_id, timestamp, battery, moisture, temperature, carbon, conductivity, light)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"#,
            sensor_id,
            dto.timestamp.naive_utc(),
            dto.battery,
            dto.moisture,
            dto.temperature,
            dto.carbon,
            dto.conductivity,
            dto.light
        )
        .execute(conn)
        .await?;
    }
    Ok(())
}

// READ sensor_data
pub async fn get_latest_sensor_data(
    conn: &sqlx::PgPool,
    search_sensor_id: i32,
    search_key_b64: &String,
) -> Result<Option<SensorDataDao>, DBError> {
    Ok(sql_stmnt!(
        SensorDataDao,
        r#"SELECT sd.timestamp, sd.battery, sd.moisture, sd.temperature, sd.carbon, sd.conductivity, sd.light  
            FROM sensor_data as sd
            JOIN sensors ON (sd.sensor_id = sensors.id)
            WHERE sensors.id = $1 AND sensors.key_b64 = $2
            ORDER BY timestamp DESC LIMIT 1"#,
        search_sensor_id,
        search_key_b64
    )
    .fetch_optional(conn)
    .await?)
}

// READ sensor_data
pub async fn get_latest_sensor_data_unchecked(
    conn: &sqlx::PgPool,
    search_sensor_id: i32,
) -> Result<Option<SensorDataDao>, DBError> {
    Ok(sql_stmnt!(
        SensorDataDao,
        r#"SELECT sd.timestamp, sd.battery, sd.moisture, sd.temperature, sd.carbon, sd.conductivity, sd.light  
            FROM sensor_data as sd
            JOIN sensors ON (sd.sensor_id = sensors.id)
            WHERE sensors.id = $1"#,
        search_sensor_id
    )
    .fetch_optional(conn)
    .await?)
}

// CREATE/UPDATE sensor_log
pub async fn insert_sensor_log(
    conn: &sqlx::PgPool,
    sensor_id: i32,
    msg: std::string::String,
    max_count: i64,
) -> Result<(), DBError> {
    let now = chrono::Utc::now().naive_utc();
    let trimmed = &msg[..std::cmp::min(255, msg.len())];

    let log_count = sql_stmnt!(
        CountRecord,
        "SELECT count(*) FROM sensor_log WHERE sensor_id = $1",
        sensor_id
    )
    .fetch_one(conn)
    .await?;

    if log_count.count() >= max_count {
        sql_stmnt!(
            r#"UPDATE sensor_log SET time = $1, log = $2 
                WHERE id = (SELECT id FROM sensor_log WHERE sensor_id = $3 ORDER BY time ASC LIMIT 1)"#,
            now,
            trimmed,
            sensor_id
        )
        .execute(conn)
        .await?;
    } else {
        sql_stmnt!(
            "INSERT INTO sensor_log (sensor_id, time, log) VALUES ($1, $2, $3)",
            sensor_id,
            now,
            trimmed
        )
        .execute(conn)
        .await?;
    }
    Ok(())
}

// READ sensor_log
pub async fn get_sensor_logs(
    conn: &sqlx::PgPool,
    sensor_id: i32,
) -> Result<Vec<dao::SensorLogDao>, DBError> {
    Ok(sql_stmnt!(
        SensorLogDao,
        "SELECT * FROM sensor_log WHERE sensor_id = $1 ORDER BY time ASC",
        sensor_id
    )
    .fetch_all(conn)
    .await?)
}

// READ sensor_log
pub async fn get_sensor_data(
    conn: &sqlx::PgPool,
    sensor_id: i32,
    limit: i64,
    offset: i64,
) -> Result<Vec<dao::SensorDataDao>, DBError> {
    Ok(sql_stmnt!(
        SensorDataDao,
        r#"SELECT timestamp, battery, moisture, temperature, carbon, conductivity, light   
            FROM sensor_data WHERE sensor_id = $1 
            ORDER BY timestamp ASC 
            LIMIT $2 
            OFFSET $3"#,
        sensor_id,
        limit,
        offset
    )
    .fetch_all(conn)
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

        let row_count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM sensor_data WHERE sensor_id = $1")
                .bind(sensor.id())
                .fetch_one(&conn)
                .await
                .unwrap();
        assert_eq!(1, row_count.0);

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

            let row_count: (i64,) =
                sqlx::query_as("SELECT COUNT(*) FROM sensor_data WHERE sensor_id = $1")
                    .bind(sensor.id())
                    .fetch_one(&conn)
                    .await
                    .unwrap();
            assert_eq!(2, row_count.0);
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

    #[tokio::test]
    async fn crud_sensor_log() {
        let conn = establish_db_connection().await.unwrap();
        let sensor = create_new_sensor(&conn, "123456".to_owned(), &None)
            .await
            .unwrap();

        // create
        let max_count = 5;
        for i in 0..10 {
            insert_sensor_log(&conn, sensor.id(), format!("{}", i), max_count)
                .await
                .unwrap();
        }
        let row_count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM sensor_log WHERE sensor_id = $1")
                .bind(sensor.id())
                .fetch_one(&conn)
                .await
                .unwrap();
        assert_eq!(max_count, row_count.0);

        // get
        let mut daos = get_sensor_logs(&conn, sensor.id()).await.unwrap();
        let mut index = max_count;

        for dao in daos.drain(..) {
            assert_eq!(&format!("{}", index), dao.log());
            index += 1;
        }
    }
}
