use crate::error::ObserverError;
use crate::logging::APP_LOGGING;
use crate::sensor::ConcurrentSensorObserver;
use actix_web::http::StatusCode;
use actix_web::{middleware, web, App, HttpResponse, HttpServer};
use dotenv::dotenv;
use std::{env, sync::Arc};

mod agent;
mod sensor;

pub use agent::dto::*;
pub use sensor::dto::*;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ErrorResponseDto {
    pub error: String,
}

pub fn build_response<T: serde::Serialize>(resp: Result<T, ObserverError>) -> HttpResponse {
    match resp {
        Ok(data) => HttpResponse::Ok().json(data),
        Err(ObserverError::User(err)) => {
            warn!(APP_LOGGING, "{}", err);
            HttpResponse::build(StatusCode::BAD_REQUEST).json(ErrorResponseDto {
                error: format!("{}", err),
            })
        }
        Err(ObserverError::Internal(err)) => {
            error!(APP_LOGGING, "{}", err);
            HttpResponse::new(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub async fn dispatch_server_daemon(observer: Arc<ConcurrentSensorObserver>) -> () {
    // Set up logging
    dotenv().ok();
    std::env::set_var("RUST_LOG", "actix_web=info");
    let bind_addr = env::var("BIND_ADDR").expect("BIND_ADDR must be set");
    let print_addr = bind_addr.clone();

    info!(APP_LOGGING, "Starting webserver at: {}", print_addr);
    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(observer.clone()))
            .data(web::JsonConfig::default().limit(4096))
            .wrap(middleware::Logger::default())
            .configure(agent::config_endpoints)
            .configure(sensor::config_endpoints)
    })
    .bind(bind_addr)
    .unwrap()
    .disable_signals()
    .run()
    .await
    .unwrap();
}
