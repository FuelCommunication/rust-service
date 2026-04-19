use super::schemas::{ClientKind, MessagePayload, Room};
use crate::state::ServerState;
use axum::{
    extract::{
        Path, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::IntoResponse,
};
use dashmap::DashMap;
use futures_util::{SinkExt, StreamExt};
use std::sync::LazyLock;
use tokio::sync::broadcast;
use uuid::Uuid;

static ROOMS: LazyLock<DashMap<String, Room>> = LazyLock::new(DashMap::new);

pub async fn websocket_handler(
    Path(room): Path<String>,
    State(state): State<ServerState>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| websocket(room, socket, state))
}

async fn websocket(room_id: String, stream: WebSocket, state: ServerState) {
    let chat_id = match Uuid::parse_str(&room_id) {
        Ok(id) => id,
        Err(_) => {
            tracing::error!("Invalid room UUID: {}", room_id);
            return;
        }
    };

    let rx = ROOMS
        .entry(room_id.clone())
        .or_insert_with(|| {
            let (sender, _) = broadcast::channel(100);
            Room { sender }
        })
        .sender
        .subscribe();

    let (mut ws_sender, mut ws_receiver) = stream.split();

    match state.message_store.get_chat_messages(chat_id, 100).await {
        Ok(messages) => {
            for db_msg in messages {
                if db_msg.is_deleted {
                    continue;
                }

                let payload = MessagePayload {
                    user_id: db_msg.message_id,
                    username: db_msg.user_id.to_string(),
                    text: db_msg.content,
                    ts: db_msg.created_at.timestamp_millis() as u64,
                };

                if let Ok(text) = serde_json::to_string(&payload) {
                    let _ = ws_sender.send(Message::Text(text.into())).await;
                }
            }
        }
        Err(e) => {
            tracing::error!("Failed to load chat history: {:?}", e);
        }
    }

    let mut send_task = tokio::spawn({
        let mut rx = rx;
        let mut ws_sender = ws_sender;
        async move {
            loop {
                match rx.recv().await {
                    Ok(msg) => {
                        if let Ok(text) = serde_json::to_string(&msg)
                            && ws_sender.send(Message::Text(text.into())).await.is_err()
                        {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("Receiver lagged by {} messages", n);
                        continue;
                    }
                }
            }
        }
    });

    let mut recv_task = tokio::spawn({
        let state = state.clone();
        let room_id = room_id.clone();

        async move {
            while let Some(Ok(msg)) = ws_receiver.next().await {
                let Message::Text(text) = msg else { continue };

                let Ok(kind) = serde_json::from_str::<ClientKind>(&text) else {
                    tracing::warn!("Failed to parse client message: {}", text);
                    continue;
                };

                match kind {
                    ClientKind::Join { username } => {
                        let text = format!("{} joined the room", username);

                        if let Ok(db_msg) = state.message_store.create_message(chat_id, Uuid::nil(), text.clone()).await {
                            let payload = MessagePayload {
                                user_id: db_msg.message_id,
                                username: "[system]".into(),
                                text,
                                ts: db_msg.created_at.timestamp_millis() as u64,
                            };

                            if let Some(room) = ROOMS.get(&room_id) {
                                let _ = room.sender.send(payload);
                            }
                        }
                    }

                    ClientKind::Chat(message) => {
                        let text = message.text.trim();
                        if text.is_empty() || text.len() > 5000 {
                            tracing::warn!("Invalid message length: {}", text.len());
                            continue;
                        }

                        if let Ok(db_msg) = state
                            .message_store
                            .create_message(chat_id, message.user_id, text.to_string())
                            .await
                        {
                            let payload = MessagePayload {
                                user_id: db_msg.message_id,
                                username: message.username,
                                text: text.to_string(),
                                ts: db_msg.created_at.timestamp_millis() as u64,
                            };

                            if let Some(room) = ROOMS.get(&room_id) {
                                let _ = room.sender.send(payload);
                            }
                        }
                    }
                }
            }
        }
    });

    tokio::select! {
        _ = &mut send_task => recv_task.abort(),
        _ = &mut recv_task => send_task.abort(),
    }

    if let Some(room) = ROOMS.get(&room_id)
        && room.sender.receiver_count() == 0
    {
        drop(room);
        ROOMS.remove(&room_id);
        tracing::info!("Room {} removed (no active connections)", room_id);
    }
}
