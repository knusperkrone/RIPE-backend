#[macro_use]
extern crate diesel;
#[macro_use]
extern crate slog;

mod agent;
mod error;
mod logging;
mod models;
mod observer;
mod rest;
mod schema;
mod sensor;

#[actix_rt::main]
async fn main() {
    let (sensor_arc, dispatch_mqtt) = observer::ConcurrentSensorObserver::new();

    futures::future::join(rest::dispatch_server(sensor_arc.clone()), dispatch_mqtt).await;
}
