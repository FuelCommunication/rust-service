# Calls service

WebRTC signaling server for peer-to-peer video/audio calls. Manages rooms and relays SDP offers/answers and ICE candidates between peers. No media passes through the server - all media flows directly between browsers via WebRTC.
Stack: axum, tokio, dashmap, futures-util, serde-json, tracing and mimalloc

## Features

- Room creation/deletion/listing via REST API
- WebSocket-based signaling for WebRTC negotiation
- Mesh topology - up to 4 peers per room (configurable)
- Automatic room cleanup when all peers disconnect or on idle timeout
- Owner-only room deletion with peer notification
- Header-based authentication via `X-User-Id` (required, injected by API gateway)
- WebSocket heartbeat (ping/pong) for dead connection detection
- Peer reconnect support - same `user_id` replaces old connection without disrupting other peers
- Bounded message channels with backpressure and payload size validation
- Prometheus metrics and structured tracing
- Graceful shutdown with SIGTERM/SIGINT handling
- No external dependencies (no database, no Redis) - rooms are in-memory

## Architecture

```
Browser A ←── WebRTC P2P ──→ Browser B
    ↑                            ↑
    └───── WebRTC P2P ───────────┘
    ↕              ↕              ↕
┌──────────────────────────────────┐
│   service-calls (signaling only) │
│   WebSocket + REST               │
└──────────────────────────────────┘
```

The server only handles signaling - exchanging SDP and ICE candidates so browsers can establish direct peer-to-peer connections. STUN server (e.g. `stun:stun.l.google.com:19302`) is configured on the client side.

## REST API

| Method   | Path              | Description              |
| -------- | ----------------- | ------------------------ |
| `GET`    | `/rooms`          | List rooms               |
| `POST`   | `/rooms`          | Create a new room        |
| `GET`    | `/rooms/{room_id}`| Get room info            |
| `DELETE` | `/rooms/{room_id}`| Delete room (owner only) |

## WebSocket signaling

Connect to `/ws/{room_id}` to join the room. Messages are JSON.

### Server → Client

```json
{ "type": "joined", "user_id": "you", "peers": ["peer1", "peer2"] }
{ "type": "user_joined", "user_id": "new_peer" }
{ "type": "user_left", "user_id": "gone_peer" }
{ "type": "offer", "from": "peer_id", "sdp": "..." }
{ "type": "answer", "from": "peer_id", "sdp": "..." }
{ "type": "ice_candidate", "from": "peer_id", "candidate": "..." }
{ "type": "error", "message": "Room is full" }
```

### Client → Server

```json
{ "type": "offer", "to": "peer_id", "sdp": "..." }
{ "type": "answer", "to": "peer_id", "sdp": "..." }
{ "type": "ice_candidate", "to": "peer_id", "candidate": "..." }
```

### Signaling flow

1. Client A creates a room via `POST /rooms`, gets `room_id`
2. Client A connects to `ws://.../ws/{room_id}`, receives `joined` with empty peers list
3. Client B connects to `ws://.../ws/{room_id}`, receives `joined` with `["A"]` in peers
4. Client A receives `user_joined` with B's user_id
5. Client A sends `offer` to B (with SDP)
6. Client B receives `offer` from A, sends `answer` back
7. Both exchange `ice_candidate` messages
8. WebRTC P2P connection established - media flows directly

## Local launch

```bash
# 1. set up environment variables
cp .env.example .env

# 2. run the service
cargo run --release
```

## Environment variables

| Variable                 | Required | Default | Description                                      |
| ------------------------ | -------- | ------- | -------------------------------------------------|
| `HOST`                   | yes      | -       | HTTP server bind address                         |
| `PORT`                   | yes      | -       | HTTP server port                                 |
| `ORIGINS`                | yes      | -       | Allowed CORS origins (comma-separated)           |
| `MAX_PEERS_PER_ROOM`     | no       | 4       | Maximum peers per room                           |
| `ROOM_IDLE_TIMEOUT_SECS` | no       | 30      | Idle room cleanup timeout (seconds)              |
| `MAX_MESSAGE_SIZE`       | no       | 65536   | Max WebSocket message / SDP payload size (bytes) |
| `CHANNEL_CAPACITY`       | no       | 64      | Bounded channel buffer per peer                  |
| `REQUEST_TIMEOUT_SECS`   | no       | 10      | HTTP request timeout (seconds)                   |
| `HEARTBEAT_INTERVAL_SECS`| no       | 15      | WebSocket ping interval (seconds)                |
