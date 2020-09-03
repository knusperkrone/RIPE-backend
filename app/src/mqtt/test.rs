use crate::models::dao::SensorDao;
use crate::plugin::{test::MockAgent, Agent};
use crate::{
    mqtt::MqttSensorClient,
    sensor::{
        handle::{SensorHandle, SensorHandleMessage},
        observer::SensorCache,
    },
};
use iftem_core::{AgentMessage, SensorDataMessage};
use rumq_client::{self, Publish, QoS};
use std::sync::Arc;
use tokio::sync::RwLock;

#[actix_rt::test]
async fn test_mqtt_connection() {
    let (_client, mut eventloop) = MqttSensorClient::new();
    let _ = eventloop.connect().await.unwrap();
}

#[actix_rt::test]
async fn test_invalid_mqtt_path() {
    // prepare
    let container = SensorCache::new();
    let mocked_container = Arc::new(RwLock::new(container));
    let (mut client, _) = MqttSensorClient::new();

    // execute
    let mocked_message = Publish {
        dup: false,
        retain: false,
        qos: QoS::ExactlyOnce,
        pkid: None,
        topic_name: "sensor/data/0".to_string(),
        payload: vec![],
    };

    // validate
    let result = client
        .on_sensor_message(&mocked_container, mocked_message)
        .await;
    assert_ne!(result.is_ok(), true);
}

#[actix_rt::test]
async fn test_valid_mqtt_path() {
    // prepare
    let sensor_id = 0;
    let key_b64 = "123456";
    let (sender, _) = tokio::sync::mpsc::channel::<SensorHandleMessage>(2);
    let (plugin_sender, plugin_receiver) = tokio::sync::mpsc::channel::<AgentMessage>(2);
    let mut container = SensorCache::new();
    let (mut client, _) = MqttSensorClient::new();
    let mock_sensor = SensorHandle {
        dao: SensorDao::new(sensor_id, key_b64.to_owned(), "mock".to_owned()),
        agents: vec![Agent::new(
            sender,
            plugin_sender,
            plugin_receiver,
            sensor_id,
            "MockDomain".to_owned(),
            "AgentName".to_owned(),
            Box::new(MockAgent::new()),
        )],
    };
    container.insert_sensor(mock_sensor);
    let mocked_container = Arc::new(RwLock::new(container));

    // execute
    let mocked_message = Publish {
        dup: false,
        retain: false,
        qos: QoS::ExactlyOnce,
        pkid: None,
        topic_name: format!("sensor/data/{}/{}", sensor_id, key_b64),
        payload: serde_json::to_vec(&SensorDataMessage::default()).unwrap(),
    };

    let result = client
        .on_sensor_message(&mocked_container, mocked_message)
        .await;

    // validate
    assert_eq!(result.is_ok(), true);
    let container = mocked_container.read().await;
    let _sensor = container.sensors(sensor_id, &key_b64.to_owned()).await;
    // TODO: check agent
}
