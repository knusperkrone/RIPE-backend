use super::build_response;
use crate::sensor::ConcurrentSensorObserver;
use std::sync::Arc;
use warp::Filter;

pub fn routes(
    observer: &Arc<ConcurrentSensorObserver>,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    health(observer.clone())
}

fn health(
    observer: Arc<ConcurrentSensorObserver>,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::any()
        .map(move || observer.clone())
        .and(warp::path!("api" / "health"))
        .and_then(|observer: Arc<ConcurrentSensorObserver>| async move {
            let ret = dto::HealthyDto {
                healthy: true,
                mqtt_broker: observer.mqtt_broker(),
                database_state: observer.check_db().await,
                sensor_count: observer.sensor_count().await,
                active_agents: observer.agents().await,
            };
            build_response(Ok(ret))
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
