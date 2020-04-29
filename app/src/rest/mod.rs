use crate::error::ObserverError;
use crate::logging::APP_LOGGING;
use crate::models::dto;
use crate::observer::ConcurrentSensorObserver;
use actix_web::http::StatusCode;
use actix_web::{middleware, web, App, HttpResponse, HttpServer};
use std::sync::Arc;

async fn sensor_register(
    observer: web::Data<Arc<ConcurrentSensorObserver>>,
    register_request: web::Json<dto::RegisterRequestDto>,
) -> HttpResponse {
    let resp = observer
        .register_new_sensor(&register_request.name, &register_request.agents)
        .await;
    match resp {
        Ok(insert_id) => HttpResponse::Ok().json(dto::RegisterResponseDto { id: insert_id }),
        Err(ObserverError::Internal(err)) => {
            error!(APP_LOGGING, "{}", err);
            HttpResponse::new(StatusCode::INTERNAL_SERVER_ERROR)
        }
        Err(ObserverError::User(err_msg)) => {
            warn!(APP_LOGGING, "{}", err_msg);
            HttpResponse::build(StatusCode::BAD_REQUEST).json(dto::ErrorResponseDto {
                error: format!("{}", err_msg),
            })
        }
    }
}

async fn sensor_unregister(
    observer: web::Data<Arc<ConcurrentSensorObserver>>,
    unregister_request: web::Json<dto::UnregisterRequestDto>,
) -> HttpResponse {
    let remove_id = unregister_request.id;
    match observer.remove_sensor(remove_id).await {
        Ok(_) => HttpResponse::Ok().json(dto::RegisterResponseDto { id: remove_id }),
        Err(ObserverError::Internal(err)) => {
            error!(APP_LOGGING, "{}", err);
            HttpResponse::new(StatusCode::INTERNAL_SERVER_ERROR)
        }
        Err(ObserverError::User(err_msg)) => {
            warn!(APP_LOGGING, "{}", err_msg);
            HttpResponse::build(StatusCode::BAD_REQUEST).json(dto::ErrorResponseDto {
                error: format!("{}", err_msg),
            })
        }
    }
}

async fn agents(observer: web::Data<Arc<ConcurrentSensorObserver>>) -> HttpResponse {
    let agents = observer.agents().await;
    HttpResponse::Ok().json(agents)
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
                web::resource("api/sensor")
                    .route(web::get().to(agents))
                    .route(web::post().to(sensor_register))
                    .route(web::delete().to(sensor_unregister)),
            )
    })
    .bind("0.0.0.0:8000")
    .unwrap()
    .disable_signals()
    .run()
    .await
    .unwrap();
}

#[cfg(test)]
mod test {
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
                .service(web::resource("/api/sensor").route(web::post().to(sensor_register))),
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
            serde_json::from_slice::<dto::RegisterResponseDto>(body).unwrap();
        }
    }

    #[actix_rt::test]
    async fn test_remove_sensor() {
        // prepare
        let observer = ConcurrentSensorObserver::new();
        let mut app = test::init_service(
            App::new().app_data(web::Data::new(observer)).service(
                web::resource("api/sensor")
                    .route(web::delete().to(sensor_unregister))
                    .route(web::post().to(sensor_register)),
            ),
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
                .service(web::resource("api/sensor").route(web::delete().to(sensor_unregister))),
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
}
