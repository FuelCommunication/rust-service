# Channels service

REST microservice for channel management. Handles creation, search, subscriptions, and ownership transfer.
Stack: axum, tokio, tower-http, sqlx, serde-json, tracing, thiserror, mimalloc, meilisearch-sdk and valkey-client 

## Features

- Channel CRUD with ownership enforcement
- Full-text search via Meilisearch (title, description)
- Caching layer via Valkey with configurable TTL
- Subscription management — subscribe, unsubscribe, list subscribers
- Subscription check endpoint for service-to-service verification
- Ownership transfer between subscribers
- Paginated responses for all list endpoints
- Header-based authentication via `X-User-Id` (injected by API gateway)
- PostgreSQL storage with sqlx (compile-time checked queries)
- Prometheus metrics and structured tracing
- Graceful shutdown with SIGTERM/SIGINT handling

## REST API

| Method   | Path                                        | Description                          |
| -------- | ------------------------------------------- | ------------------------------------ |
| `POST`   | `/channels`                                 | Create a new channel                 |
| `GET`    | `/channels`                                 | List all channels (paginated)        |
| `GET`    | `/channels/search?q=`                       | Full-text search via Meilisearch     |
| `GET`    | `/channels/{channel_id}`                    | Get a single channel (cached)        |
| `PATCH`  | `/channels/{channel_id}`                    | Update channel (owner only)          |
| `DELETE` | `/channels/{channel_id}`                    | Delete channel (owner only)          |
| `GET`    | `/channels/sub/user/{user_id}`              | Get user's subscriptions             |
| `GET`    | `/channels/sub/channel/{channel_id}`        | Get channel's subscribers            |
| `GET`    | `/channels/{channel_id}/subscribers/check`  | Check if current user is subscribed  |
| `POST`   | `/channels/{channel_id}/subscribe`          | Subscribe to channel                 |
| `DELETE` | `/channels/{channel_id}/subscribe`          | Unsubscribe from channel             |
| `POST`   | `/channels/{channel_id}/transfer/{user_id}` | Transfer ownership to subscriber     |

Paginated endpoints accept `currentPage` (default 1) and `pageSize` (default 10) query parameters.

## Caching

Valkey is used as a cache layer with the following strategy:

| Key pattern                   | Cached by            | Invalidated by                      |
| ----------------------------- | -------------------- | ----------------------------------- |
| `channel:{id}`                | `get_channel`        | `update_channel`, `delete_channel`  |
| `sub:{user_id}:{channel_id}` | `check_subscription` | `subscribe`, `unsubscribe`, `delete_channel` |

Default TTL: 300 seconds (5 minutes), configurable via `CACHE_TTL_SECS`.

## Search

Meilisearch provides full-text search across channel titles and descriptions. The search index is automatically synchronized on channel create, update, and delete.

## Local launch

```bash
# 1. start dependencies (PostgreSQL, Valkey, Meilisearch)
docker compose up -d

# 2. set up environment variables
cp .env.example .env

# 3. run migrations
sqlx migrate run

# 4. run the service
cargo run --release
```

The HTTP server starts on port `3003` by default.

## Environment variables

| Variable                  | Required | Default | Description                            |
| ------------------------- | -------- | ------- | -------------------------------------- |
| `HOST`                    | yes      | —       | HTTP server bind address               |
| `PORT`                    | yes      | —       | HTTP server port                       |
| `ORIGINS`                 | yes      | —       | Allowed CORS origins (comma-separated) |
| `DATABASE_URL`            | yes      | —       | PostgreSQL connection string           |
| `DB_MAX_CONNECTIONS`      | no       | 10      | Max database pool connections          |
| `DB_MIN_CONNECTIONS`      | no       | 1       | Min database pool connections          |
| `DB_ACQUIRE_TIMEOUT_SECS` | no       | 5       | Database connection acquire timeout    |
| `VALKEY_URL`              | yes      | —       | Valkey connection URL                  |
| `CACHE_TTL_SECS`          | no       | 300     | Cache TTL in seconds                   |
| `MEILISEARCH_URL`         | yes      | —       | Meilisearch instance URL               |
| `MEILISEARCH_API_KEY`     | no       | —       | Meilisearch API key (optional in dev)  |

## Database schema

- **channels** — channel info (title, description, avatar_url) with auto-updated timestamps
- **channel_subscribers** — channel membership with ownership flag, unique per (user_id, channel_id)
