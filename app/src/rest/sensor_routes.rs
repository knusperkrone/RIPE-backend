use super::build_response;
use crate::sensor::ConcurrentSensorObserver;
use chrono_tz::Tz;
use chrono_tz::UTC;
use std::sync::Arc;
use warp::Filter;

pub fn routes(
    observer: &Arc<ConcurrentSensorObserver>,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    register_sensor(observer.clone())
        .or(unregister_sensor(observer.clone()))
        .or(sensor_status(observer.clone()))
        .or(sensor_logs(observer.clone()))
        .or(sensor_reload(observer.clone()))
}

/// POST /api/sensor
///
/// Register a new sensor
///
/// Returns a `SensorRegisterResponseDto` which contains the
/// assigned sensor_id and it's api key, which should be stored safely
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

/// DELETE /api/sensor
///
/// Unregister a sensor
///
/// Returns 200 if the sensor got unregistered
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

/// GET api/sensor/:id/:key
///
/// Fetch a sensor status
///
/// Returns a `SensorStatusDto`, which holds the sensor
/// - name
/// - values
/// - server rendered UI
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

/// GET api/sensor/log/:id/:key
///
/// Fetch a sensor logs
///
/// Returns a Array of log messages prefix with the timestamp
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

/// POST api/sensor/:id/:key/reload
///
/// Reload the plugins of a sensor
/// Only possible, if all agents are inactive
///
/// Returns 200 if the sensor got reloaded
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

    #[derive(Debug, Serialize, Deserialize)]
    pub struct SensorRegisterRequestDto {
        pub name: Option<String>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct BrokerDto {
        pub tcp: Option<String>,
        pub wss: Option<String>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct SensorCredentialDto {
        pub id: i32,
        pub key: String,
        pub broker: BrokerDto,
    }

    #[derive(Debug, Serialize, Deserialize)]
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
        let routes = routes(&observer);

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
        let routes = routes(&observer);
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
        let routes = routes(&observer);
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
