use super::schemas::{ClientEvent, MessagePayload, ServerEvent};
use crate::state::{Room, ServerState};
use axum::{
    extract::{
        Path, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use futures_util::{
    SinkExt, StreamExt,
    stream::{SplitSink, SplitStream},
};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{broadcast, mpsc};
use uuid::Uuid;

const MAX_MESSAGE_LENGTH: usize = 5000;

pub async fn websocket_handler(
    Path(room): Path<String>,
    State(state): State<ServerState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let Some(user_id) = headers
        .get("X-User-Id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| Uuid::parse_str(s).ok())
    else {
        return (StatusCode::UNAUTHORIZED, "Missing user identity").into_response();
    };

    let username = headers
        .get("X-Username")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let username = if username.is_empty() { user_id.to_string() } else { username };

    let check_url = format!("{}/channels/{}/subscribers/check", state.channels_service_url, room);
    let resp = state
        .http_client
        .get(&check_url)
        .header("X-User-Id", user_id.to_string())
        .send()
        .await;

    match resp {
        Ok(r) if r.status().is_success() => {}
        Ok(r) if r.status().as_u16() == 403 => {
            return (StatusCode::FORBIDDEN, "Not subscribed to this channel").into_response();
        }
        _ => {
            return (StatusCode::BAD_GATEWAY, "Failed to verify subscription").into_response();
        }
    }

    ws.on_upgrade(move |socket| websocket(room, socket, state, user_id, username))
        .into_response()
}

async fn websocket(room_id: String, stream: WebSocket, state: ServerState, user_id: Uuid, username: String) {
    let chat_id = match Uuid::parse_str(&room_id) {
        Ok(id) => id,
        Err(_) => {
            tracing::error!("Invalid room UUID: {}", room_id);
            return;
        }
    };

    let (mut ws_sender, ws_receiver) = stream.split();
    let rx = state
        .rooms
        .entry(room_id.clone())
        .or_insert_with(|| {
            let (sender, _) = broadcast::channel(state.broadcast_buffer_size);
            Room { sender }
        })
        .sender
        .subscribe();

    send_history(&state, chat_id, &mut ws_sender).await;

    let (direct_tx, direct_rx) = mpsc::unbounded_channel();
    let mut send_task = tokio::spawn(send_loop(rx, direct_rx, ws_sender));
    let mut recv_task = tokio::spawn(recv_loop(
        ws_receiver,
        state.clone(),
        room_id.clone(),
        chat_id,
        user_id,
        username.clone(),
        direct_tx,
    ));

    tokio::select! {
        _ = &mut send_task => recv_task.abort(),
        _ = &mut recv_task => send_task.abort(),
    }

    if let Some(room) = state.rooms.get(&room_id)
        && room.sender.receiver_count() == 0
    {
        drop(room);
        state.rooms.remove(&room_id);
        tracing::info!("Room {} removed (no active connections)", room_id);
    }
}

async fn send_history(state: &ServerState, chat_id: Uuid, ws_sender: &mut SplitSink<WebSocket, Message>) {
    match state.message_store.get_chat_messages(chat_id, 100).await {
        Ok(messages) => {
            let payloads: Vec<MessagePayload> = messages
                .into_iter()
                .filter(|m| !m.is_deleted)
                .map(|m| MessagePayload {
                    message_id: m.message_id,
                    user_id: m.user_id,
                    username: m.user_id.to_string(),
                    text: m.content,
                    ts: m.created_at.timestamp_millis() as u64,
                })
                .collect();

            let event = ServerEvent::History { messages: payloads };
            if let Ok(text) = serde_json::to_string(&event) {
                let _ = ws_sender.send(Message::Text(text.into())).await;
            }
        }
        Err(e) => {
            tracing::error!("Failed to load chat history: {:?}", e);
        }
    }
}

fn broadcast_to_room(state: &ServerState, room_id: &str, event: ServerEvent) {
    if let Some(room) = state.rooms.get(room_id) {
        let _ = room.sender.send(event);
    }
}

fn now_millis() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64
}

async fn send_loop(
    mut rx: broadcast::Receiver<ServerEvent>,
    mut direct_rx: mpsc::UnboundedReceiver<ServerEvent>,
    mut ws_sender: SplitSink<WebSocket, Message>,
) {
    loop {
        let event = tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(event) => event,
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("Receiver lagged by {} messages", n);
                        continue;
                    }
                }
            }
            Some(event) = direct_rx.recv() => event,
            else => break,
        };

        if let Ok(text) = serde_json::to_string(&event)
            && ws_sender.send(Message::Text(text.into())).await.is_err()
        {
            break;
        }
    }
}

async fn recv_loop(
    mut ws_receiver: SplitStream<WebSocket>,
    state: ServerState,
    room_id: String,
    chat_id: Uuid,
    user_id: Uuid,
    username: String,
    direct_tx: mpsc::UnboundedSender<ServerEvent>,
) {
    while let Some(Ok(msg)) = ws_receiver.next().await {
        let Message::Text(text) = msg else { continue };

        let Ok(event) = serde_json::from_str::<ClientEvent>(&text) else {
            let _ = direct_tx.send(ServerEvent::Error {
                text: "Invalid message format".into(),
            });
            continue;
        };

        match event {
            ClientEvent::Chat { text } => {
                let text = text.trim().to_string();
                if text.is_empty() || text.len() > MAX_MESSAGE_LENGTH {
                    let _ = direct_tx.send(ServerEvent::Error {
                        text: "Invalid message length".into(),
                    });
                    continue;
                }

                match state.message_store.create_message(chat_id, user_id, text.clone()).await {
                    Ok(db_msg) => {
                        broadcast_to_room(
                            &state,
                            &room_id,
                            ServerEvent::Message(MessagePayload {
                                message_id: db_msg.message_id,
                                user_id,
                                username: username.clone(),
                                text,
                                ts: db_msg.created_at.timestamp_millis() as u64,
                            }),
                        );
                    }
                    Err(e) => {
                        tracing::error!("Failed to save message: {:?}", e);
                        let _ = direct_tx.send(ServerEvent::Error {
                            text: "Failed to save message".into(),
                        });
                    }
                }
            }

            ClientEvent::Edit { message_id, text } => {
                let text = text.trim().to_string();
                if text.is_empty() || text.len() > MAX_MESSAGE_LENGTH {
                    let _ = direct_tx.send(ServerEvent::Error {
                        text: "Invalid message length".into(),
                    });
                    continue;
                }

                match state.message_store.get_message(message_id).await {
                    Ok(Some(msg)) if msg.user_id == user_id => {
                        match state
                            .message_store
                            .update_message(chat_id, msg.created_at, message_id, text.clone())
                            .await
                        {
                            Ok(()) => {
                                broadcast_to_room(
                                    &state,
                                    &room_id,
                                    ServerEvent::Edited {
                                        message_id,
                                        text,
                                        ts: now_millis(),
                                    },
                                );
                            }
                            Err(e) => {
                                tracing::error!("Failed to update message: {:?}", e);
                                let _ = direct_tx.send(ServerEvent::Error {
                                    text: "Failed to edit message".into(),
                                });
                            }
                        }
                    }
                    Ok(Some(_)) => {
                        let _ = direct_tx.send(ServerEvent::Error {
                            text: "Permission denied".into(),
                        });
                    }
                    Ok(None) => {
                        let _ = direct_tx.send(ServerEvent::Error {
                            text: "Message not found".into(),
                        });
                    }
                    Err(e) => {
                        tracing::error!("Failed to get message: {:?}", e);
                        let _ = direct_tx.send(ServerEvent::Error {
                            text: "Failed to edit message".into(),
                        });
                    }
                }
            }

            ClientEvent::Delete { message_id } => match state.message_store.get_message(message_id).await {
                Ok(Some(msg)) if msg.user_id == user_id => {
                    match state.message_store.delete_message(chat_id, msg.created_at, message_id).await {
                        Ok(()) => {
                            broadcast_to_room(&state, &room_id, ServerEvent::Deleted { message_id });
                        }
                        Err(e) => {
                            tracing::error!("Failed to delete message: {:?}", e);
                            let _ = direct_tx.send(ServerEvent::Error {
                                text: "Failed to delete message".into(),
                            });
                        }
                    }
                }
                Ok(Some(_)) => {
                    let _ = direct_tx.send(ServerEvent::Error {
                        text: "Permission denied".into(),
                    });
                }
                Ok(None) => {
                    let _ = direct_tx.send(ServerEvent::Error {
                        text: "Message not found".into(),
                    });
                }
                Err(e) => {
                    tracing::error!("Failed to get message: {:?}", e);
                    let _ = direct_tx.send(ServerEvent::Error {
                        text: "Failed to delete message".into(),
                    });
                }
            },

            ClientEvent::Typing => {
                broadcast_to_room(
                    &state,
                    &room_id,
                    ServerEvent::Typing {
                        user_id,
                        username: username.clone(),
                    },
                );
            }
        }
    }
}
