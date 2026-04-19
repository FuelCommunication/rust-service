# Rust service
Stack: hyper + tokio, tower, serde, tracing

## Start service
Locally:
```shell
cargo run --release
```

With docker:
```shell
docker build -p 3000:3000  ./
docker run <image>
```

## Ping server
```shell
curl  http://127.0.0.1:3000/ping
```