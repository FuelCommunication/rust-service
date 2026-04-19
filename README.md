# Rust service
Stack: hyper + tokio, tower, serde-json, tracing

## Start service
Locally:
```shell
cargo run --release
```

With docker:
```shell
docker build -t hyper-server .
docker run --rm -p 3000:3000 hyper-server
```

## Ping server
```shell
curl http://127.0.0.1:3000/ping
```