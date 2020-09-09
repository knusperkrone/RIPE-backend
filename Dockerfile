#1 BUILD
FROM alpine:latest as build
RUN apk update && \
    apk add git build-base openssl-dev \
    rust cargo cmake postgresql-dev

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
RUN cargo build --target x86_64-alpine-linux-musl --release

WORKDIR /iftem
RUN rm ./app/src/*.rs
RUN rm ./core/src/*.rs

# copy your source tree
COPY ./app/src ./app/src
COPY ./core/src ./core/src
COPY ./core/build.rs ./core/build.rs

# build for release
# RUN rm ./app/target/release/deps/iftem*
WORKDIR /iftem/app
RUN cargo build --target x86_64-alpine-linux-musl --release 

#2 RUN
FROM alpine:latest
RUN apk update && \
    apk add git build-base postgresql-dev 

RUN addgroup -g 1000 iftem
RUN adduser -D -s /bin/sh -u 1000 -G iftem iftem
USER iftem

WORKDIR /home/iftem/bin/
COPY --from=build /iftem/app/target/x86_64-alpine-linux-musl/release/iftem .
RUN chown iftem:iftem iftem
CMD ["./iftem"]
