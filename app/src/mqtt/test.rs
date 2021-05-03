use crate::mqtt::MqttSensorClient;
use iftem_core::SensorDataMessage;
use paho_mqtt::Message;
use std::time::Duration;
use tokio::time::timeout;

#[tokio::test]
async fn test_mqtt_connection() {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let client = MqttSensorClient::new("connection_test".to_owned(), tx);
    client.connect().await;
}

#[tokio::test]
async fn test_invalid_mqtt_path() {
    // prepare
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

    // execute
    let mocked_message = Message::new("sensor/data/0".to_string(), vec![], 1);

    // validate
    let result = MqttSensorClient::on_sensor_message(&tx, mocked_message);
    assert_ne!(result.is_ok(), true);
}

#[tokio::test]
async fn test_send_path() {
    // prepare
    let sensor_id = 0;
    let key_b64 = "123456";
    let data = SensorDataMessage::default();
    let mocked_message = Message::new(
        format!("sensor/data/{}/{}", sensor_id, key_b64),
        serde_json::to_vec(&data).unwrap(),
        1,
    );
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

    // execute
    let result = MqttSensorClient::on_sensor_message(&tx, mocked_message);

    // validate
    println!("{:?}", result);
    assert_eq!(result.is_ok(), true);
    if let Ok(Some((actual_id, actual_data))) = timeout(Duration::from_millis(10), rx.recv()).await
    {
        assert_eq!(sensor_id, actual_id);
        assert_eq!(data, actual_data);
    } else {
        assert!(false);
    }
}
