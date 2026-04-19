use axum::{
    Json,
    extract::{FromRequestParts, Path, Query, State, rejection::PathRejection},
    http::StatusCode,
    http::request::Parts,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};

use crate::{
    room::{self, Room},
    state::ServerState,
};

pub struct UserId(pub String);

impl<S: Send + Sync> FromRequestParts<S> for UserId {
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .headers
            .get("X-User-Id")
            .and_then(|v| v.to_str().ok())
            .filter(|s| !s.is_empty())
            .map(|s| UserId(s.to_string()))
            .ok_or(StatusCode::UNAUTHORIZED)
    }
}

pub struct RoomId(pub String);

impl FromRequestParts<ServerState> for RoomId {
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, state: &ServerState) -> Result<Self, Self::Rejection> {
        let Path(room_id) = Path::<String>::from_request_parts(parts, state)
            .await
            .map_err(|e: PathRejection| {
                tracing::warn!("Invalid room_id path: {e}");
                StatusCode::BAD_REQUEST
            })?;

        if state.rooms.contains_key(&room_id) {
            Ok(RoomId(room_id))
        } else {
            Err(StatusCode::NOT_FOUND)
        }
    }
}

#[derive(Serialize)]
pub struct RoomResponse {
    id: String,
    created_by: String,
    peers: Vec<String>,
    max_peers: usize,
}

#[derive(Serialize)]
struct CreateRoomResponse {
    id: String,
}

#[derive(Deserialize)]
pub struct ListRoomsParams {
    #[serde(default)]
    offset: usize,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    20
}

#[derive(Serialize)]
pub struct ListRoomsResponse {
    rooms: Vec<RoomSummary>,
    total: usize,
}

#[derive(Serialize)]
struct RoomSummary {
    id: String,
    created_by: String,
    peer_count: usize,
    max_peers: usize,
}

pub async fn list_rooms(State(state): State<ServerState>, Query(params): Query<ListRoomsParams>) -> Json<ListRoomsResponse> {
    let total = state.rooms.len();
    let limit = params.limit.min(100);

    let rooms: Vec<RoomSummary> = state
        .rooms
        .iter()
        .skip(params.offset)
        .take(limit)
        .map(|entry| {
            let room = entry.value();
            RoomSummary {
                id: room.id.clone(),
                created_by: room.created_by.clone(),
                peer_count: room.peers.len(),
                max_peers: room.max_peers,
            }
        })
        .collect();

    Json(ListRoomsResponse { rooms, total })
}

pub async fn create_room(State(state): State<ServerState>, UserId(user_id): UserId) -> impl IntoResponse {
    let room_id = room::new_room_id();
    let room = Room::new(room_id.clone(), user_id, state.max_peers);

    state.rooms.insert(room_id.clone(), room);
    tracing::info!(room_id = %room_id, "Room created");

    (StatusCode::CREATED, Json(CreateRoomResponse { id: room_id }))
}

pub async fn get_room(State(state): State<ServerState>, RoomId(room_id): RoomId) -> Json<RoomResponse> {
    let room = state.rooms.get(&room_id).expect("RoomId extractor guarantees existence");
    Json(RoomResponse {
        id: room.id.clone(),
        created_by: room.created_by.clone(),
        peers: room.peer_ids(),
        max_peers: room.max_peers,
    })
}

pub async fn delete_room(
    State(state): State<ServerState>,
    RoomId(room_id): RoomId,
    UserId(user_id): UserId,
) -> impl IntoResponse {
    let room = state.rooms.get(&room_id).expect("RoomId extractor guarantees existence");

    if room.created_by != user_id {
        return StatusCode::FORBIDDEN;
    }

    let msg = crate::signal::SignalMessage::Error {
        message: "Room closed by owner".into(),
    };
    room.broadcast_all(&msg);

    drop(room);
    state.rooms.remove(&room_id);
    tracing::info!(room_id = %room_id, "Room deleted by owner");

    StatusCode::NO_CONTENT
}
