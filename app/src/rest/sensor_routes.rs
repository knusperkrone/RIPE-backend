use super::{build_response, SwaggerHostDefinition};
use crate::error;
use crate::rest::AgentStatusDto;
use crate::sensor::observer::controller::SensorObserver;
use crate::sensor::ConcurrentObserver;
use chrono::DateTime;
use chrono_tz::Tz;
use chrono_tz::UTC;
use ripe_core::SensorDataMessage;
use std::sync::Arc;
use warp::hyper::body::Bytes;
use warp::Filter;

pub fn routes(
    observer: &Arc<ConcurrentObserver>,
) -> (
    SwaggerHostDefinition,
    impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone,
) {
    use super::ErrorResponseDto;
    use ripe_core::{AgentState, AgentUI, AgentUIDecorator};
    use utoipa::OpenApi;
    #[derive(OpenApi)]
    #[openapi(
        paths(register_sensor, unregister_sensor, sensor_status, add_sensor_logs, sensor_logs, add_sensor_data, sensor_data_within, first_sensor_data, sensor_reload),
        components(schemas(dto::SensorRegisterRequestDto, dto::SensorCredentialDto, dto::SensorStatusDto, dto::SensorDataDto, dto::BrokersDto,
            dto::SensorStatusDto, AgentStatusDto, ErrorResponseDto, AgentUI, AgentUIDecorator, AgentState
        )),
        tags((name = "sensor", description = "Sensor related API"))
    )]
    struct ApiDoc;
    let sensor_observer = SensorObserver::new(observer.clone());

    (
        SwaggerHostDefinition {
            open_api: ApiDoc::openapi(),
        },
        register_sensor(sensor_observer.clone())
            .or(unregister_sensor(sensor_observer.clone()))
            .or(sensor_status(sensor_observer.clone()))
            .or(add_sensor_logs(sensor_observer.clone()))
            .or(sensor_logs(sensor_observer.clone()))
            .or(add_sensor_data(sensor_observer.clone()))
            .or(sensor_data_within(sensor_observer.clone()))
            .or(first_sensor_data(sensor_observer))
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
        (status = 200, description = "The freshly registered sensor, store the id and key safely", body = SensorCredentialDto, content_type = "application/json"),
        (status = 400, description = "Invalid request body", body = ErrorResponseDto, content_type = "application/json"),
        (status = 500, description = "Internal error", body = ErrorResponseDto, content_type = "application/json"),
    ),
    tag = "sensor",
)]
fn register_sensor(
    observer: SensorObserver,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::any()
        .map(move || observer.clone())
        .and(warp::post())
        .and(warp::path!("api" / "sensor"))
        .and(warp::body::json())
        .and_then(
            |observer: SensorObserver, body: dto::SensorRegisterRequestDto| async move {
                let resp =
                    observer
                        .register(body.name)
                        .await
                        .map(|sensor| dto::SensorCredentialDto {
                            id: sensor.id,
                            key: sensor.key,
                            broker: observer.brokers().unwrap_or(&vec![]).clone().into(),
                        });
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
        (status = 200, description = "Delete a sensor", body = SensorCredentialDto, content_type = "application/json"),
        (status = 400, description = "Agent not found or invalid credentials", body = ErrorResponseDto, content_type = "application/json"),
        (status = 500, description = "Internal error", body = ErrorResponseDto, content_type = "application/json"),
    ),
    tag = "sensor",
)]
fn unregister_sensor(
    observer: SensorObserver,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::any()
        .map(move || observer.clone())
        .and(warp::delete())
        .and(warp::path!("api" / "sensor"))
        .and(warp::body::json())
        .and_then(
            |observer: SensorObserver, body: dto::SensorCredentialDto| async move {
                let resp = observer.unregister(body.id, &body.key).await.map(|()| {
                    dto::SensorCredentialDto {
                        id: body.id,
                        key: body.key,
                        broker: dto::BrokersDto { items: vec![] },
                    }
                });
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
    observer: SensorObserver,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    let timezone_header = warp::header::optional::<String>("X-TZ");
    warp::any()
        .map(move || observer.clone())
        .and(warp::get())
        .and(timezone_header)
        .and(warp::header::<String>("X-KEY"))
        .and(warp::path!("api" / "sensor" / i32))
        .and_then(
            |observer: SensorObserver,
             tz_opt: Option<String>,
             key_b64: String,
             sensor_id: i32| async move {
                let tz: Tz = tz_opt.unwrap_or("UTC".to_owned()).parse().unwrap_or(UTC);
                let resp = observer
                    .status(sensor_id, key_b64, tz)
                    .await
                    .map(|(data, mut agents)| dto::SensorStatusDto {
                        data: dto::SensorDataDto::from(data),
                        agents: agents.drain(..).map(AgentStatusDto::from).collect(),
                        broker: observer.brokers().unwrap_or(&vec![]).clone().into(),
                    });
                build_response(resp)
            },
        )
        .boxed()
}

#[utoipa::path(
    post,
    path = "/api/sensor/{id}/data",
    params(
        ("id" = i32, Path, description = "The sensor id"),
        ("x-key" = String, Header, description = "The sensor key"),
    ),
    responses(
        (status = 200, description = "The sensor data was updated"),
        (status = 400, description = "Agent not found or invalid credentials", body = ErrorResponseDto, content_type = "application/json"),
        (status = 500, description = "Internal error", body = ErrorResponseDto, content_type = "application/json"),
    ),
    tag = "sensor",
)]
fn add_sensor_logs(
    observer: SensorObserver,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::any()
        .map(move || observer.clone())
        .and(warp::post())
        .and(warp::header::<String>("X-KEY"))
        .and(warp::path!("api" / "sensor" / i32 / "data"))
        .and(warp::body::bytes())
        .and_then(
            |observer: SensorObserver, key_b64: String, sensor_id: i32, body: Bytes| async move {
                if let Ok(msg) = String::from_utf8(body.to_vec()) {
                    let res = observer.add_log(sensor_id, &key_b64, msg).await;
                    build_response(res)
                } else {
                    build_response(Ok(()))
                }
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
    observer: SensorObserver,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    let timezone_header = warp::header::optional::<String>("X-TZ");
    warp::any()
        .map(move || observer.clone())
        .and(warp::get())
        .and(timezone_header)
        .and(warp::header::<String>("X-KEY"))
        .and(warp::path!("api" / "sensor" / i32 / "log"))
        .and_then(
            |observer: SensorObserver,
             tz_opt: Option<String>,
             key_b64: String,
             sensor_id: i32| async move {
                let tz: Tz = tz_opt.unwrap_or("UTC".to_owned()).parse().unwrap_or(UTC);
                let resp: Result<Vec<String>, _> = observer
                    .logs(sensor_id, &key_b64, tz)
                    .await;

                build_response(resp)
            },
        )
        .boxed()
}

#[derive(serde::Serialize, serde::Deserialize)]
struct DataQuery {
    from: DateTime<chrono::Utc>,
    until: DateTime<chrono::Utc>,
}

#[utoipa::path(
    post,
    path = "/api/sensor/{id}/data",
    params(
        ("id" = i32, Path, description = "The sensor id"),
        ("x-key" = String, Header, description = "The sensor key"),
    ),
    responses(
        (status = 200, description = "The sensor data was updated"),
        (status = 400, description = "Agent not found or invalid credentials", body = ErrorResponseDto, content_type = "application/json"),
        (status = 500, description = "Internal error", body = ErrorResponseDto, content_type = "application/json"),
    ),
    tag = "sensor",
)]
fn add_sensor_data(
    observer: SensorObserver,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::any()
        .map(move || observer.clone())
        .and(warp::post())
        .and(warp::header::<String>("X-KEY"))
        .and(warp::path!("api" / "sensor" / i32 / "data"))
        .and(warp::body::json())
        .and_then(
            |observer: SensorObserver,
             key_b64: String,
             sensor_id: i32,
             body: SensorDataMessage| async move {
                let res = observer.add_data(sensor_id, &key_b64, body).await;
                build_response(res)
            },
        )
        .boxed()
}

#[utoipa::path(
    get,
    path = "/api/sensor/{id}/data",
    params(
        ("id" = i32, Path, description = "The sensor id"),
        ("x-key" = String, Header, description = "The sensor key"),
        ("from" = DateTime, Query, description = "The date to start"),
        ("until" = DateTime, Query, description = "The date to end"),
    ),
    responses(
        (status = 200, description = "The sensor data of a day", body = [SensorDataDto], content_type = "application/json"),
        (status = 400, description = "Agent not found or invalid credentials", body = ErrorResponseDto, content_type = "application/json"),
        (status = 500, description = "Internal error", body = ErrorResponseDto, content_type = "application/json"),
    ),
    tag = "sensor",
)]
fn sensor_data_within(
    observer: SensorObserver,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::any()
        .map(move || observer.clone())
        .and(warp::get())
        .and(warp::header::<String>("X-KEY"))
        .and(warp::path!("api" / "sensor" / i32 / "data"))
        .and(warp::query())
        .and_then(
            |observer: SensorObserver,
             key_b64: String,
             sensor_id: i32,
             query: DataQuery| async move {
                if query.from >= query.until || query.until - query.from > chrono::Duration::days(1)
                {
                    return build_response(Err(error::ObserverError::from(
                        error::ApiError::ArgumentError(),
                    )));
                }
                let resp: Result<Vec<dto::SensorDataDto>, _> = observer
                    .data(
                        sensor_id,
                        &key_b64,
                        query.from,
                        query.until,
                    )
                    .await;
                build_response(resp)
            },
        )
        .boxed()
}

#[utoipa::path(
    get,
    path = "/api/sensor/{id}/data/first",
    params(
        ("id" = i32, Path, description = "The sensor id"),
        ("x-key" = String, Header, description = "The sensor key"),
    ),
    responses(
        (status = 200, description = "The first sensor data of a sensor", body = Option<SensorDataDto>, content_type = "application/json"),
        (status = 400, description = "Agent not found or invalid credentials", body = ErrorResponseDto, content_type = "application/json"),
        (status = 500, description = "Internal error", body = ErrorResponseDto, content_type = "application/json"),
    ),
    tag = "sensor",
)]
fn first_sensor_data(
    observer: SensorObserver,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::any()
        .map(move || observer.clone())
        .and(warp::get())
        .and(warp::header::<String>("X-KEY"))
        .and(warp::path!("api" / "sensor" / i32 / "data" / "first"))
        .and_then(
            |observer: SensorObserver, key_b64: String, sensor_id: i32| async move {
                let resp: Result<Option<dto::SensorDataDto>, _> =
                    observer.first_data(sensor_id, &key_b64).await;
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
    observer: Arc<ConcurrentObserver>,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::any()
        .map(move || observer.clone())
        .and(warp::post())
        .and(warp::header::<String>("X-KEY"))
        .and(warp::path!("api" / "sensor" / i32 / "reload"))
        .and_then(
            |_observer: Arc<ConcurrentObserver>, _key_b64: String, _sensor_id: i32| async move {
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
    use crate::{models::dao::SensorDataDao, mqtt::MqttScheme, rest::AgentStatusDto};
    use ripe_core::SensorDataMessage;
    use serde::{Deserialize, Serialize};
    use utoipa::ToSchema;

    #[derive(Debug, Serialize, Deserialize, ToSchema)]
    pub struct SensorRegisterRequestDto {
        pub name: Option<String>,
    }

    #[derive(Debug, Serialize, Deserialize, ToSchema)]
    pub struct BrokerCredentialsDto {
        pub username: String,
        pub password: String,
    }

    #[derive(Debug, Serialize, Deserialize, ToSchema)]
    pub struct BrokerDto {
        scheme: MqttScheme,
        host: String,
        port: u16,
        credentials: Option<BrokerCredentialsDto>,
    }

    #[derive(Debug, Serialize, Deserialize, ToSchema)]
    pub struct BrokersDto {
        pub items: Vec<BrokerDto>,
    }

    impl From<Vec<crate::mqtt::MqttBroker>> for BrokersDto {
        fn from(mut from: Vec<crate::mqtt::MqttBroker>) -> Self {
            BrokersDto {
                items: from
                    .drain(..)
                    .map(|b| BrokerDto {
                        scheme: b.connection.scheme,
                        host: b.connection.host,
                        port: b.connection.port,
                        credentials: b.credentials.map(|c| BrokerCredentialsDto {
                            username: c.username,
                            password: c.password,
                        }),
                    })
                    .collect(),
            }
        }
    }

    #[derive(Debug, Serialize, Deserialize, ToSchema)]
    pub struct SensorCredentialDto {
        pub id: i32,
        pub key: String,
        pub broker: BrokersDto,
    }

    #[derive(Debug, Serialize, Deserialize, ToSchema)]
    pub struct SensorStatusDto {
        pub data: SensorDataDto,
        pub agents: Vec<AgentStatusDto>,
        pub broker: BrokersDto,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct PageDto<T> {
        page: u32,
        total_pages: u32,
        payload: T,
    }

    #[derive(Debug, Serialize, Deserialize, ToSchema)]
    pub struct SensorDataDto {
        pub timestamp: chrono::DateTime<chrono::Utc>,
        pub battery: Option<f64>,
        pub moisture: Option<f64>,
        pub temperature: Option<f64>,
        pub carbon: Option<i32>,
        pub conductivity: Option<i32>,
        pub light: Option<i32>,
    }

    impl From<SensorDataMessage> for SensorDataDto {
        fn from(other: SensorDataMessage) -> Self {
            SensorDataDto {
                timestamp: other.timestamp,
                battery: other.battery,
                moisture: other.moisture,
                temperature: other.temperature,
                carbon: other.carbon,
                conductivity: other.conductivity,
                light: other.light,
            }
        }
    }

    impl From<SensorDataDao> for SensorDataDto {
        fn from(other: SensorDataDao) -> Self {
            SensorDataDto {
                timestamp: chrono::DateTime::from_naive_utc_and_offset(
                    other.timestamp,
                    chrono::Utc,
                ),
                battery: other.battery,
                moisture: other.moisture,
                temperature: other.temperature,
                carbon: other.carbon,
                conductivity: other.conductivity,
                light: other.light,
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{config::CONFIG, models::establish_db_connection};

    async fn build_mocked_observer() -> (Arc<ConcurrentObserver>, SensorObserver) {
        let plugin_path = CONFIG.plugin_dir();
        let plugin_dir = std::path::Path::new(&plugin_path);
        let db_conn = establish_db_connection().await.unwrap();
        let observer = ConcurrentObserver::new(plugin_dir, db_conn);
        let sensor_observer = SensorObserver::new(observer.clone());
        (observer, sensor_observer)
    }

    #[tokio::test]
    async fn test_rest_register_sensor() {
        // Prepare
        let (observer, _sensor_observer) = build_mocked_observer().await;
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
        let (observer, sensor_observer) = build_mocked_observer().await;
        let routes = routes(&observer).1;
        let sensor = sensor_observer.register(None).await.unwrap();

        // Execute
        let res = warp::test::request()
            .path("/api/sensor")
            .method("DELETE")
            .json(&dto::SensorCredentialDto {
                id: sensor.id,
                key: sensor.key,
                broker: dto::BrokersDto {
                    tcp: None,
                    wss: None,
                },
            })
            .reply(&routes)
            .await;

        // Validate
        assert_eq!(res.status(), 200);
    }

    #[tokio::test]
    async fn test_rest_sensor_status() {
        // Prepare
        let (observer, sensor_observer) = build_mocked_observer().await;
        let routes = routes(&observer).1;
        let register = sensor_observer.register(None).await.unwrap();

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
