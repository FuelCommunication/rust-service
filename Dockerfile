FROM rust:1.86-slim AS builder
RUN apt-get update && apt-get install musl-tools -y && rustup target add x86_64-unknown-linux-musl
WORKDIR /usr/src/app
COPY . .
RUN cargo build --target x86_64-unknown-linux-musl --release

FROM scratch
COPY --from=builder /usr/src/app/target/x86_64-unknown-linux-musl/release/stream-service /usr/local/bin/stream-service
EXPOSE 3000
CMD ["stream-service"]