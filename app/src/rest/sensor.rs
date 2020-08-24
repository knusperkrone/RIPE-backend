use super::build_response;
use crate::sensor::ConcurrentSensorObserver;
use actix_web::{web, HttpResponse};
use std::sync::Arc;

async fn sensor_register(
    observer: web::Data<Arc<ConcurrentSensorObserver>>,
    register_request: web::Json<dto::SensorRegisterRequestDto>,
) -> HttpResponse {
    let resp = observer.register_new_sensor(&register_request.name).await;
    build_response(resp)
}

async fn sensor_unregister(
    observer: web::Data<Arc<ConcurrentSensorObserver>>,
    unregister_request: web::Json<dto::SensorUnregisterRequestDto>,
) -> HttpResponse {
    let remove_id = unregister_request.id;
    let resp = observer.remove_sensor(remove_id).await;
    build_response(resp)
}

async fn sensor_reload(
    observer: web::Data<Arc<ConcurrentSensorObserver>>,
    path: web::Path<(i32, String)>,
) -> HttpResponse {
    let (sensor_id, key_b64) = path.into_inner();
    let resp = observer.reload_sensor(sensor_id, key_b64).await;
    build_response(resp)
}

async fn sensor_status(
    observer: web::Data<Arc<ConcurrentSensorObserver>>,
    path: web::Path<(i32, String)>,
) -> HttpResponse {
    let (sensor_id, key_b64) = path.into_inner();
    let resp = observer.sensor_status(sensor_id, key_b64).await;
    build_response(resp)
}

pub fn config_endpoints(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::resource("api/sensor")
            .route(web::post().to(sensor_register))
            .route(web::delete().to(sensor_unregister)),
    )
    .service(web::resource("api/sensor/{id}/{key}").route(web::get().to(sensor_status)))
    .service(web::resource("api/sensor/{id}/{key}/reload").route(web::post().to(sensor_reload)));
}

pub mod dto {
    use crate::rest::{AgentPayload, AgentStatusDto};
    use iftem_core::SensorDataMessage;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize)]
    pub struct SensorRegisterRequestDto {
        pub name: Option<String>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct SensorRegisterResponseDto {
        pub id: i32,
        pub key: String,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct SensorUnregisterRequestDto {
        pub id: i32,
    }

    #[derive(Debug, Serialize)]
    pub struct SensorMessageDto {
        #[serde(skip_serializing)]
        pub sensor_id: i32,
        pub domain: String,
        pub payload: AgentPayload,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct SensorStatusDto {
        pub name: String,
        pub data: SensorDataMessage,
        pub agents: Vec<AgentStatusDto>,
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::rest::AgentRegisterDto;
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

        let request_json = dto::SensorRegisterRequestDto { name: None };

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
            serde_json::from_slice::<dto::SensorRegisterResponseDto>(body).unwrap();
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
        let create_json = dto::SensorRegisterRequestDto { name: None };
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
        let created_id = serde_json::from_slice::<dto::SensorRegisterResponseDto>(body)
            .unwrap()
            .id;
        let delete_json = dto::SensorUnregisterRequestDto { id: created_id };

        // execute
        req = test::TestRequest::delete()
            .uri("/api/sensor")
            .set_json(&delete_json)
            .to_request();
        resp = app.call(req).await.unwrap();

        // validate
        assert_eq!(200, resp.status());
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
        let delete_json = dto::SensorUnregisterRequestDto { id: preferred_id };

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
        let register = observer
            .register_new_sensor(&Some(sensor_name.to_owned()))
            .await
            .unwrap();
        observer
            .register_agent(
                register.id,
                register.key.clone(),
                AgentRegisterDto {
                    agent_name: mock_agent.to_owned(),
                    domain: agent_domain.to_owned(),
                },
            )
            .await
            .unwrap();

        let mut app = test::init_service(
            App::new()
                .app_data(web::Data::new(observer))
                .configure(super::config_endpoints),
        )
        .await;

        // Execute
        let req = test::TestRequest::get()
            .uri(&format!(
                "/api/sensor/{}/{}",
                register.id,
                register.key.clone()
            ))
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
}
