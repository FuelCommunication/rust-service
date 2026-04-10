FROM rust:1.91-slim AS builder
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        musl-tools \
        cmake \
        pkg-config \
        build-essential \
    && rustup target add x86_64-unknown-linux-musl \
    && ln -s /usr/bin/g++ /usr/bin/x86_64-linux-musl-g++
WORKDIR /usr/src/app
COPY Cargo.toml Cargo.lock ./
COPY service-gateway service-gateway
RUN sed -i 's/^members = .*/members = ["service-gateway"]/' Cargo.toml && \
    sed -i '/^default-members/d' Cargo.toml && \
    sed -i '/^s3-client/d' Cargo.toml && \
    sed -i '/^kafka-client/d' Cargo.toml && \
    sed -i '/^scylladb/d' Cargo.toml && \
    cargo build -p service-gateway --target x86_64-unknown-linux-musl --release

FROM scratch
COPY --from=builder /usr/src/app/target/x86_64-unknown-linux-musl/release/service-gateway /usr/local/bin/service-gateway
USER 1000
WORKDIR /app
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/service-gateway"]
