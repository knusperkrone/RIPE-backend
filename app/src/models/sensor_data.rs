use chrono::{DateTime, NaiveDateTime, Utc};
use ripe_core::SensorDataMessage;

use crate::error::DBError;

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

impl From<SensorDataDao> for SensorDataMessage {
    fn from(val: SensorDataDao) -> Self {
        SensorDataMessage {
            timestamp: DateTime::<Utc>::from_naive_utc_and_offset(val.timestamp, Utc),
            battery: val.battery,
            moisture: val.moisture,
            temperature: val.temperature,
            carbon: val.carbon,
            conductivity: val.conductivity,
            light: val.light,
        }
    }
}

pub async fn insert(
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

pub async fn get_latest(
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
pub async fn get_latest_unchecked(
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

pub async fn get(
    conn: &sqlx::PgPool,
    sensor_id: i32,
    from: chrono::DateTime<chrono::Utc>,
    until: chrono::DateTime<chrono::Utc>,
) -> Result<Vec<SensorDataDao>, DBError> {
    Ok(sql_stmnt!(
        SensorDataDao,
        r#"SELECT timestamp, battery, moisture, temperature, carbon, conductivity, light   
            FROM sensor_data 
            WHERE sensor_id = $1 
            AND timestamp >= $2 and timestamp < $3
            ORDER BY timestamp ASC"#,
        sensor_id,
        from.naive_utc(),
        until.naive_utc()
    )
    .fetch_all(conn)
    .await?)
}

// READ sensor_data
pub async fn get_first(conn: &sqlx::PgPool, sensor_id: i32) -> Result<SensorDataDao, DBError> {
    Ok(sql_stmnt!(
        SensorDataDao,
        r#"SELECT timestamp, battery, moisture, temperature, carbon, conductivity, light   
            FROM sensor_data 
            WHERE sensor_id = $1 
            ORDER BY timestamp ASC
            LIMIT 1"#,
        sensor_id
    )
    .fetch_one(conn)
    .await?)
}
