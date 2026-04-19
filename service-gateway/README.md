# Gateway service

API gateway built on Pingora. Routes HTTP/gRPC traffic to backend services, handles authentication, CORS, rate limiting, and REST-to-gRPC translation for the auth service.
Stack: pingora, pingora-limits, tonic, prost, serde-json, tracing, uuid, chrono and bytes

## Features

- Reverse proxy to images, chats, channels, and auth services
- JWT authentication via gRPC call to auth service (token validation)
- REST-to-gRPC translation for auth endpoints (`/access/*`)
- OAuth 2.0 flow support (Google, GitHub) with redirect handling
- CORS with configurable allowed origins
- Per-client rate limiting (by `appid` header or client IP)
- Request body size enforcement (Content-Length check + streaming accumulation)
- Request ID injection (UUID v7) for tracing across services
- Header injection: `X-User-Id`, `X-Username`, `X-Email`, `X-Forwarded-For`, `X-Real-IP`, `X-Forwarded-Proto`
- Client-supplied internal headers are stripped to prevent spoofing
- Graceful shutdown with configurable grace period

## Routing

| Path prefix   | Upstream          | Protocol | Auth required |
| ------------- | ----------------- | -------- | ------------- |
| `/images/*`   | images service    | HTTP     | yes           |
| `/ws/*`       | chats service     | HTTP/WS  | yes           |
| `/channels/*` | channels service  | HTTP     | yes           |
| `/auth.*`     | auth service      | gRPC/H2  | no            |
| `/access/*`   | handled in-gateway| REST→gRPC| no            |
| `/ping`       | proxied           | HTTP     | no            |
| `/health`     | proxied           | HTTP     | no            |
| `/metrics`    | proxied           | HTTP     | no            |

## Auth API (REST-to-gRPC)

These endpoints are intercepted by the gateway and translated to gRPC calls to the auth service:

| Method | Path                     | Description               |
| ------ | ------------------------ | ------------------------- |
| `POST` | `/access/login`          | Login with email/password |
| `POST` | `/access/register`       | Register new user         |
| `POST` | `/access/refresh`        | Refresh access token      |
| `POST` | `/access/logout`         | Invalidate refresh token  |
| `GET`  | `/access/oauth/google`   | Start Google OAuth flow   |
| `GET`  | `/access/oauth/github`   | Start GitHub OAuth flow   |
| `GET`  | `/access/oauth/callback` | OAuth callback            |

POST endpoints expect `Content-Type: application/json`.

## Authentication flow

1. Public routes (`/auth.*`, `/access/*`, `/ping`, `/health`, `/metrics`) pass through without auth
2. For protected routes, the gateway extracts the Bearer token from `Authorization` header (or `token` query parameter for WebSocket)
3. Token is validated via `AuthService.ValidateToken` gRPC call
4. On success, `X-User-Id`, `X-Username`, `X-Email` headers are injected into the upstream request
5. On failure, 401 Unauthorized is returned

## Rate limiting

Per-client rate limiting based on the `appid` header (falls back to client IP). Configurable via `GATEWAY_MAX_REQ_PER_SEC`. Returns `429 Too Many Requests` with rate limit headers when exceeded.

## Local launch

```bash
# 1. set up environment variables
cp .env.example .env

# 2. ensure upstream services are running

# 3. run the gateway
cargo run --release
```

The gateway listens on `0.0.0.0:8080` by default.

## Environment variables

| Variable                                | Required | Default                                        | Description                        |
| --------------------------------------- | -------- | ---------------------------------------------- | ---------------------------------- |
| `GATEWAY_LISTEN_ADDR`                   | yes      | -                                              | Gateway bind address               |
| `GATEWAY_IMAGES_UPSTREAM`               | yes      | -                                              | Images service address             |
| `GATEWAY_CHATS_UPSTREAM`                | yes      | -                                              | Chats service address              |
| `GATEWAY_CHANNELS_UPSTREAM`             | yes      | -                                              | Channels service address           |
| `GATEWAY_AUTH_UPSTREAM`                 | yes      | -                                              | Auth service gRPC address          |
| `GATEWAY_MAX_REQ_PER_SEC`               | yes      | -                                              | Max requests per second per client |
| `GATEWAY_MAX_BODY_SIZE_MB`              | yes      | -                                              | Max request body size in MB        |
| `GATEWAY_CONN_TIMEOUT_SECS`             | yes      | -                                              | Upstream connection timeout        |
| `GATEWAY_TOTAL_CONN_TIMEOUT_SECS`       | yes      | -                                              | Total upstream connection timeout  |
| `GATEWAY_READ_TIMEOUT_SECS`             | yes      | -                                              | Upstream read timeout              |
| `GATEWAY_WRITE_TIMEOUT_SECS`            | yes      | -                                              | Upstream write timeout             |
| `GATEWAY_ALLOWED_ORIGINS`               | no       | (empty = allow all)                            | Allowed CORS origins               |
| `GATEWAY_OAUTH_CALLBACK_URL`            | no       | `http://127.0.0.1:8080/access/oauth/callback`  | OAuth redirect URI                 |
| `GATEWAY_FRONTEND_URL`                  | no       | `http://localhost:3000`                        | Frontend URL for OAuth redirects   |
| `GATEWAY_GRACE_PERIOD_SECS`             | no       | `5`                                            | Graceful shutdown grace period     |
| `GATEWAY_GRACEFUL_SHUTDOWN_TIMEOUT_SECS`| no       | `5`                                             | Graceful shutdown timeout         |
| `RUST_LOG`                              | no       | -                                              | Tracing filter (e.g. `info`)       |
