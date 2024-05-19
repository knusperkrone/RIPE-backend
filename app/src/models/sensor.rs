use super::CountRecord;
use crate::error::DBError;

#[derive(sqlx::FromRow, Debug)]
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

pub async fn insert(
    conn: &sqlx::PgPool,
    key_b64: &String,
    name_opt: Option<String>,
) -> Result<SensorDao, DBError> {
    let name: String;
    if let Some(opt_name) = name_opt {
        name = opt_name;
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
pub async fn read(conn: &sqlx::PgPool) -> Result<Vec<SensorDao>, DBError> {
    Ok(sql_stmnt!(SensorDao, "SELECT * FROM sensors")
        .fetch_all(conn)
        .await?)
}

pub async fn exists_with_key(conn: &sqlx::PgPool, sensor_id: i32, key: &String) -> bool {
    let count: CountRecord = sql_stmnt!(
        CountRecord,
        "SELECT count(*) FROM sensors WHERE id = $1 AND key_b64 = $2",
        sensor_id,
        key
    )
    .fetch_one(conn)
    .await
    .unwrap_or(CountRecord { count: Some(0) });

    count.count.unwrap_or(0) == 1
}

pub async fn delete(conn: &sqlx::PgPool, remove_id: i32) -> Result<(), DBError> {
    sql_stmnt!("DELETE FROM agent_configs WHERE sensor_id = $1", remove_id)
        .execute(conn)
        .await?;
    sql_stmnt!("DELETE FROM sensors WHERE id = $1", remove_id)
        .execute(conn)
        .await?;
    Ok(())
}
