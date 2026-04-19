# Rust service
Stack: axum, tokio, tower-http, serde-json, tracing

## Start service
Locally:
```shell
cargo run --release
```

In docker container:
```shell
docker build -t rust-server .
docker run --rm -p 3000:3000 rust-server
```

## Ping server
```shell
curl http://127.0.0.1:3000/ping
```