use chrono::NaiveDateTime;

use crate::{error::DBError, models::CountRecord};

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

pub async fn upsert(
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
pub async fn get(conn: &sqlx::PgPool, sensor_id: i32) -> Result<Vec<SensorLogDao>, DBError> {
    Ok(sql_stmnt!(
        SensorLogDao,
        "SELECT * FROM sensor_log WHERE sensor_id = $1 ORDER BY time ASC",
        sensor_id
    )
    .fetch_all(conn)
    .await?)
}
