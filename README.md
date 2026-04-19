# Rust service
Stack: axum, tokio, tower-http, serde, tracing, aws-sdk-s3, rdkafka, scylla

## Start service
Locally:
```bash
 RUST_LOG=info cargo run --release
```

In docker container:
```bash
docker build -t rust-server .
docker run --rm -p 3000:3000 -v $(pwd)/.env:/app/.env rust-service
```

## Ping server
```bash
curl http://127.0.0.1:3000/ping
```