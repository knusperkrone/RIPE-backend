FROM rust:1.43

WORKDIR /usr/src/iftem
COPY . .

RUN cargo install -p iftem

CMD ["iftem"]