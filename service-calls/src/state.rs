use std::{sync::Arc, time::Duration};

use dashmap::DashMap;

use crate::{
    room::{Room, Rooms},
    signal::SignalMessage,
};

pub type ServerState = Arc<ServerData>;

pub struct ServerData {
    pub rooms: Rooms,
    pub max_peers: usize,
    pub max_message_size: usize,
    pub channel_capacity: usize,
    pub heartbeat_interval_secs: u64,
}

impl ServerData {
    pub fn new(
        max_peers: usize,
        room_idle_timeout_secs: u64,
        max_message_size: usize,
        channel_capacity: usize,
        heartbeat_interval_secs: u64,
    ) -> ServerState {
        let rooms: Rooms = Arc::new(DashMap::with_capacity(1_000));

        spawn_idle_cleanup(rooms.clone(), room_idle_timeout_secs);

        Arc::new(Self {
            rooms,
            max_peers,
            max_message_size,
            channel_capacity,
            heartbeat_interval_secs,
        })
    }
}

fn spawn_idle_cleanup(rooms: Rooms, timeout_secs: u64) {
    let interval = Duration::from_secs(timeout_secs.max(5));

    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.tick().await;

        loop {
            ticker.tick().await;
            cleanup_idle_rooms(&rooms, timeout_secs);
        }
    });
}

fn cleanup_idle_rooms(rooms: &Rooms, timeout_secs: u64) {
    rooms.retain(|room_id: &String, room: &mut Room| {
        if room.idle_secs() < timeout_secs {
            return true;
        }

        if room.is_empty() {
            tracing::info!(room_id = %room_id, "Room removed (idle, empty)");
            return false;
        }

        let msg = SignalMessage::Error {
            message: "Room closed due to inactivity".into(),
        };
        room.broadcast_all(&msg);
        tracing::info!(
            room_id = %room_id,
            peers = room.peers.len(),
            "Room removed (idle timeout)"
        );
        false
    });
}
