use chrono::{DateTime, NaiveDateTime, Utc};

#[derive(sqlx::FromRow)]
#[allow(dead_code)]
pub struct AgentCommandDao {
    sensor_id: i32,
    pub domain: std::string::String,
    pub command: i32,
    pub created_at: NaiveDateTime,
}

pub async fn insert(
    conn: &sqlx::PgPool,
    sensor_id: i32,
    domain: &str,
    command: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"INSERT INTO agent_commands
                (sensor_id, domain, command)
                VALUES ($1, $2, $3)"#,
        sensor_id,
        domain,
        command
    )
    .execute(conn)
    .await?;
    Ok(())
}

pub async fn get(
    conn: &sqlx::PgPool,
    sensor_id: i32,
    from: DateTime<Utc>,
    until: DateTime<Utc>,
) -> Result<Vec<AgentCommandDao>, sqlx::Error> {
    let result = sqlx::query_as!(
        AgentCommandDao,
        r#"SELECT *
                FROM agent_commands
                WHERE sensor_id = $1 
                AND created_at >= $2
                AND created_at < $3
            "#,
        sensor_id,
        from.naive_utc(),
        until.naive_utc()
    )
    .fetch_all(conn)
    .await?;
    Ok(result)
}
