use std::sync::Arc;

use chrono_tz::UTC;

use super::*;
use crate::{config::CONFIG, models::establish_db_connection};

async fn build_mocked_observer() -> Arc<ConcurrentSensorObserver> {
    let plugin_path = CONFIG.plugin_dir();
    let plugin_dir = std::path::Path::new(&plugin_path);
    let db_conn = establish_db_connection().await.unwrap();
    ConcurrentSensorObserver::new(plugin_dir, db_conn)
}

#[tokio::test]
async fn test_insert_sensor() {
    // prepare
    let observer = build_mocked_observer().await;

    // Execute
    let mut results = Vec::<i32>::new();
    for _ in 0..2 {
        let res = observer.register_sensor(None).await;

        let resp = res.unwrap();
        results.push(resp.id);
    }

    // Validate
    assert_ne!(results[0], results[1]);
}

#[tokio::test]
async fn test_unregister_sensor() {
    // prepare
    let observer = build_mocked_observer().await;
    let cred = observer.register_sensor(None).await.unwrap();

    // execute
    let res = observer.unregister_sensor(cred.id, cred.key).await;

    // validate
    assert!(res.is_ok());
}

#[tokio::test]
async fn test_invalid_remove_sensor() {
    // prepare
    let observer = build_mocked_observer().await;
    let remove_id = -1;
    let remove_key = "asdase".to_owned();

    // execute
    let res = observer.unregister_sensor(remove_id, remove_key).await;

    // validate
    assert!(res.is_err());
}

#[tokio::test]
async fn test_sensor_status() {
    // prepare
    let observer = build_mocked_observer().await;
    let sensor_res = observer.register_sensor(None).await.unwrap();

    // execute
    let res = observer.sensor_status(sensor_res.id, sensor_res.key, UTC).await;

    // validate
    assert!(res.is_ok());
}

#[tokio::test]
async fn test_register_agent() {
    // prepare
    let domain = "TEST".to_owned();
    let agent = "MockAgent".to_owned();
    let observer = build_mocked_observer().await;
    let sensor_res = observer.register_sensor(None).await.unwrap();

    // execute

    let res = observer
        .register_agent(sensor_res.id, sensor_res.key, domain, agent)
        .await;

    // validate
    assert!(res.is_ok());
}

#[tokio::test]
async fn test_unregister_agent() {
    // prepare
    let domain = "TEST".to_owned();
    let agent = "MockAgent".to_owned();
    let observer = build_mocked_observer().await;
    let sensor_res = observer.register_sensor(None).await.unwrap();
    observer
        .register_agent(
            sensor_res.id,
            sensor_res.key.clone(),
            domain.clone(),
            agent.clone(),
        )
        .await
        .unwrap();

    // execute
    let res = observer
        .unregister_agent(sensor_res.id, sensor_res.key, domain, agent)
        .await;

    // validate
    assert!(res.is_ok());
}

#[tokio::test]
async fn test_agent_config() {
    todo!()
}

#[tokio::test]
async fn test_set_agent_config() {
    todo!()
}
