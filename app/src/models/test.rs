use chrono::Utc;
use ripe_core::SensorDataMessage;

use super::agent;
use super::sensor;
use super::sensor_data;
use super::sensor_log;
use super::*;

#[tokio::test]
async fn test_db_connection() {
    establish_db_connection().await;
}

#[tokio::test]
async fn crud_sensors() {
    let conn = establish_db_connection().await.unwrap();

    // create
    let sensor = sensor::insert(&conn, &"123456".to_owned(), None)
        .await
        .unwrap();

    // read
    assert_ne!(sensor::read(&conn).await.unwrap().is_empty(), true);

    // delete
    sensor::delete(&conn, sensor.id()).await.unwrap();
}

#[tokio::test]
async fn crud_agent_configs() {
    let conn = establish_db_connection().await.unwrap();
    let sensor = sensor::insert(&conn, &"123456".to_owned(), None)
        .await
        .unwrap();
    let mut dao = agent::AgentConfigDao {
        sensor_id: sensor.id(),
        state_json: "{}".to_owned(),
        domain: "test_domain".to_owned(),
        agent_impl: "agent_impl".to_owned(),
    };

    // create
    agent::insert(&conn, &dao).await.unwrap();

    // read
    let mut actual_config = agent::get(&conn, &sensor).await.unwrap().pop().unwrap();
    assert_eq!(dao.sensor_id(), actual_config.sensor_id());
    assert_eq!(dao.domain(), actual_config.domain());
    assert_eq!(dao.state_json(), actual_config.state_json());
    assert_eq!(dao.agent_impl(), actual_config.agent_impl());

    // update
    dao.state_json = "{\"key\": true}".to_owned();
    agent::update(&conn, &dao).await.unwrap();
    actual_config = agent::get(&conn, &sensor).await.unwrap().pop().unwrap();
    assert_eq!(dao.sensor_id(), actual_config.sensor_id());
    assert_eq!(dao.domain(), actual_config.domain());
    assert_eq!(dao.state_json(), actual_config.state_json());
    assert_eq!(dao.agent_impl(), actual_config.agent_impl());

    // delete
    sensor::delete(&conn, sensor.id()).await.unwrap();
}

#[tokio::test]
async fn crud_sensor_data() {
    let conn = establish_db_connection().await.unwrap();
    let sensor = sensor::insert(&conn, &"123456".to_owned(), None)
        .await
        .unwrap();

    // create
    // insert
    sensor_data::insert(
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

    let row_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM sensor_data WHERE sensor_id = $1")
        .bind(sensor.id())
        .fetch_one(&conn)
        .await
        .unwrap();
    assert_eq!(1, row_count.0);

    for _ in 0..2 {
        // insert||update
        sensor_data::insert(
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
    sensor_data::get_latest(&conn, sensor.id(), sensor.key_b64())
        .await
        .unwrap()
        .unwrap();
    sensor_data::get_latest_unchecked(&conn, sensor.id())
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn crud_sensor_log() {
    let conn = establish_db_connection().await.unwrap();
    let sensor = sensor::insert(&conn, &"123456".to_owned(), None)
        .await
        .unwrap();

    // create
    let max_count = 5;
    for i in 0..10 {
        sensor_log::upsert(&conn, sensor.id(), format!("{}", i), max_count)
            .await
            .unwrap();
    }
    let row_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM sensor_log WHERE sensor_id = $1")
        .bind(sensor.id())
        .fetch_one(&conn)
        .await
        .unwrap();
    assert_eq!(max_count, row_count.0);

    // get
    let mut daos = sensor_log::get(&conn, sensor.id()).await.unwrap();
    let mut index = max_count;

    for dao in daos.drain(..) {
        assert_eq!(&format!("{}", index), dao.log());
        index += 1;
    }
}
