#1 BUILD app
FROM rust:1.52 as build
RUN apt-get update && \ 
        apt-get install -y cmake

WORKDIR /iftem

#1.1a crate mock crates
RUN USER=root cargo new --bin app
RUN USER=root cargo new --lib core

#1.1b copy actual crate manifests
COPY ./Cargo.lock ./app/Cargo.lock
COPY ./app/Cargo.toml ./app/Cargo.toml
COPY ./Cargo.lock ./core/Cargo.lock
COPY ./core/Cargo.toml ./core/Cargo.toml

#1.1c cache dependencies
WORKDIR /iftem/app
RUN cargo build --release

#1.2a copy actual sources
WORKDIR /iftem
RUN rm ./app/src/*.rs
RUN rm ./core/src/*.rs
COPY ./app/src ./app/src
COPY ./core/src ./core/src
COPY ./core/build.rs ./core/build.rs

#1.2b build app for release
WORKDIR /iftem/app
RUN cargo build --release

#1.3 build plugins for release
WORKDIR /iftem
COPY ./plugins ./plugins
WORKDIR /iftem/plugins
RUN cargo build --release

#2 RUN
FROM debian:buster-slim
RUN apt-get update && apt-get install -y openssl libpq-dev

WORKDIR /app
COPY --from=build /iftem/app/target/release/iftem .
COPY --from=build /iftem/plugins/target/release/*.so ./plugins/
COPY .env-docker .env

ENV RUST_LOG=info
ENV RUST_BACKTRACE=full
CMD ["/app/iftem"]
