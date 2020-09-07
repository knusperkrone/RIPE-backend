#1 BUILD
FROM rust:1.45 as build
RUN USER=root cargo new --bin iftem
RUN /bin/sh -c 'apt-get update && apt-get install -y cmake'

RUN USER=root cargo new --bin iftem/app
RUN USER=root cargo new --bin iftem/core
WORKDIR /iftem

# copy over your manifests
COPY ./app/Cargo.lock ./app/Cargo.lock
COPY ./app/Cargo.toml ./app/Cargo.toml
COPY ./core/Cargo.lock ./core/Cargo.lock
COPY ./core/Cargo.toml ./core/Cargo.toml

# this build step will cache your dependencies
WORKDIR /iftem/app
RUN cargo build --release

WORKDIR /iftem
RUN rm ./app/src/*.rs
RUN rm ./core/src/*.rs

# copy your source tree
COPY ./app/src ./app/src
COPY ./core/src ./core/src
COPY ./core/build.rs ./core/build.rs

# build for release
RUN rm ./app/target/release/deps/iftem*

WORKDIR /iftem/app
RUN cargo build --release

#2 RUN
FROM rust:1.45
COPY --from=build /iftem/app/target/release/iftem .
CMD ["./iftem"]
