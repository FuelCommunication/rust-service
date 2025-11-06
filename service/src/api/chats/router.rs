use super::schemas::{ClientKind, MessagePayload};
use axum::{
    extract::{
        Path, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex},
};
use tokio::sync::broadcast;
use uuid::Uuid;

struct Room {
    sender: broadcast::Sender<MessagePayload>,
    history: Vec<MessagePayload>,
}

static ROOMS: LazyLock<Arc<Mutex<HashMap<String, Room>>>> = LazyLock::new(|| Arc::new(Mutex::new(HashMap::new())));

pub async fn websocket_handler(Path(room): Path<String>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(|socket| websocket(room, socket))
}

async fn websocket(room: String, stream: WebSocket) {
    let (mut rx, history) = {
        let mut rooms = ROOMS.lock().unwrap();

        let room = rooms.entry(room.clone()).or_insert_with(|| {
            let (s, _rx) = broadcast::channel(100);
            Room {
                sender: s,
                history: Vec::new(),
            }
        });

        (room.sender.subscribe(), room.history.clone())
    };

    let (mut ws_sender, mut ws_receiver) = stream.split();

    for msg in history {
        let text = serde_json::to_string(&msg).unwrap();
        let _ = ws_sender.send(Message::Text(text.into())).await;
    }

    let uid = Uuid::now_v7().to_string();
    let username = format!("Anon-{}", &uid[..8]);

    let mut send_task = tokio::spawn({
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
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                }
            }
        }
    });

    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_receiver.next().await {
            if let Message::Text(t) = msg
                && let Ok(kind) = serde_json::from_str::<ClientKind>(&t)
            {
                match kind {
                    ClientKind::Join { username } => {
                        let payload = MessagePayload {
                            id: Uuid::now_v7().to_string(),
                            username: "[system]".to_string(),
                            text: format!("{} joined room {}", username, room),
                            ts: chrono::Utc::now().timestamp_millis() as u64,
                        };
                        let mut rooms = ROOMS.lock().unwrap();
                        if let Some(room) = rooms.get_mut(&room) {
                            room.history.push(payload.clone());
                            let _ = room.sender.send(payload);
                        }
                    }
                    ClientKind::Chat { message: text } => {
                        let payload = MessagePayload {
                            id: Uuid::now_v7().to_string(),
                            username: username.clone(),
                            text,
                            ts: chrono::Utc::now().timestamp_millis() as u64,
                        };
                        let mut rooms = ROOMS.lock().unwrap();
                        if let Some(room) = rooms.get_mut(&room) {
                            room.history.push(payload.clone());
                            let _ = room.sender.send(payload);
                        }
                    }
                }
            }
        }
    });

    tokio::select! {
        _ = &mut send_task => (),
        _ = &mut recv_task => (),
    }
}
