use super::build_response;
use crate::sensor::ConcurrentSensorObserver;
use actix_web::{web, HttpResponse};
use std::sync::Arc;

/// Registers an agent to a sensor
///
/// Returns 200 if the new agent was added
async fn agent_register(
    observer: web::Data<Arc<ConcurrentSensorObserver>>,
    register_request: web::Json<dto::AgentRegisterDto>,
    path: web::Path<(i32, String)>,
) -> HttpResponse {
    let (sensor_id, key_b64) = path.into_inner();
    let req = register_request.0;
    let resp = observer.register_agent(sensor_id, key_b64, req).await;
    build_response(resp)
}

/// Unregisters an agent to a sensor
///
/// Returns 200 if the new agent was removed
async fn agent_unregister(
    observer: web::Data<Arc<ConcurrentSensorObserver>>,
    register_request: web::Json<dto::AgentRegisterDto>,
    path: web::Path<(i32, String)>,
) -> HttpResponse {
    let (sensor_id, key_b64) = path.into_inner();
    let req = register_request.0;
    let resp = observer.unregister_agent(sensor_id, key_b64, req).await;
    build_response(resp)
}

/// Show all active agent plugins
///
/// Returns a string list of all names
async fn get_active_agents(observer: web::Data<Arc<ConcurrentSensorObserver>>) -> HttpResponse {
    let agents = observer.agents().await;
    HttpResponse::Ok().json(agents)
}

pub fn config_endpoints(cfg: &mut web::ServiceConfig) {
    cfg.service(web::resource("api/agent").route(web::get().to(get_active_agents)))
        .service(
            web::resource("api/agent/{id}/{key}")
                .route(web::post().to(agent_register))
                .route(web::delete().to(agent_unregister)),
        );
}

///
/// DTO
///
pub mod dto {
    use iftem_core::{AgentMessage, AgentState, AgentUI};
    use serde::{Deserialize, Serialize, Serializer};

    #[derive(Debug, Copy, Clone)]
    pub enum AgentPayload {
        State(AgentState),
        Bool(bool),
        Int(i32),
    }

    impl AgentPayload {
        pub fn from(message: AgentMessage) -> Result<Self, ()> {
            match message {
                AgentMessage::State(s) => Ok(AgentPayload::State(s)),
                AgentMessage::Bool(b) => Ok(AgentPayload::Bool(b)),
                AgentMessage::Int(i) => Ok(AgentPayload::Int(i)),
                AgentMessage::Task(_) => Err(()),
            }
        }
    }

    impl Serialize for AgentPayload {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            match self {
                AgentPayload::Bool(b) => serializer.serialize_bool(*b),
                AgentPayload::Int(i) => serializer.serialize_i32(*i),
                AgentPayload::State(_) => serializer.serialize_none(),
            }
        }
    }

    #[derive(Serialize, Deserialize, Debug)]
    pub struct AgentRegisterDto {
        pub domain: String,
        pub agent_name: String,
    }

    #[derive(Serialize, Deserialize, Debug)]
    pub struct AgentStatusDto {
        pub domain: String,
        pub agent_name: String,
        pub ui: AgentUI,
    }
}

///
/// TEST
///
#[cfg(test)]
mod test {
    use super::*;
    use crate::rest::AgentRegisterDto;
    use actix_web::dev::Service;
    use actix_web::{test, web, App};

    #[actix_rt::test]
    async fn test_register_agent() {
        // prepare
        let sensor_name = "Agent_Register_Sensor";
        let agent_domain = "Water";
        let mock_agent = "MockAgent";
        let observer = ConcurrentSensorObserver::new();
        let register = observer
            .register_new_sensor(&Some(sensor_name.to_owned()))
            .await
            .unwrap();
        let request_json = AgentRegisterDto {
            agent_name: mock_agent.to_owned(),
            domain: agent_domain.to_owned(),
        };

        let mut app = test::init_service(
            App::new()
                .app_data(web::Data::new(observer))
                .configure(super::config_endpoints),
        )
        .await;

        // Execute
        let req = test::TestRequest::post()
            .uri(&format!("/api/agent/{}/{}", register.id, register.key))
            .set_json(&request_json)
            .to_request();
        let resp = app.call(req).await.unwrap();

        // Validate
        assert_eq!(200, resp.status());
    }

    #[actix_rt::test]
    async fn test_unregister_agent() {
        // Prepare
        let sensor_name = "Agent_Unregister_Sensor";
        let agent_domain = "Water";
        let mock_agent = "MockAgent";
        let observer = ConcurrentSensorObserver::new();
        let register = observer
            .register_new_sensor(&Some(sensor_name.to_owned()))
            .await
            .unwrap();
        let register_request = AgentRegisterDto {
            agent_name: mock_agent.to_owned(),
            domain: agent_domain.to_owned(),
        };
        let request_json = AgentRegisterDto {
            agent_name: mock_agent.to_owned(),
            domain: agent_domain.to_owned(),
        };
        observer
            .register_agent(register.id, register.key.clone(), register_request)
            .await
            .unwrap();

        let mut app = test::init_service(
            App::new()
                .app_data(web::Data::new(observer))
                .configure(super::config_endpoints),
        )
        .await;

        // Execute
        let req = test::TestRequest::delete()
            .uri(&format!("/api/agent/{}/{}", register.id, register.key))
            .set_json(&request_json)
            .to_request();
        let resp = app.call(req).await.unwrap();

        // Validate
        assert_eq!(200, resp.status());
    }
}
