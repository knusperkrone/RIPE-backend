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
            use tokio::time::timeout;
            let duration = std::time::Duration::from_millis(500);
            let mqtt_broker = timeout(duration.clone(), observer.mqtt_broker())
                .await
                .unwrap_or(Some("TIMEOUT".to_owned()));
            let database_state = timeout(duration.clone(), observer.check_db())
                .await
                .unwrap_or("TIMEOUT".to_owned());
            let sensor_count = timeout(duration.clone(), observer.sensor_count())
                .await
                .unwrap_or(usize::MAX);
            let active_agents = timeout(duration.clone(), observer.agents())
                .await
                .unwrap_or(vec!["TIMEOUT".to_owned()]);

            let ret = dto::HealthyDto {
                healthy: true,
                mqtt_broker,
                database_state,
                sensor_count,
                active_agents,
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
