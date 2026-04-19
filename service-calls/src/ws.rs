use std::time::Duration;

use axum::{
    extract::{
        Path, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;

use crate::{
    room::{self, Rooms},
    routes::UserId,
    signal::{ClientMessage, SignalMessage},
    state::ServerState,
};

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    Path(room_id): Path<String>,
    State(state): State<ServerState>,
    UserId(user_id): UserId,
) -> impl IntoResponse {
    let max_msg_size = state.max_message_size;

    ws.max_message_size(max_msg_size)
        .on_upgrade(move |socket| handle_socket(socket, room_id, user_id, state))
}

async fn handle_socket(socket: WebSocket, room_id: String, user_id: String, state: ServerState) {
    let (tx, rx) = mpsc::channel::<SignalMessage>(state.channel_capacity);
    let session_id = room::next_session_id();

    let (existing_peers, is_reconnect) = {
        let Some(mut room) = state.rooms.get_mut(&room_id) else {
            send_error_and_close(socket, "Room not found").await;
            return;
        };

        if room.is_full_for(&user_id) {
            drop(room);
            send_error_and_close(socket, "Room is full").await;
            return;
        }

        let peers = room.peer_ids();

        let was_connected = room.peers.contains_key(&user_id);
        if !was_connected {
            room.broadcast(
                &user_id,
                &SignalMessage::UserJoined {
                    user_id: user_id.clone(),
                },
            );
        }

        let old_peer = room.add_peer(user_id.clone(), session_id, tx);
        if let Some(old) = &old_peer {
            tracing::info!(
                user_id = %user_id,
                room_id = %room_id,
                old_session = old.session_id,
                new_session = session_id,
                "Peer reconnected"
            );
        }

        (peers, old_peer.is_some())
    };

    let (mut ws_tx, mut ws_rx) = socket.split();

    let joined = SignalMessage::Joined {
        user_id: user_id.clone(),
        peers: if is_reconnect {
            existing_peers.into_iter().filter(|p| p != &user_id).collect()
        } else {
            existing_peers
        },
    };
    if let Ok(json) = serde_json::to_string(&joined)
        && ws_tx.send(Message::Text(json.into())).await.is_err()
    {
        cleanup_peer(&state.rooms, &room_id, &user_id, session_id);
        return;
    }

    let rooms_for_task = state.rooms.clone();
    let user_id_for_task = user_id.clone();
    let room_id_for_task = room_id.clone();
    let heartbeat_interval = Duration::from_secs(state.heartbeat_interval_secs);
    let mut rx = rx;
    let send_task = tokio::spawn(async move {
        let mut ping_interval = tokio::time::interval(heartbeat_interval);
        ping_interval.tick().await;

        loop {
            tokio::select! {
                msg = rx.recv() => {
                    match msg {
                        Some(msg) => {
                            let Ok(json) = serde_json::to_string(&msg) else {
                                tracing::warn!("Failed to serialize signal message");
                                continue;
                            };
                            if ws_tx.send(Message::Text(json.into())).await.is_err() {
                                break;
                            }
                        }
                        None => break,
                    }
                }
                _ = ping_interval.tick() => {
                    if ws_tx.send(Message::Ping(vec![].into())).await.is_err() {
                        break;
                    }
                }
            }
        }
        cleanup_peer(&rooms_for_task, &room_id_for_task, &user_id_for_task, session_id);
    });

    let max_message_size = state.max_message_size;

    while let Some(Ok(msg)) = ws_rx.next().await {
        match msg {
            Message::Text(text) => {
                if let Some(room) = state.rooms.get(&room_id) {
                    room.touch();
                }
                handle_client_message(&state.rooms, &room_id, &user_id, &text, max_message_size);
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    send_task.abort();
    cleanup_peer(&state.rooms, &room_id, &user_id, session_id);
}

fn handle_client_message(rooms: &Rooms, room_id: &str, from: &str, text: &str, max_message_size: usize) {
    let msg: ClientMessage = match serde_json::from_str(text) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(peer = %from, "Invalid signal message: {e}");
            return;
        }
    };

    if let Err(reason) = msg.validate_size(max_message_size) {
        tracing::warn!(peer = %from, "Message rejected: {reason}");
        return;
    }

    let Some(room) = rooms.get(room_id) else {
        return;
    };

    match msg {
        ClientMessage::Offer { to, sdp } => {
            room.send_to(
                &to,
                SignalMessage::Offer {
                    from: from.to_string(),
                    sdp,
                },
            );
        }
        ClientMessage::Answer { to, sdp } => {
            room.send_to(
                &to,
                SignalMessage::Answer {
                    from: from.to_string(),
                    sdp,
                },
            );
        }
        ClientMessage::IceCandidate { to, candidate } => {
            room.send_to(
                &to,
                SignalMessage::IceCandidate {
                    from: from.to_string(),
                    candidate,
                },
            );
        }
    }
}

fn cleanup_peer(rooms: &Rooms, room_id: &str, user_id: &str, session_id: u64) {
    let should_remove = {
        let Some(mut room) = rooms.get_mut(room_id) else {
            return;
        };

        if !room.remove_peer_by_session(user_id, session_id) {
            return;
        }

        room.broadcast(
            user_id,
            &SignalMessage::UserLeft {
                user_id: user_id.to_string(),
            },
        );

        room.is_empty()
    };

    if should_remove {
        rooms.remove(room_id);
        tracing::info!(room_id = %room_id, "Room removed (empty)");
    }
}

async fn send_error_and_close(mut socket: WebSocket, message: &str) {
    let msg = SignalMessage::Error { message: message.into() };
    if let Ok(json) = serde_json::to_string(&msg) {
        let _ = socket.send(Message::Text(json.into())).await;
        let _ = socket.close().await;
    }
}
