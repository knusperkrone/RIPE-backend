use super::*;
use actix_web::dev::Service;
use actix_web::{test, web, App};

#[actix_rt::test]
async fn test_insert_sensor() {
    // prepare
    let observer = ConcurrentSensorObserver::new();
    let mut app = test::init_service(
        App::new()
            .app_data(web::Data::new(observer))
            .configure(super::config_endpoints),
    )
    .await;

    let request_json = dto::RegisterRequestDto {
        name: None,
        agents: vec![], // TODO: Load plugin
    };

    // execute
    for _ in 0..2 {
        let req = test::TestRequest::post()
            .uri("/api/sensor")
            .set_json(&request_json)
            .to_request();
        let resp = app.call(req).await.unwrap();

        assert_eq!(200, resp.response().status());
        let body = match resp.response().body().as_ref() {
            Some(actix_web::body::Body::Bytes(bytes)) => bytes,
            _ => panic!("Response error"),
        };

        // check panic
        println!("{:?}", body);
        serde_json::from_slice::<dto::RegisterResponseDto>(body).unwrap();
    }
}

#[actix_rt::test]
async fn test_remove_sensor() {
    // prepare
    let observer = ConcurrentSensorObserver::new();
    let mut app = test::init_service(
        App::new()
            .app_data(web::Data::new(observer))
            .configure(super::config_endpoints),
    )
    .await;

    // Create new json and parse id
    let create_json = dto::RegisterRequestDto {
        name: None,
        agents: vec![], // TODO: Load plugin
    };
    let mut req = test::TestRequest::post()
        .uri("/api/sensor")
        .set_json(&create_json)
        .to_request();

    let mut resp = app.call(req).await.unwrap();
    assert_eq!(200, resp.status());
    let body = match resp.response().body().as_ref() {
        Some(actix_web::body::Body::Bytes(bytes)) => bytes,
        _ => panic!("Response error"),
    };

    // Prepare delete call
    let created_id = serde_json::from_slice::<dto::RegisterResponseDto>(body)
        .unwrap()
        .id;
    let delete_json = dto::UnregisterRequestDto { id: created_id };

    // execute
    req = test::TestRequest::delete()
        .uri("/api/sensor")
        .set_json(&delete_json)
        .to_request();
    resp = app.call(req).await.unwrap();
    assert_eq!(200, resp.status());

    // validate
    let body = match resp.response().body().as_ref() {
        Some(actix_web::body::Body::Bytes(bytes)) => bytes,
        _ => panic!("Response error"),
    };
    let del_resp: dto::UnregisterResponseDto = serde_json::from_slice(&body).unwrap();
    assert_eq!(created_id, del_resp.id);
}

#[actix_rt::test]
async fn test_invalid_remove_sensor() {
    // prepare
    let preferred_id = std::i32::MAX;
    let observer = ConcurrentSensorObserver::new();
    let mut app = test::init_service(
        App::new()
            .app_data(web::Data::new(observer))
            .configure(super::config_endpoints),
    )
    .await;
    let delete_json = dto::UnregisterRequestDto { id: preferred_id };

    // execute
    let req = test::TestRequest::delete()
        .uri("/api/sensor")
        .set_json(&delete_json)
        .to_request();
    let resp_delete = app.call(req).await.unwrap();

    // validate
    assert_eq!(400, resp_delete.status());
}

#[actix_rt::test]
async fn test_sensor_status() {
    // Prepare
    let sensor_name = "Status_Sensor";
    let agent_domain = "Water";
    let mock_agent = "MockAgent";
    let observer = ConcurrentSensorObserver::new();
    let sensor_id = observer
        .register_new_sensor(
            &Some(sensor_name.to_owned()),
            &vec![dto::AgentRegisterDto {
                agent_name: mock_agent.to_owned(),
                domain: agent_domain.to_owned(),
            }],
        )
        .await
        .unwrap()
        .id;
    let mut app = test::init_service(
        App::new()
            .app_data(web::Data::new(observer))
            .configure(super::config_endpoints),
    )
    .await;

    // Execute
    let req = test::TestRequest::get()
        .uri(&format!("/api/sensor/{}/123456", sensor_id))
        .to_request();
    let resp = app.call(req).await.unwrap();

    // Validate
    assert_eq!(200, resp.status());
    let body = match resp.response().body().as_ref() {
        Some(actix_web::body::Body::Bytes(bytes)) => bytes,
        _ => panic!("Response error"),
    };
    let status: dto::SensorStatusDto = serde_json::from_slice(&body).unwrap();
    assert_eq!(sensor_name, status.name);

    assert_eq!(1, status.agents.len());
    let agent = &status.agents[0];
    assert_eq!(mock_agent, agent.agent_name);
    assert_eq!(agent_domain, agent.domain);
}