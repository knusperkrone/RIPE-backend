use super::{build_response, SwaggerHostDefinition};
use crate::sensor::ConcurrentSensorObserver;
use chrono_tz::Tz;
use chrono_tz::UTC;
use std::sync::Arc;
use warp::Filter;

pub fn routes(
    observer: &Arc<ConcurrentSensorObserver>,
) -> (
    SwaggerHostDefinition,
    impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone,
) {
    use super::ErrorResponseDto;
    use crate::rest::AgentStatusDto;
    use ripe_core::{AgentUI, AgentUIDecorator, SensorDataMessage, AgentState};
    use utoipa::OpenApi;
    #[derive(OpenApi)]
    #[openapi(
        paths(register_sensor, unregister_sensor, sensor_status, sensor_logs, sensor_reload),
        components(schemas(dto::SensorRegisterRequestDto, dto::BrokerDto, dto::SensorCredentialDto, dto::SensorStatusDto, 
            SensorDataMessage, ErrorResponseDto, AgentStatusDto, AgentUI, AgentUIDecorator, AgentState
        )),
        tags((name = "sensor", description = "Sensor related API"))
    )]
    struct ApiDoc;

    (
        SwaggerHostDefinition {
            url: "/api/doc/sensor-api.json".to_owned(),
            openApi: ApiDoc::openapi(),
        },
        register_sensor(observer.clone())
            .or(unregister_sensor(observer.clone()))
            .or(sensor_status(observer.clone()))
            .or(sensor_logs(observer.clone()))
            .or(sensor_reload(observer.clone()))
            .or(warp::path!("api" / "doc" / "sensor-api.json")
                .and(warp::get())
                .map(|| warp::reply::json(&ApiDoc::openapi()))),
    )
}

#[utoipa::path(
    post,
    path = "/api/sensor",
    request_body(content = SensorRegisterRequestDto, content_type = "application/json"),
    responses(
        (status = 200, description = "The freshly registered sensor, store the id and key safely", body = SensorStatusDto, content_type = "application/json"),
        (status = 400, description = "Invalid request body", body = ErrorResponseDto, content_type = "application/json"),
        (status = 500, description = "Internal error", body = ErrorResponseDto, content_type = "application/json"),
    ),
    tag = "sensor",
)]
fn register_sensor(
    observer: Arc<ConcurrentSensorObserver>,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::any()
        .map(move || observer.clone())
        .and(warp::post())
        .and(warp::path!("api" / "sensor"))
        .and(warp::body::json())
        .and_then(
            |observer: Arc<ConcurrentSensorObserver>, body: dto::SensorRegisterRequestDto| async move {
                let resp = observer.register_sensor(body.name).await;
                build_response(resp)
            },
        )
        .boxed()
}

#[utoipa::path(
    delete,
    path = "/api/sensor",
    request_body(content = SensorCredentialDto, content_type = "application/json"),
    responses(
        (status = 200, description = "Delete a sensor", body = SensorStatusDto, content_type = "application/json"),
        (status = 400, description = "Agent not found or invalid credentials", body = ErrorResponseDto, content_type = "application/json"),
        (status = 500, description = "Internal error", body = ErrorResponseDto, content_type = "application/json"),
    ),
    tag = "sensor",
)]
fn unregister_sensor(
    observer: Arc<ConcurrentSensorObserver>,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::any()
        .map(move || observer.clone())
        .and(warp::delete())
        .and(warp::path!("api" / "sensor"))
        .and(warp::body::json())
        .and_then(
            |observer: Arc<ConcurrentSensorObserver>, body: dto::SensorCredentialDto| async move {
                let resp = observer.unregister_sensor(body.id, body.key).await;
                build_response(resp)
            },
        )
        .boxed()
}

#[utoipa::path(
    get,
    path = "/api/sensor/{id}",
    params(
        ("id" = i32, Path, description = "The sensor id"),
        ("x-key" = String, Header, description = "The sensor key"),
        ("x-tz" = Option<String>, Header, description = "The timezone to format displayed text into"),
    ),
    responses(
        (status = 200, description = "The current sensor status", body = SensorStatusDto, content_type = "application/json"),
        (status = 400, description = "Agent not found or invalid credentials", body = ErrorResponseDto, content_type = "application/json"),
        (status = 500, description = "Internal error", body = ErrorResponseDto, content_type = "application/json"),
    ),
    tag = "sensor",
)]
fn sensor_status(
    observer: Arc<ConcurrentSensorObserver>,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    let timezone_header = warp::header::optional::<String>("X-TZ");
    let key_header = warp::header::optional::<String>("X-KEY");
    warp::any()
        .map(move || observer.clone())
        .and(warp::get())
        .and(timezone_header)
        .and(key_header)
        .and(warp::path!("api" / "sensor" / i32))
        .and_then(
            |observer: Arc<ConcurrentSensorObserver>,
             tz_opt: Option<String>,
             key_b64: Option<String>,
             sensor_id: i32| async move {
                let tz: Tz = tz_opt.unwrap_or("UTC".to_owned()).parse().unwrap_or(UTC);
                let resp = observer
                    .sensor_status(sensor_id, key_b64.unwrap_or_default(), tz)
                    .await;
                build_response(resp)
            },
        )
        .boxed()
}

#[utoipa::path(
    get,
    path = "/api/sensor/{id}/log",
    params(
        ("id" = i32, Path, description = "The sensor id"),
        ("x-key" = String, Header, description = "The sensor key"),
        ("x-tz" = Option<String>, Header, description = "The timezone to format displayed text into"),
    ),
    responses(
        (status = 200, description = "The last X sensor logs", body = [String], content_type = "application/json"),
        (status = 400, description = "Agent not found or invalid credentials", body = ErrorResponseDto, content_type = "application/json"),
        (status = 500, description = "Internal error", body = ErrorResponseDto, content_type = "application/json"),
    ),
    tag = "sensor",
)]
fn sensor_logs(
    observer: Arc<ConcurrentSensorObserver>,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    let timezone_header = warp::header::optional::<String>("X-TZ");
    let key_header = warp::header::optional::<String>("X-KEY");
    warp::any()
        .map(move || observer.clone())
        .and(warp::get())
        .and(timezone_header)
        .and(key_header)
        .and(warp::path!("api" / "sensor" / i32 / "log"))
        .and_then(
            |observer: Arc<ConcurrentSensorObserver>,
             tz_opt: Option<String>,
             key_b64: Option<String>,
             sensor_id: i32| async move {
                let tz: Tz = tz_opt.unwrap_or("UTC".to_owned()).parse().unwrap_or(UTC);
                let resp = observer
                    .sensor_logs(sensor_id, key_b64.unwrap_or_default(), tz)
                    .await;
                build_response(resp)
            },
        )
        .boxed()
}

#[utoipa::path(
    post,
    path = "/api/sensor/{id}/reload",
    params(
        ("id" = i32, Path, description = "The sensor id"),
        ("x-key" = String, Header, description = "The sensor key"),
    ),
    responses(
        (status = 200, description = "The sensor plugins got reloaded, as all agents were inactive"),
        (status = 400, description = "Agent not found or invalid credentials", body = ErrorResponseDto, content_type = "application/json"),
        (status = 500, description = "Internal error", body = ErrorResponseDto, content_type = "application/json"),
    ),
    tag = "sensor",
)]
fn sensor_reload(
    observer: Arc<ConcurrentSensorObserver>,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    let key_header = warp::header::optional::<String>("X-KEY");
    warp::any()
        .map(move || observer.clone())
        .and(warp::post())
        .and(key_header)
        .and(warp::path!("api" / "sensor" / i32 / "reload"))
        .and_then(
            |_observer: Arc<ConcurrentSensorObserver>,
             _key_b64: Option<String>,
             _sensor_id: i32| async move {
                // TODO: let resp = observer.reload_sensor(sensor_id, key_b64).await;
                build_response(Ok(()))
            },
        )
        .boxed()
}

///
/// DTO
///
pub mod dto {
    use crate::rest::AgentStatusDto;
    use ripe_core::SensorDataMessage;
    use serde::{Deserialize, Serialize};
    use utoipa::ToSchema;

    #[derive(Debug, Serialize, Deserialize, ToSchema)]
    pub struct SensorRegisterRequestDto {
        pub name: Option<String>,
    }

    #[derive(Debug, Serialize, Deserialize, ToSchema)]
    pub struct BrokerDto {
        pub tcp: Option<String>,
        pub wss: Option<String>,
    }

    #[derive(Debug, Serialize, Deserialize, ToSchema)]
    pub struct SensorCredentialDto {
        pub id: i32,
        pub key: String,
        pub broker: BrokerDto,
    }

    #[derive(Debug, Serialize, Deserialize, ToSchema)]
    pub struct SensorStatusDto {
        pub name: String,
        pub data: SensorDataMessage,
        pub agents: Vec<AgentStatusDto>,
        pub broker: BrokerDto,
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{config::CONFIG, models::establish_db_connection};

    async fn build_mocked_observer() -> Arc<ConcurrentSensorObserver> {
        let plugin_path = CONFIG.plugin_dir();
        let plugin_dir = std::path::Path::new(&plugin_path);
        let db_conn = establish_db_connection().await.unwrap();
        ConcurrentSensorObserver::new(plugin_dir, db_conn)
    }

    #[tokio::test]
    async fn test_rest_register_sensor() {
        // Prepare
        let observer = build_mocked_observer().await;
        let routes = routes(&observer).1;

        // Execute
        let dto = dto::SensorRegisterRequestDto { name: None };
        let res = warp::test::request()
            .path("/api/sensor")
            .method("POST")
            .json(&dto)
            .reply(&routes)
            .await;

        // Validate
        assert_eq!(res.status(), 200);
        let _: dto::SensorCredentialDto = serde_json::from_slice(res.body()).unwrap();
    }

    #[tokio::test]
    async fn test_rest_unregister_sensor() {
        // Prepare
        let observer = build_mocked_observer().await;
        let routes = routes(&observer).1;
        let dto = observer.register_sensor(None).await.unwrap();

        // Execute
        let res = warp::test::request()
            .path("/api/sensor")
            .method("DELETE")
            .json(&dto)
            .reply(&routes)
            .await;

        // Validate
        assert_eq!(res.status(), 200);
    }

    #[tokio::test]
    async fn test_rest_sensor_status() {
        // Prepare
        let observer = build_mocked_observer().await;
        let routes = routes(&observer).1;
        let register = observer.register_sensor(None).await.unwrap();

        // Execute
        let res = warp::test::request()
            .header("X-KEY", register.key)
            .path(&format!("/api/sensor/{}", register.id as i32))
            .reply(&routes)
            .await;

        // Validate
        assert_eq!(res.status(), 200);
    }
}
