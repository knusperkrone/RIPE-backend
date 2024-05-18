use crate::config::CONFIG;
use crate::error::DBError;

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

pub async fn establish_db_connection() -> Option<sqlx::PgPool> {
    let database_url = CONFIG.database_url();
    sqlx::postgres::PgPoolOptions::new()
        .connect(database_url)
        .await
        .ok()
}

pub async fn check_schema(conn: &sqlx::PgPool) -> Result<(), DBError> {
    sql_stmnt!("SELECT count(*) as count FROM sensors")
        .fetch_one(conn)
        .await?;
    Ok(())
}

#[derive(sqlx::FromRow)]
pub(crate) struct CountRecord {
    pub count: Option<i64>,
}

impl CountRecord {
    pub fn count(self) -> i64 {
        self.count.unwrap_or(0)
    }
}

pub mod agent;
pub mod sensor;
pub mod sensor_data;
pub mod sensor_log;

#[cfg(test)]
mod test;
