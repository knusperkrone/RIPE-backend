use crate::error::ObserverError;
use crate::logging::APP_LOGGING;
use crate::models::dto;
use crate::sensor::ConcurrentSensorObserver;
use actix_web::http::StatusCode;
use actix_web::{middleware, web, App, HttpResponse, HttpServer};
use dotenv::dotenv;
use std::{env, sync::Arc};

async fn sensor_register(
    observer: web::Data<Arc<ConcurrentSensorObserver>>,
    register_request: web::Json<dto::RegisterRequestDto>,
) -> HttpResponse {
    let resp = observer
        .register_new_sensor(&register_request.name, &register_request.agents)
        .await;
    build_response(resp)
}

async fn sensor_unregister(
    observer: web::Data<Arc<ConcurrentSensorObserver>>,
    unregister_request: web::Json<dto::UnregisterRequestDto>,
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

async fn get_agents(observer: web::Data<Arc<ConcurrentSensorObserver>>) -> HttpResponse {
    let agents = observer.agents().await;
    HttpResponse::Ok().json(agents)
}

async fn sensor_status(
    observer: web::Data<Arc<ConcurrentSensorObserver>>,
    path: web::Path<(i32, String)>,
) -> HttpResponse {
    let (sensor_id, key_b64) = path.into_inner();
    let resp = observer.sensor_status(sensor_id, key_b64).await;
    build_response(resp)
}

fn build_response<T: serde::Serialize>(resp: Result<T, ObserverError>) -> HttpResponse {
    match resp {
        Ok(data) => HttpResponse::Ok().json(data),
        Err(ObserverError::User(err)) => {
            warn!(APP_LOGGING, "{}", err);
            HttpResponse::build(StatusCode::BAD_REQUEST).json(dto::ErrorResponseDto {
                error: format!("{}", err),
            })
        }
        Err(ObserverError::Internal(err)) => {
            error!(APP_LOGGING, "{}", err);
            HttpResponse::new(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

fn config_endpoints(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::resource("api/sensor")
            .route(web::get().to(get_agents))
            .route(web::post().to(sensor_register))
            .route(web::delete().to(sensor_unregister)),
    )
    .service(web::resource("api/sensor/{id}/{key}").route(web::get().to(sensor_status)))
    .service(web::resource("api/sensor/{id}/{key}/reload").route(web::post().to(sensor_reload)));
}

pub async fn dispatch_server(observer: Arc<ConcurrentSensorObserver>) {
    // Set up logging
    dotenv().ok();
    std::env::set_var("RUST_LOG", "actix_web=info");
    env_logger::init();
    let bind_addr = env::var("BIND_ADDR").expect("BIND_ADDR must be set");
    let print_addr = bind_addr.clone();

    info!(APP_LOGGING, "Starting webserver at: {}", print_addr);
    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(observer.clone()))
            .data(web::JsonConfig::default().limit(4096))
            .wrap(middleware::Logger::default())
            .configure(config_endpoints)
    })
    .bind(bind_addr)
    .unwrap()
    .disable_signals()
    .run()
    .await
    .unwrap();
}

#[cfg(test)]
mod test;
