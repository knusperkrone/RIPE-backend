#1 BUILD
FROM rust:1.45 as build_app
RUN USER=root cargo new --bin iftem
WORKDIR /iftem/app

# copy over your manifests
COPY ./app/Cargo.lock ./Cargo.lock
COPY ./app/Cargo.toml ./Cargo.toml
# this build step will cache your dependencies
RUN cargo build --release
RUN rm src/*.rs
# copy your source tree
COPY ./src ./src
COPY ./core ../core
# build for release
RUN rm ./target/release/deps/iftem*
RUN cargo build --release

#2 RUN
FROM rust:1.45
COPY --from=build /iftem/target/release/iftem .
CMD ["./iftem"]
