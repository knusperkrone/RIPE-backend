use crate::logging::APP_LOGGING;
use crate::observer::ConcurrentSensorObserver;
use actix_web::{middleware, web, App, HttpResponse, HttpServer};
use std::sync::Arc;

pub mod dto {
    use crate::models::NewSensorAgentConfig;
    use serde::{Deserialize, Serialize};
    use std::string::String;

    #[derive(Debug, Serialize, Deserialize)]
    pub struct RegisterRequestDto {
        pub actions: Vec<AgentRegisterConfig>,
        pub name: Option<String>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct RegisterResponseDto {
        pub id: i32,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct UnregisterRequestDto {
        pub id: i32,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct UnregisterResponseDto {
        pub id: i32,
    }
}

async fn sensor_register(
    observer: web::Data<Arc<ConcurrentSensorObserver>>,
    register_request: web::Json<dto::RegisterRequestDto>,
) -> Option<HttpResponse> {
    if let Some(insert_id) = observer
        .register_new_sensor(&register_request.name, &register_request.actions)
        .await
    {
        Some(HttpResponse::Ok().json(dto::RegisterResponseDto { id: insert_id }))
    } else {
        None
    }
}

async fn sensor_unregister(
    observer: web::Data<Arc<ConcurrentSensorObserver>>,
    unregister_request: web::Json<dto::UnregisterRequestDto>,
) -> Option<HttpResponse> {
    let remove_id = unregister_request.id;
    if observer.remove_sensor(remove_id).await {
        Some(HttpResponse::Ok().json(dto::RegisterResponseDto { id: remove_id }))
    } else {
        None
    }
}

pub async fn dispatch_server(observer: Arc<ConcurrentSensorObserver>) {
    // Set up logging
    info!(APP_LOGGING, "Start listening to REST endpoints");
    std::env::set_var("RUST_LOG", "actix_web=info");
    env_logger::init();

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(observer.clone()))
            .data(web::JsonConfig::default().limit(4096))
            .wrap(middleware::Logger::default())
            .service(
                web::resource("api/sensor/register")
                    .route(web::post().to(sensor_register))
                    .route(web::delete().to(sensor_unregister)),
            )
    })
    .bind("127.0.0.1:8000")
    .unwrap()
    .disable_signals()
    .run()
    .await
    .unwrap();
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::agent::Agent;
    use actix_web::dev::Service;
    use actix_web::{test, web, App};

    #[actix_rt::test]
    async fn test_insert_sensor() {
        // prepare
        let (observer, _) = ConcurrentSensorObserver::new();
        let mut app = test::init_service(
            App::new().app_data(web::Data::new(observer)).service(
                web::resource("/api/sensor/register")
                    .route(web::post().to(sensor_register))
                    .route(web::delete().to(sensor_unregister)),
            ),
        )
        .await;

        let request_json = dto::RegisterRequestDto {
            name: None,
            actions: vec![AgentRegisterConfig::new(
                Agent::WATER_IDENTIFIER.to_string(),
            )],
        };

        // execute
        for i in 0..10 {
            let req = test::TestRequest::post()
                .uri("/api/sensor/register")
                .set_json(&request_json)
                .to_request();
            let resp = app.call(req).await.unwrap();

            assert_eq!(200, resp.response().status());
            let body = match resp.response().body().as_ref() {
                Some(actix_web::body::Body::Bytes(bytes)) => bytes,
                _ => panic!("Response error"),
            };

            // validate
            let serialized_resp: dto::RegisterResponseDto = serde_json::from_slice(body).unwrap();
            assert_eq!(serialized_resp.id, i);
        }
    }

    #[actix_rt::test]
    async fn test_remove_sensor() {
        // prepare
        let preferred_id = 0x0;
        let (observer, _) = ConcurrentSensorObserver::new();
        let mut app = test::init_service(
            App::new().app_data(web::Data::new(observer)).service(
                web::resource("api/sensor/register")
                    .route(web::post().to(sensor_register))
                    .route(web::delete().to(sensor_unregister)),
            ),
        )
        .await;
        let create_json = dto::RegisterRequestDto {
            name: None,
            actions: vec![AgentRegisterConfig::new(
                Agent::WATER_IDENTIFIER.to_string(),
            )],
        };
        let delete_json = dto::UnregisterRequestDto { id: preferred_id };

        // execute
        let mut req = test::TestRequest::post()
            .uri("/api/sensor/register")
            .set_json(&create_json)
            .to_request();
        let _ = app.call(req).await.unwrap();

        req = test::TestRequest::delete()
            .uri("/api/sensor/register")
            .set_json(&delete_json)
            .to_request();
        let resp_delete = app.call(req).await.unwrap();

        // validate
        let body = match resp_delete.response().body().as_ref() {
            Some(actix_web::body::Body::Bytes(bytes)) => bytes,
            _ => panic!("Response error"),
        };
        let serialized_resp: dto::UnregisterResponeDto = serde_json::from_slice(&body).unwrap();
        assert_eq!(serialized_resp.id, preferred_id);
    }

    #[actix_rt::test]
    async fn test_invalid_remove_sensor() {
        // prepare
        let preferred_id = 0x80;
        let (observer, _) = ConcurrentSensorObserver::new();
        let mut app = test::init_service(
            App::new().app_data(web::Data::new(observer)).service(
                web::resource("api/sensor/register")
                    .route(web::post().to(sensor_register))
                    .route(web::delete().to(sensor_unregister)),
            ),
        )
        .await;
        let delete_json = dto::UnregisterRequestDto { id: preferred_id };

        // execute
        let req = test::TestRequest::delete()
            .uri("/api/sensor/register")
            .set_json(&delete_json)
            .to_request();
        let resp_delete = app.call(req).await.unwrap();

        // validate
        assert_eq!(404, resp_delete.status());
    }
}
