use crate::error::ObserverError;
use crate::logging::APP_LOGGING;
use crate::sensor::ConcurrentSensorObserver;
use dotenv::dotenv;
use std::{convert::Infallible, env, sync::Arc};
use warp::{hyper::StatusCode, Filter};

mod agent_routes;
mod sensor_routes;

pub use agent_routes::dto::*;
pub use sensor_routes::dto::*;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ErrorResponseDto {
    pub error: String,
}

pub fn build_response<T: serde::Serialize>(
    resp: Result<T, ObserverError>,
) -> Result<impl warp::Reply, Infallible> {
    match resp {
        Ok(data) => Ok(warp::reply::with_status(
            warp::reply::json(&data),
            StatusCode::OK,
        )),
        Err(ObserverError::User(err)) => {
            warn!(APP_LOGGING, "{}", err);
            let json = warp::reply::json(&ErrorResponseDto {
                error: format!("{}", err),
            });
            Ok(warp::reply::with_status(json, StatusCode::BAD_REQUEST))
        }
        Err(ObserverError::Internal(err)) => {
            error!(APP_LOGGING, "{}", err);
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
    dotenv().ok();
    std::env::set_var("RUST_LOG", "actix_web=info");
    let bind_port = env::var("BIND_PORT").expect("BIND_PORT must be set");

    let sensor_routes = sensor_routes::routes(&observer);
    let agent_routes = agent_routes::routes(&observer);

    info!(APP_LOGGING, "Starting webserver at: 0.0.0.0:{}", bind_port);
    warp::serve(sensor_routes.or(agent_routes))
        .run(([0, 0, 0, 0], bind_port.parse().unwrap()))
        .await;
}
