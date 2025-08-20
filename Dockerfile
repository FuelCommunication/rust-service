FROM rust:1.86-slim AS builder
RUN apt-get update && apt-get install musl-tools -y && rustup target add x86_64-unknown-linux-musl
WORKDIR /usr/src/app
COPY Cargo.toml Cargo.lock ./
COPY service service
RUN cargo build --target x86_64-unknown-linux-musl --release

FROM scratch
COPY --from=builder /usr/src/app/target/x86_64-unknown-linux-musl/release/service /usr/local/bin/service
USER 1000
EXPOSE 3000
ENTRYPOINT ["/usr/local/bin/stream-service"]