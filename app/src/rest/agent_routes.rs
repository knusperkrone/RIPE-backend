use super::{build_response, SwaggerHostDefinition};
use crate::sensor::AgentObserver;
use crate::sensor::ConcurrentObserver;
use chrono_tz::Tz;
use chrono_tz::UTC;
use std::sync::Arc;
use warp::Filter;

pub fn routes(
    observer: &Arc<ConcurrentObserver>,
) -> (
    SwaggerHostDefinition,
    impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone,
) {
    use super::ErrorResponseDto;
    use crate::rest::AgentStatusDto;
    use utoipa::OpenApi;
    #[derive(OpenApi)]
    #[openapi(
        paths(get_active_agents, register_agent, unregister_agent, on_agent_cmd, agent_config, set_agent_config),
        components(schemas(dto::AgentDto, dto::AgentStatusDto, dto::ForceRequest, super::ErrorResponseDto,
            ErrorResponseDto, AgentStatusDto),
        ),
        tags((name = "agent", description = "Agent related API"))
    )]
    struct ApiDoc;
    let agent_observer = AgentObserver::new(observer.clone());

    (
        SwaggerHostDefinition {
            open_api: ApiDoc::openapi(),
        },
        get_active_agents(observer.clone())
            .or(register_agent(agent_observer.clone()))
            .or(unregister_agent(agent_observer.clone()))
            .or(on_agent_cmd(agent_observer.clone()))
            .or(agent_config(agent_observer.clone()))
            .or(set_agent_config(agent_observer.clone()))
            .or(warp::path!("api" / "doc" / "agent-api.json")
                .and(warp::get())
                .map(|| warp::reply::json(&ApiDoc::openapi()))),
    )
}

#[utoipa::path(
    get,
    path = "/api/agent",
    responses(
        (status = 200, description = "Get all active loaded Agents", body = String, content_type = "application/json"),
        (status = 500, description = "Internal error", body = ErrorResponseDto, content_type = "application/json"),
    ),
    tag = "agent",
)]
fn get_active_agents(
    observer: Arc<ConcurrentObserver>,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::any()
        .map(move || observer.clone())
        .and(warp::get())
        .and(warp::path!("api" / "agent"))
        .and_then(|observer: Arc<ConcurrentObserver>| async move {
            let factory = observer.agent_factory.read().await;
            let agents: Vec<String> = factory.agents().drain(..).map(ToOwned::to_owned).collect();
            build_response(Ok(agents))
        })
        .boxed()
}

#[utoipa::path(
    post,
    path = "/api/agent/{id}",
    params(
        ("id" = i32, Path, description = "The sensor id"),
        ("x-key" = String, Header, description = "The sensor key"),
    ),
    request_body(content = AgentDto, description = "The agent to register", content_type = "application/json"),
    responses(
        (status = 200, description = "Register a Agent for provided Sensor", body = AgentDto, content_type = "application/json"),
        (status = 400, description = "Agent not found or invalid credentials", body = ErrorResponseDto, content_type = "application/json"),
        (status = 500, description = "Internal error", body = ErrorResponseDto, content_type = "application/json"),
    ),
    tag = "agent",
)]
fn register_agent(
    observer: AgentObserver,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    let key_header = warp::header::optional::<String>("X-KEY");
    warp::any()
        .map(move || observer.clone())
        .and(warp::post())
        .and(key_header)
        .and(warp::path!("api" / "agent" / i32))
        .and(warp::body::json())
        .and_then(
            |observer: AgentObserver,
             key_b64: Option<String>,
             sensor_id: i32,
             body: dto::AgentDto| async move {
                let domain = body.domain;
                let agent_name = body.agent_name;
                let resp = observer
                    .register(sensor_id, &key_b64.unwrap_or_default(), &domain, &agent_name)
                    .await
                    .map(|_| dto::AgentDto { domain, agent_name });

                build_response(resp)
            },
        )
        .boxed()
}

#[utoipa::path(
    delete,
    path = "/api/agent/{id}",
    params(
        ("id" = i32, Path, description = "The sensor id"),
        ("x-key" = String, Header, description = "The sensor key"),
    ),
    request_body(content = AgentDto, description = "The agent to delete", content_type = "application/json"),
    responses(
        (status = 200, description = "Delete a Agent for provided Sensor", body = AgentDto, content_type = "application/json"),
        (status = 400, description = "Agent not found or invalid credentials", body = ErrorResponseDto, content_type = "application/json"),
        (status = 500, description = "Internal error", body = ErrorResponseDto, content_type = "application/json"),
    ),
    tag = "agent",
)]
fn unregister_agent(
    observer: AgentObserver,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    let key_header = warp::header::optional::<String>("X-KEY");
    warp::any()
        .map(move || observer.clone())
        .and(warp::delete())
        .and(key_header)
        .and(warp::path!("api" / "agent" / i32))
        .and(warp::body::json())
        .and_then(
            |observer: AgentObserver,
             key_b64: Option<String>,
             sensor_id: i32,
             body: dto::AgentDto| async move {
                let domain = body.domain;
                let agent_name = body.agent_name;
                let resp = observer
                    .unregister(sensor_id, key_b64.unwrap_or_default(), &domain, &agent_name)
                    .await
                    .map(|()| dto::AgentDto { domain, agent_name });
                build_response(resp)
            },
        )
        .boxed()
}

#[utoipa::path(
    post,
    path = "/api/agent/{id}/{domain}",
    tag = "agent",
    params(
        ("id" = i32, Path, description = "The sensor id"),
        ("domain" = String, Path, description = "The domain"),
        ("x-key" = String, Header, description = "The sensor key"),
    ),
    request_body(content = ForceRequest, description = "The payload for the agend", content_type = "application/json"),
    responses(
        (status = 200, description = "Set forced command for Agent", content_type = "application/json"),
        (status = 400, description = "Agent not found or invalid credentials", body = ErrorResponseDto, content_type = "application/json"),
        (status = 500, description = "Internal error", body = ErrorResponseDto, content_type = "application/json"),
    ),
)]
fn on_agent_cmd(
    observer: AgentObserver,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    let key_header = warp::header::optional::<String>("X-KEY");
    warp::any()
        .map(move || observer.clone())
        .and(warp::post())
        .and(key_header)
        .and(warp::path!("api" / "agent" / i32 / String))
        .and(warp::body::json())
        .and_then(
            |observer: AgentObserver,
             key_b64: Option<String>,
             sensor_id: i32,
             domain_b64: String,
             body: dto::ForceRequest| async move {
                //let key_b64 = Some("".to_owned());
                let resp = observer
                    .on_cmd(
                        sensor_id,
                        key_b64.unwrap_or_default(),
                        &decode_b64(domain_b64),
                        body.payload,
                    )
                    .await;
                build_response(resp)
            },
        )
        .boxed()
}

#[utoipa::path(
    get,
    path = "/api/agent/{id}/{domain}/config",
    params(
        ("id" = i32, Path, description = "The sensor id"),
        ("domain" = String, Path, description = "The domain"),
        ("x-key" = String, Header, description = "The sensor key"),
        ("x-tz" = Option<String>, Header, description = "The timezone to format displayed text into"),
    ),
    responses(
        (status = 200, description = "Get the config map for an agent", content_type = "application/json"),
        (status = 400, description = "Agent not found or invalid credentials", body = ErrorResponseDto, content_type = "application/json"),
        (status = 500, description = "Internal error", body = ErrorResponseDto, content_type = "application/json"),
    ),
    tag = "agent",
)]
fn agent_config(
    observer: AgentObserver,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    let timezone_header = warp::header::optional::<String>("X-TZ");
    let key_header = warp::header::optional::<String>("X-KEY");
    warp::any()
        .map(move || observer.clone())
        .and(warp::get())
        .and(timezone_header)
        .and(key_header)
        .and(warp::path!("api" / "agent" / i32 / String / "config"))
        .and_then(
            |observer: AgentObserver,
             tz_opt: Option<String>,
             key_b64: Option<String>,
             sensor_id: i32,
             domain_b64: String| async move {
                let tz: Tz = tz_opt.unwrap_or("UTC".to_owned()).parse().unwrap_or(UTC);
                let resp = observer
                    .config(
                        sensor_id,
                        key_b64.unwrap_or_default(),
                        &decode_b64(domain_b64),
                        tz,
                    )
                    .await;
                build_response(resp)
            },
        )
        .boxed()
}

#[utoipa::path(
    post,
    path = "/api/agent/{id}/{domain}/config",
    params(
        ("id" = i32, Path, description = "The sensor id"),
        ("domain" = String, Path, description = "The domain"),
        ("x-key" = String, Header, description = "The sensor key"),
        ("x-tz" = Option<String>, Header, description = "The timezone to format displayed text into"),
    ),
    request_body(content = String, description = "The payload for the agend", content_type = "application/json"),
    responses(
        (status = 200, description = "Set the fetched config Map for an agent", content_type = "application/json"),
        (status = 400, description = "Agent not found or invalid credentials", body = ErrorResponseDto, content_type = "application/json"),
        (status = 500, description = "Internal error", body = ErrorResponseDto, content_type = "application/json"),
    ),
    tag = "agent",
)]
fn set_agent_config(
    observer: AgentObserver,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    let timezone_header = warp::header::optional::<String>("X-TZ");
    let key_header = warp::header::optional::<String>("X-KEY");
    warp::any()
        .map(move || observer.clone())
        .and(warp::post())
        .and(timezone_header)
        .and(key_header)
        .and(warp::path!("api" / "agent" / i32 / String / "config"))
        .and(warp::body::json())
        .and_then(
            |observer: AgentObserver,
             tz_opt: Option<String>,
             key_b64: Option<String>,
             sensor_id: i32,
             domain_b64: String,
             body: _| async move {
                let tz: Tz = tz_opt.unwrap_or("UTC".to_owned()).parse().unwrap_or(UTC);
                let resp = observer
                    .set_config(
                        sensor_id,
                        key_b64.unwrap_or_default(),
                        &decode_b64(domain_b64),
                        body,
                        tz,
                    )
                    .await;
                build_response(resp)
            },
        )
        .boxed()
}

fn decode_b64(payload_b64: String) -> String {
    use base64::{engine::general_purpose, Engine as _};
    if let Ok(payload_bytes) = general_purpose::STANDARD.decode(payload_b64) {
        if let Ok(payload) = String::from_utf8(payload_bytes) {
            return payload;
        }
    }
    String::default()
}

///
/// DTO
///
pub mod dto {
    use ripe_core::AgentUI;
    use serde::{Deserialize, Serialize};
    use utoipa::ToSchema;

    use crate::sensor::observer::Agent;

    #[derive(Serialize, Deserialize, Debug, ToSchema)]
    pub struct AgentDto {
        pub domain: String,
        pub agent_name: String,
    }

    #[derive(Serialize, Deserialize, Debug, ToSchema)]
    pub struct AgentStatusDto {
        pub domain: String,
        pub agent_name: String,
        pub ui: AgentUI,
    }

    impl From<Agent> for AgentStatusDto {
        fn from(other: Agent) -> Self {
            AgentStatusDto {
                domain: other.domain,
                agent_name: other.name,
                ui: other.ui,
            }
        }
    }

    #[derive(Serialize, Deserialize, ToSchema)]
    pub struct ForceRequest {
        pub payload: i64,
    }
}

///
/// TEST
///
#[cfg(test)]
mod test {
    use super::*;
    use std::collections::HashMap;

    fn encode_b64(payload: String) -> String {
        use base64::{engine::general_purpose, Engine as _};
        general_purpose::STANDARD.encode(payload)
    }

    use crate::{
        config::CONFIG, models::establish_db_connection, sensor::observer::sensor::SensorObserver,
    };

    async fn build_mocked_observer() -> (Arc<ConcurrentObserver>, AgentObserver, SensorObserver) {
        let plugin_path = CONFIG.plugin_dir();
        let plugin_dir = std::path::Path::new(&plugin_path);
        let db_conn = establish_db_connection().await.unwrap();
        let observer = ConcurrentObserver::new(plugin_dir, db_conn);
        let agent_observer = AgentObserver::new(observer.clone());
        let sensor_observer = SensorObserver::new(observer.clone());
        (observer, agent_observer, sensor_observer)
    }

    #[tokio::test]
    async fn test_rest_agents() {
        // Prepare
        let (observer, _agent_observer, _sensor_observer) = build_mocked_observer().await;
        let routes = routes(&observer).1;

        // Execute
        let res = warp::test::request()
            .path("/api/agent")
            .reply(&routes)
            .await;

        // Validate
        assert_eq!(200, res.status());
        let _: Vec<String> = serde_json::from_slice(res.body()).unwrap();
    }

    async fn handle_rejection(
        err: warp::Rejection,
    ) -> Result<impl warp::Reply, std::convert::Infallible> {
        println!("ERR: {:?}", err);
        Ok(warp::reply::with_status(
            format!("ERR: {:?}", err),
            warp::http::StatusCode::BAD_REQUEST,
        ))
    }

    #[tokio::test]
    async fn test_rest_register_agent() {
        // Prepare
        let (observer, _agent_observer, sensor_observer) = build_mocked_observer().await;
        let sensor = sensor_observer.register(None).await.unwrap();
        let routes = routes(&observer).1.recover(handle_rejection);

        // Execute
        let dto = dto::AgentDto {
            agent_name: "MockAgent".to_owned(),
            domain: "Test".to_owned(),
        };
        let res = warp::test::request()
            .method("POST")
            .header("X-KEY", sensor.key)
            .path(&format!("/api/agent/{}", sensor.id))
            .json(&dto)
            .reply(&routes)
            .await;

        // Validate
        assert_eq!(200, res.status());
    }

    #[tokio::test]
    async fn test_rest_unregister_agent() {
        // Prepare
        let agent_name = "MockAgent".to_owned();
        let domain = "Test".to_owned();
        let (observer, agent_observer, sensor_observer) = build_mocked_observer().await;
        let sensor = sensor_observer.register(None).await.unwrap();
        agent_observer
            .register(sensor.id, sensor.key.clone(), &domain, &agent_name)
            .await
            .unwrap();
        let routes = routes(&observer).1.recover(handle_rejection);

        // Execute
        let dto = dto::AgentDto { domain, agent_name };
        let res = warp::test::request()
            .method("DELETE")
            .header("X-KEY", sensor.key)
            .path(&format!("/api/agent/{}", sensor.id))
            .json(&dto)
            .reply(&routes)
            .await;

        // Validate
        assert_eq!(200, res.status());
    }

    #[tokio::test]
    async fn test_rest_agent_cmd() {
        // Prepare
        let agent_name = "MockAgent".to_owned();
        let domain = "Test".to_owned();
        let (observer, agent_observer, sensor_observer) = build_mocked_observer().await;
        let sensor = sensor_observer.register(None).await.unwrap();
        agent_observer
            .register(sensor.id, sensor.key.clone(), &domain, &agent_name)
            .await
            .unwrap();
        let routes = routes(&observer).1.recover(handle_rejection);

        // Execute
        let dto = dto::ForceRequest { payload: 1 };
        let res = warp::test::request()
            .method("POST")
            .header("X-KEY", sensor.key)
            .path(&format!("/api/agent/{}/{}", sensor.id, encode_b64(domain)))
            .json(&dto)
            .reply(&routes)
            .await;

        // Validate
        assert_eq!(200, res.status());
    }

    #[tokio::test]
    async fn test_rest_agent_config() {
        // Prepare
        let agent_name = "MockAgent".to_owned();
        let domain = "Test".to_owned();
        let (observer, agent_observer, sensor_observer) = build_mocked_observer().await;
        let sensor = sensor_observer.register(None).await.unwrap();
        agent_observer
            .register(sensor.id, sensor.key.clone(), &domain, &agent_name)
            .await
            .unwrap();
        let routes = routes(&observer).1.recover(handle_rejection);

        // Execute
        let dto = dto::ForceRequest { payload: 1 };
        let res = warp::test::request()
            .header("X-KEY", sensor.key)
            .path(&format!(
                "/api/agent/{}/{}/config",
                sensor.id,
                encode_b64(domain)
            ))
            .json(&dto)
            .reply(&routes)
            .await;

        // Validate
        assert_eq!(200, res.status());
    }

    #[tokio::test]
    async fn test_rest_set_agent_config() {
        // Prepare
        let agent_name = "MockAgent".to_owned();
        let domain = "Test".to_owned();
        let (observer, agent_observer, sensor_observer) = build_mocked_observer().await;
        let sensor = sensor_observer.register(None).await.unwrap();
        agent_observer
            .register(sensor.id, sensor.key.clone(), &domain, &agent_name)
            .await
            .unwrap();
        let routes = routes(&observer).1.recover(handle_rejection);

        // Execute
        let dto = HashMap::<String, String>::new();
        let res = warp::test::request()
            .method("POST")
            .header("X-KEY", sensor.key)
            .path(&format!(
                "/api/agent/{}/{}/config",
                sensor.id,
                encode_b64(domain)
            ))
            .json(&dto)
            .reply(&routes)
            .await;

        // Validate
        assert_eq!(200, res.status());
    }
}
