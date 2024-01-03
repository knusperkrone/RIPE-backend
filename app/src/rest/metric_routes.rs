use super::{build_response_with_status, SwaggerHostDefinition};
use crate::{models, sensor::ConcurrentObserver};
use std::sync::Arc;
use warp::{hyper::StatusCode, Filter, Reply};

pub fn routes(
    observer: &Arc<ConcurrentObserver>,
) -> (
    SwaggerHostDefinition,
    impl Filter<Extract = impl Reply, Error = warp::Rejection> + Clone,
) {
    use utoipa::OpenApi;
    #[derive(OpenApi)]
    #[openapi(
        paths(health),
        components(schemas(dto::HealthyDto)),
        tags((name = "metric", description = "Application related API"))
    )]
    struct ApiDoc;

    (
        SwaggerHostDefinition {
            open_api: ApiDoc::openapi(),
        },
        health(observer.clone()).or(warp::path!("api" / "doc" / "metric-api.json")
            .and(warp::get())
            .map(|| warp::reply::json(&ApiDoc::openapi()))),
    )
}

#[utoipa::path(
    get,
    path = "/api/health",
    responses(
        (status = 200, description = "The service is healthy", body = HealthyDto, content_type = "application/json"),
        (status = 500, description = "The service is unhealthy", body = HealthyDto, content_type = "application/json"),
    ),
    tag = "metric",
)]
fn health(
    observer: Arc<ConcurrentObserver>,
) -> impl Filter<Extract = impl Reply, Error = warp::Rejection> + Clone {
    warp::any()
        .map(move || observer.clone())
        .and(warp::path!("api" / "health"))
        .and_then(|observer: Arc<ConcurrentObserver>| async move {
            use tokio::time::timeout;
            let duration = std::time::Duration::from_millis(500);
            let results = tokio::join!(
                timeout(duration, models::check_schema(&observer.db_conn)),
                timeout(duration, observer.container.read()),
                timeout(duration, observer.agent_factory.read())
            );
            let healthy = results.0.is_ok() && results.1.is_ok() && results.2.is_ok();
            let mqtt_broker = observer.mqtt_client.broker();
            let database_state = results
                .0
                .map(|_| "HEALTHY".to_owned())
                .unwrap_or("TIMEOUT".to_owned());
            let sensor_count = results
                .1
                .map(|container| container.len())
                .unwrap_or(usize::MAX);
            let active_agents = results
                .2
                .map(|factory| factory.agents().drain(..).map(ToOwned::to_owned).collect())
                .unwrap_or(vec!["TIMEOUT".to_owned()]);
            let status = if healthy {
                StatusCode::OK
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };

            build_response_with_status(
                Ok(dto::HealthyDto {
                    mqtt_broker: mqtt_broker.map(ToOwned::to_owned),
                    healthy,
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
    use utoipa::ToSchema;

    use crate::mqtt::Broker;
    #[derive(Debug, Serialize, ToSchema)]
    pub struct HealthyDto {
        pub healthy: bool,
        pub mqtt_broker: Option<Broker>,
        pub database_state: String,
        pub sensor_count: usize,
        pub active_agents: Vec<String>,
    }
}
