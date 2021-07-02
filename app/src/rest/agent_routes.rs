use super::build_response;
use crate::sensor::ConcurrentSensorObserver;
use chrono_tz::Tz;
use chrono_tz::UTC;
use std::sync::Arc;
use warp::Filter;

pub fn routes(
    observer: &Arc<ConcurrentSensorObserver>,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    get_active_agents(observer.clone())
        .or(register_agent(observer.clone()))
        .or(unregister_agent(observer.clone()))
        .or(on_agent_cmd(observer.clone()))
        .or(agent_config(observer.clone()))
        .or(set_agent_config(observer.clone()))
}

/// GET api/agent
///
/// Show all active agent plugins
///
/// Returns a string list of all names
fn get_active_agents(
    observer: Arc<ConcurrentSensorObserver>,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::any()
        .map(move || observer.clone())
        .and(warp::get())
        .and(warp::path!("api" / "agent"))
        .and_then(|observer: Arc<ConcurrentSensorObserver>| async move {
            let agents = observer.agents().await;
            build_response(Ok(agents))
        })
        .boxed()
}

/// POST api/agent/:id/:pwd
///
/// Register an agent to a sensor
///
/// Returns 200 if the new agent was added
fn register_agent(
    observer: Arc<ConcurrentSensorObserver>,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    let key_header = warp::header::optional::<String>("X-KEY");
    warp::any()
        .map(move || observer.clone())
        .and(warp::post())
        .and(key_header)
        .and(warp::path!("api" / "agent" / i32))
        .and(warp::body::json())
        .and_then(
            |observer: Arc<ConcurrentSensorObserver>,
             key_b64: Option<String>,
             sensor_id: i32,
             body: dto::AgentDto| async move {
                let domain = body.domain;
                let agent_name = body.agent_name;
                let resp = observer
                    .register_agent(sensor_id, key_b64.unwrap_or_default(), domain, agent_name)
                    .await;
                build_response(resp)
            },
        )
        .boxed()
}

/// DELETE api/agent/:id/:pwd
///
/// Unregister an agent to a sensor
///
/// Returns 200 if the new agent was removed
fn unregister_agent(
    observer: Arc<ConcurrentSensorObserver>,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    let key_header = warp::header::optional::<String>("X-KEY");
    warp::any()
        .map(move || observer.clone())
        .and(warp::delete())
        .and(key_header)
        .and(warp::path!("api" / "agent" / i32))
        .and(warp::body::json())
        .and_then(
            |observer: Arc<ConcurrentSensorObserver>,
             key_b64: Option<String>,
             sensor_id: i32,
             body: dto::AgentDto| async move {
                let domain = body.domain;
                let agent_name = body.agent_name;
                let resp = observer
                    .unregister_agent(sensor_id, key_b64.unwrap_or_default(), domain, agent_name)
                    .await;
                build_response(resp)
            },
        )
        .boxed()
}

/// POST api/agent/:id/:pwd/:domain
///
/// Sends an command to an agent
///
/// Returns 200 if the new agent was notified
fn on_agent_cmd(
    observer: Arc<ConcurrentSensorObserver>,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    let key_header = warp::header::optional::<String>("X-KEY");
    warp::any()
        .map(move || observer.clone())
        .and(warp::post())
        .and(key_header)
        .and(warp::path!("api" / "agent" / i32 / String))
        .and(warp::body::json())
        .and_then(
            |observer: Arc<ConcurrentSensorObserver>,
             key_b64: Option<String>,
             sensor_id: i32,
             domain_b64: String,
             body: dto::ForceRequest| async move {
                let resp = observer
                    .on_agent_cmd(
                        sensor_id,
                        key_b64.unwrap_or_default(),
                        decode_b64(domain_b64),
                        body.payload,
                    )
                    .await;
                build_response(resp)
            },
        )
        .boxed()
}

/// GET api/agent/:id/:pwd/:domain/config
///
/// Get the config for an agent
///
/// Returns a HashMap with name and AgentConfigType
fn agent_config(
    observer: Arc<ConcurrentSensorObserver>,
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
            |observer: Arc<ConcurrentSensorObserver>,
             tz_opt: Option<String>,
             key_b64: Option<String>,
             sensor_id: i32,
             domain_b64: String| async move {
                let tz: Tz = tz_opt.unwrap_or("UTC".to_owned()).parse().unwrap_or(UTC);
                let resp = observer
                    .agent_config(
                        sensor_id,
                        key_b64.unwrap_or_default(),
                        decode_b64(domain_b64),
                        tz,
                    )
                    .await;
                build_response(resp)
            },
        )
        .boxed()
}

/// POST api/agent/:id/:pwd/:domain/config
///
/// Sets the config for an agent
///
/// Returns 200, if the HashMap was valid and accepted by the agent
fn set_agent_config(
    observer: Arc<ConcurrentSensorObserver>,
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
            |observer: Arc<ConcurrentSensorObserver>,
             tz_opt: Option<String>,
             key_b64: Option<String>,
             sensor_id: i32,
             domain_b64: String,
             body: _| async move {
                let tz: Tz = tz_opt.unwrap_or("UTC".to_owned()).parse().unwrap_or(UTC);
                let resp = observer
                    .set_agent_config(
                        sensor_id,
                        key_b64.unwrap_or_default(),
                        decode_b64(domain_b64),
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
    if let Ok(payload_bytes) = base64::decode(payload_b64) {
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

    #[derive(Serialize, Deserialize, Debug)]
    pub struct AgentDto {
        pub domain: String,
        pub agent_name: String,
    }

    #[derive(Serialize, Deserialize, Debug)]
    pub struct AgentStatusDto {
        pub domain: String,
        pub agent_name: String,
        pub ui: AgentUI,
    }

    #[derive(Serialize, Deserialize)]
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

    use crate::{config::CONFIG, models::establish_db_connection};

    async fn build_mocked_observer() -> Arc<ConcurrentSensorObserver> {
        let plugin_path = CONFIG.plugin_dir();
        let plugin_dir = std::path::Path::new(&plugin_path);
        let db_conn = establish_db_connection().await.unwrap();
        ConcurrentSensorObserver::new(plugin_dir, db_conn)
    }

    #[tokio::test]
    async fn test_rest_agents() {
        // Prepare
        let observer = build_mocked_observer().await;
        let routes = routes(&observer);

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
            "",
            warp::http::StatusCode::BAD_REQUEST,
        ))
    }

    #[tokio::test]
    async fn test_rest_register_agent() {
        // Prepare
        let observer = build_mocked_observer().await;
        let sensor = observer.register_sensor(None).await.unwrap();
        let routes = routes(&observer).recover(handle_rejection);

        // Execute
        let dto = dto::AgentDto {
            agent_name: "MockAgent".to_owned(),
            domain: "Test".to_owned(),
        };
        let res = warp::test::request()
            .method("POST")
            .path(&format!("/api/agent/{}/{}", sensor.id, sensor.key))
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
        let observer = build_mocked_observer().await;
        let sensor = observer.register_sensor(None).await.unwrap();
        observer
            .register_agent(
                sensor.id,
                sensor.key.clone(),
                domain.clone(),
                agent_name.clone(),
            )
            .await
            .unwrap();
        let routes = routes(&observer).recover(handle_rejection);

        // Execute
        let dto = dto::AgentDto { domain, agent_name };
        let res = warp::test::request()
            .method("DELETE")
            .path(&format!("/api/agent/{}/{}", sensor.id, sensor.key))
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
        let observer = build_mocked_observer().await;
        let sensor = observer.register_sensor(None).await.unwrap();
        observer
            .register_agent(
                sensor.id,
                sensor.key.clone(),
                domain.clone(),
                agent_name.clone(),
            )
            .await
            .unwrap();
        let routes = routes(&observer).recover(handle_rejection);

        // Execute
        let dto = dto::ForceRequest { payload: 1 };
        let res = warp::test::request()
            .method("POST")
            .path(&format!(
                "/api/agent/{}/{}/{}",
                sensor.id, sensor.key, domain
            ))
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
        let observer = build_mocked_observer().await;
        let sensor = observer.register_sensor(None).await.unwrap();
        observer
            .register_agent(
                sensor.id,
                sensor.key.clone(),
                domain.clone(),
                agent_name.clone(),
            )
            .await
            .unwrap();
        let routes = routes(&observer).recover(handle_rejection);

        // Execute
        let dto = dto::ForceRequest { payload: 1 };
        let res = warp::test::request()
            .path(&format!(
                "/api/agent/{}/{}/{}/config",
                sensor.id, sensor.key, domain
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
        let observer = build_mocked_observer().await;
        let sensor = observer.register_sensor(None).await.unwrap();
        observer
            .register_agent(
                sensor.id,
                sensor.key.clone(),
                domain.clone(),
                agent_name.clone(),
            )
            .await
            .unwrap();
        let routes = routes(&observer).recover(handle_rejection);

        // Execute
        let dto = HashMap::<String, String>::new();
        let res = warp::test::request()
            .method("POST")
            .path(&format!(
                "/api/agent/{}/{}/{}/config",
                sensor.id, sensor.key, domain
            ))
            .json(&dto)
            .reply(&routes)
            .await;

        // Validate
        assert_eq!(200, res.status());
    }
}
