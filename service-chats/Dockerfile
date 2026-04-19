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
COPY . ./
RUN cargo build -p service-chats --target x86_64-unknown-linux-musl --release

FROM scratch
COPY --from=builder /usr/src/app/target/x86_64-unknown-linux-musl/release/service-chats /usr/local/bin/service-chats
USER 1000
WORKDIR /app
ENTRYPOINT ["/usr/local/bin/service-chats"]
