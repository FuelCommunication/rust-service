# Chat service

WebSocket real-time chat microservice. Handles messaging, message editing/deletion, typing indicators, and chat history via ScyllaDB

## Features

- Real-time messaging over WebSocket with room-based broadcasting
- Message CRUD — send, edit, delete with ownership checks
- Chat history — loads last 100 messages on connect
- Typing indicators broadcast to room participants
- User join/leave notifications
- ScyllaDB storage via `scylladb-client`
- Prometheus metrics endpoint (`/metrics`)
- CORS support with configurable origins
- Graceful shutdown with SIGTERM/SIGINT handling

## WebSocket API

Connect: `GET /ws/{room_id}` with headers `X-User-Id` and `X-Username`

### Client events

| Type     | Payload                          | Description          |
| -------- | -------------------------------- | -------------------- |
| `chat`   | `{ "text": "..." }`              | Send a message       |
| `edit`   | `{ "message_id": "", "text": "..." }` | Edit own message |
| `delete` | `{ "message_id": "" }`           | Delete own message   |
| `typing` | —                                | Typing indicator     |

### Server events

| Type          | Description                              |
| ------------- | ---------------------------------------- |
| `message`     | New message with id, user, text, ts      |
| `edited`      | Message was edited                       |
| `deleted`     | Message was deleted                      |
| `user_joined` | User joined the room                     |
| `user_left`   | User left the room                       |
| `typing`      | User is typing                           |
| `history`     | Array of messages sent on connect        |
| `error`       | Error message (invalid format, etc.)     |

## HTTP endpoints

| Endpoint   | Description              |
| ---------- | ------------------------ |
| `/ping`    | Liveness check           |
| `/health`  | ScyllaDB health check    |
| `/metrics` | Prometheus metrics       |

## Local launch

```bash
# 1. start ScyllaDB
docker compose up -d

# 2. set up environment variables
cp .env.example .env

# 3. run the service
cargo run --release
```

The server starts on `0.0.0.0:3001` by default

## Environment variables

| Variable                | Required | Default                                        | Description                       |
| ----------------------- | -------- | ---------------------------------------------- | --------------------------------- |
| `HOST`                  | yes      | —                                              | Server bind address               |
| `PORT`                  | yes      | —                                              | Server port                       |
| `ORIGINS`               | yes      | —                                              | Comma-separated CORS origins      |
| `SCYLLA_URL`            | yes      | —                                              | ScyllaDB node address (host:port) |
| `SCYLLA_NODES`          | no       | `""`                                           | Additional ScyllaDB nodes (comma-separated) |
| `BROADCAST_BUFFER_SIZE` | no       | `128`                                          | Broadcast channel buffer size     |
