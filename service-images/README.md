# Image service

HTTP microservice for image upload, download, and deletion. Uses S3-compatible storage (RustFS) and Kafka for event notifications

## Features

- Image upload via multipart/form-data with content type validation (JPEG, PNG, GIF, WebP)
- Image download with original content type preserved
- Image deletion with ownership tracking via `X-User-Id` header
- S3-compatible object storage (RustFS) via `s3-client`
- Kafka event notifications on upload/delete via `kafka-client`
- Prometheus metrics endpoint (`/metrics`)
- CORS support with configurable origins
- Graceful shutdown with SIGTERM/SIGINT handling

## HTTP API

| Method   | Endpoint              | Description                     |
| -------- | --------------------- | ------------------------------- |
| `GET`    | `/ping`               | Liveness check                  |
| `POST`   | `/images/upload`      | Upload image (multipart)        |
| `GET`    | `/images/{filename}`  | Download image                  |
| `DELETE` | `/images/{filename}`  | Delete image                    |
| `GET`    | `/metrics`            | Prometheus metrics              |

### Headers

- `X-User-Id` (UUID) - required for upload and delete operations

### Allowed content types

`image/jpeg`, `image/png`, `image/gif`, `image/webp`

## Local launch

```bash
# 1. start RustFS and Kafka
docker compose up -d

# 2. set up environment variables
cp .env.example .env

# 3. run the service
cargo run --release
```

The server starts on `0.0.0.0:3001` by default. RustFS console is available at `http://localhost:9001`

## Environment variables

| Variable       | Required | Default | Description                          |
| -------------- | -------- | ------- | ------------------------------------ |
| `HOST`         | yes      | -       | Server bind address                  |
| `PORT`         | yes      | -       | Server port                          |
| `ORIGINS`      | yes      | -       | Comma-separated CORS origins         |
| `ACCESS_KEY`   | yes      | -       | S3 access key                        |
| `SECRET_KEY`   | yes      | -       | S3 secret key                        |
| `REGION`       | yes      | -       | S3 region                            |
| `ENDPOINT_URL` | yes      | -       | S3 endpoint URL                      |
| `BUCKET`       | yes      | -       | S3 bucket name                       |
| `BROKERS`      | yes      | -       | Kafka broker addresses               |
| `TOPIC`        | yes      | -       | Kafka topic for image events         |
| `GROUP_ID`     | yes      | -       | Kafka consumer group ID              |
