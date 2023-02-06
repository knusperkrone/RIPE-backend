use crate::config::CONFIG;
use crate::error::ObserverError;
use crate::logging::APP_LOGGING;
use crate::sensor::ConcurrentSensorObserver;
use std::net::IpAddr;
use std::str::FromStr;
use std::{convert::Infallible, sync::Arc};
use warp::hyper::StatusCode;
use warp::Filter;

mod agent_routes;
mod doc_routes;
mod metric_routes;
mod sensor_routes;

pub use agent_routes::dto::*;
pub use sensor_routes::dto::*;

#[derive(Debug, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct ErrorResponseDto {
    pub error: String,
}

pub struct SwaggerHostDefinition {
    open_api: utoipa::openapi::OpenApi,
}

pub fn build_response<T: serde::Serialize>(
    resp: Result<T, ObserverError>,
) -> Result<impl warp::Reply, Infallible> {
    build_response_with_status(resp, StatusCode::OK)
}

pub fn build_response_with_status<T: serde::Serialize>(
    resp: Result<T, ObserverError>,
    status: StatusCode,
) -> Result<impl warp::Reply, Infallible> {
    match resp {
        Ok(data) => Ok(warp::reply::with_status(warp::reply::json(&data), status)),
        Err(ObserverError::User(err)) => {
            warn!(APP_LOGGING, "UserRequest error: {}", err);
            let json = warp::reply::json(&ErrorResponseDto {
                error: format!("{}", err),
            });
            Ok(warp::reply::with_status(json, StatusCode::BAD_REQUEST))
        }
        Err(ObserverError::Internal(err)) => {
            error!(APP_LOGGING, "InternalRequest error: {}", err);
            let json = warp::reply::json(&ErrorResponseDto {
                error: "Internal Error".to_owned(),
            });
            Ok(warp::reply::with_status(
                json,
                StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    }
}

pub async fn dispatch_server_daemon(observer: Arc<ConcurrentSensorObserver>) {
    // Set up logging
    std::env::set_var("RUST_LOG", "actix_web=info");
    let server_port = CONFIG.server_port();
    let (sensor_swagger_path, sensor_routes) = sensor_routes::routes(&observer);
    let (metric_swagger_path, metric_routes) = metric_routes::routes(&observer);
    let (agent_swagger_path, agent_routes) = agent_routes::routes(&observer);

    info!(
        APP_LOGGING,
        "Starting webserver at: 0.0.0.0:{}", server_port
    );
    let addr = IpAddr::from_str("::0").unwrap();
    warp::serve(
        sensor_routes
            .or(metric_routes)
            .or(agent_routes)
            .or(doc_routes::swagger(vec![
                sensor_swagger_path,
                metric_swagger_path,
                agent_swagger_path,
            ])),
    )
    .run((addr, server_port.parse().unwrap()))
    .await;
}
