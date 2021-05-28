#1 BUILD app
FROM rust:1.52 as build
RUN apt-get update && \ 
        apt-get install -y cmake npm

WORKDIR /ripe

#1.1a crate mock crates
RUN USER=root cargo new --bin app
RUN USER=root cargo new --lib core

#1.1b copy actual crate manifests
COPY ./Cargo.lock ./app/Cargo.lock
COPY ./app/Cargo.toml ./app/Cargo.toml
COPY ./Cargo.lock ./core/Cargo.lock
COPY ./core/Cargo.toml ./core/Cargo.toml

#1.1c cache dependencies
WORKDIR /ripe/app
RUN cargo build --release

#1.2a copy actual sources
WORKDIR /ripe
RUN rm ./app/src/*.rs
RUN rm ./core/src/*.rs
COPY ./app/src ./app/src
COPY ./core/src ./core/src
COPY ./core/build.rs ./core/build.rs

#1.2b copy migrations
WORKDIR /ripe/app
COPY migrations ./migrations

#1.2c build app for release
RUN cargo build --release

#1.3 build plugins for release
WORKDIR /ripe
COPY ./plugins ./plugins
WORKDIR /ripe/plugins
RUN cargo build --release

#2 RUN
FROM debian:buster-slim
RUN apt-get update && apt-get install -y openssl libpq-dev

WORKDIR /app
COPY --from=build /ripe/app/target/release/ripe .
COPY --from=build /ripe/plugins/target/release/*.so ./plugins/
COPY --from=build /ripe/plugins/target/release/*.wasm ./plugins/
COPY .env-docker .env

ENV RUST_BACKTRACE=full
CMD ["/app/ripe"]
