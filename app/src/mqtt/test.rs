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
use paho_mqtt::Message;
use std::sync::{Arc, RwLock};

#[actix_rt::test]
async fn test_mqtt_connection() {
    let client = MqttSensorClient::new(Arc::new(RwLock::new(SensorCache::new())));
    client.connect();
}

#[actix_rt::test]
async fn test_invalid_mqtt_path() {
    // prepare
    let container = SensorCache::new();
    let mocked_container = Arc::new(RwLock::new(container));

    // execute
    let mocked_message = Message::new("sensor/data/0".to_string(), vec![], 1);

    // validate
    let result = MqttSensorClient::on_sensor_message(&mocked_container, mocked_message);
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
    let mocked_message = Message::new(
        format!("sensor/data/{}/{}", sensor_id, key_b64),
        serde_json::to_vec(&SensorDataMessage::default()).unwrap(),
        1,
    );

    let result = MqttSensorClient::on_sensor_message(&mocked_container, mocked_message);

    // validate
    assert_eq!(result.is_ok(), true);
    let container = mocked_container.read().unwrap();
    let _sensor = container.sensor(sensor_id, &key_b64.to_owned()).unwrap();
    // TODO: check agent
}
