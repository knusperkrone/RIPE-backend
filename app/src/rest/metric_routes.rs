use super::build_response_with_status;
use crate::sensor::ConcurrentSensorObserver;
use std::sync::Arc;
use warp::{hyper::StatusCode, Filter, Reply};

pub fn routes(
    observer: &Arc<ConcurrentSensorObserver>,
) -> impl Filter<Extract = impl Reply, Error = warp::Rejection> + Clone {
    health(observer.clone())
}

fn health(
    observer: Arc<ConcurrentSensorObserver>,
) -> impl Filter<Extract = impl Reply, Error = warp::Rejection> + Clone {
    warp::any()
        .map(move || observer.clone())
        .and(warp::path!("api" / "health"))
        .and_then(|observer: Arc<ConcurrentSensorObserver>| async move {
            use tokio::time::timeout;
            let duration = std::time::Duration::from_millis(500);
            let results = tokio::join!(
                timeout(duration.clone(), observer.mqtt_broker_tcp()),
                timeout(duration.clone(), observer.check_db()),
                timeout(duration.clone(), observer.sensor_count()),
                timeout(duration.clone(), observer.agents())
            );
            let healthy =
                results.0.is_ok() && results.1.is_ok() && results.2.is_ok() && results.3.is_ok();
            let mqtt_broker = results.0.unwrap_or(Some("TIMEOUT".to_owned()));
            let database_state = results.1.unwrap_or("TIMEOUT".to_owned());
            let sensor_count = results.2.unwrap_or(usize::MAX);
            let active_agents = results.3.unwrap_or(vec!["TIMEOUT".to_owned()]);
            let status = if healthy {
                StatusCode::OK
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };

            build_response_with_status(
                Ok(dto::HealthyDto {
                    healthy,
                    mqtt_broker,
                    database_state,
                    sensor_count,
                    active_agents,
                }),
                status,
            )
        })
        .boxed()
}

mod dto {
    use serde::Serialize;
    #[derive(Debug, Serialize)]
    pub struct HealthyDto {
        pub healthy: bool,
        pub mqtt_broker: Option<String>,
        pub database_state: String,
        pub sensor_count: usize,
        pub active_agents: Vec<String>,
    }
}
