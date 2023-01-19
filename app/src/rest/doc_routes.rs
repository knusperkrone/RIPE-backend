use super::SwaggerHostDefinition;
use std::sync::Arc;
use utoipa::openapi::path::PathsBuilder;
use utoipa::openapi::schema::ComponentsBuilder;
use utoipa::openapi::tag::Tag;
use utoipa::openapi::OpenApiBuilder;
use utoipa_swagger_ui::Config;
use warp::Filter;
use warp::{
    http::Uri,
    hyper::{Response, StatusCode},
    path::{FullPath, Tail},
    Rejection, Reply,
};

pub fn swagger(
    mut spec_defs: Vec<SwaggerHostDefinition>,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    let builder = OpenApiBuilder::new();
    let mut path_configs: Vec<String> = vec!["api.json".to_owned()];
    let mut tags: Vec<Tag> = Vec::new();
    let mut paths = PathsBuilder::new();
    let mut components = ComponentsBuilder::new();

    for spec_def in spec_defs.drain(..) {
        let spec = &spec_def.openApi;
        path_configs.push(spec_def.url);

        for tag in spec.tags.iter() {
            tags.append(&mut tag.clone());
        }
        for (key, value) in spec.paths.paths.iter() {
            paths = paths.path(key, value.clone());
        }
        if let Some(spec_components) = &spec.components {
            for (key, value) in spec_components.schemas.iter() {
                components = components.schema(key, value.clone());
            }
            for (key, value) in spec_components.responses.iter() {
                components = components.response(key, value.clone());
            }
            for (key, value) in spec_components.security_schemes.iter() {
                components = components.security_scheme(key, value.clone());
            }
        }
    }

    let merged_api = builder
        .tags(Some(tags))
        .paths(paths.build())
        .components(Some(components.build()))
        .build();
    let config = Arc::new(Config::new(path_configs));

    warp::path!("api" / "doc" / "api.json")
        .and(warp::get())
        .map(move || warp::reply::json(&merged_api))
        .or(warp::path("api")
            .and(warp::path("doc"))
            .and(warp::get())
            .and(warp::path::full())
            .and(warp::path::tail())
            .and(warp::any().map(move || config.clone()))
            .and_then(serve_swagger))
}

async fn serve_swagger(
    full_path: FullPath,
    tail: Tail,
    config: Arc<Config<'static>>,
) -> Result<Box<dyn Reply + 'static>, Rejection> {
    if full_path.as_str() == "/api/doc" {
        return Ok(Box::new(warp::redirect::found(Uri::from_static(
            "/api/doc/",
        ))));
    }

    let path = tail.as_str();
    match utoipa_swagger_ui::serve(path, config) {
        Ok(file) => {
            if let Some(file) = file {
                Ok(Box::new(
                    Response::builder()
                        .header("Content-Type", file.content_type)
                        .body(file.bytes),
                ))
            } else {
                Ok(Box::new(StatusCode::NOT_FOUND))
            }
        }
        Err(error) => Ok(Box::new(
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(error.to_string()),
        )),
    }
}
