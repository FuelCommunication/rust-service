use dashmap::DashMap;
use std::{
    collections::HashMap,
    sync::Arc,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::signal::SignalMessage;

pub type Rooms = Arc<DashMap<String, Room>>;

static SESSION_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn next_session_id() -> u64 {
    SESSION_COUNTER.fetch_add(1, Ordering::Relaxed)
}

pub fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

pub struct Peer {
    pub user_id: String,
    pub session_id: u64,
    pub tx: mpsc::Sender<SignalMessage>,
}

pub struct Room {
    pub id: String,
    pub created_by: String,
    pub peers: HashMap<String, Peer>,
    pub max_peers: usize,
    pub last_activity: AtomicU64,
}

impl Room {
    pub fn new(id: String, created_by: String, max_peers: usize) -> Self {
        Self {
            id,
            created_by,
            peers: HashMap::new(),
            max_peers,
            last_activity: AtomicU64::new(now_secs()),
        }
    }

    pub fn touch(&self) {
        self.last_activity.store(now_secs(), Ordering::Relaxed);
    }

    pub fn idle_secs(&self) -> u64 {
        now_secs().saturating_sub(self.last_activity.load(Ordering::Relaxed))
    }

    pub fn is_full(&self) -> bool {
        self.peers.len() >= self.max_peers
    }

    pub fn is_full_for(&self, user_id: &str) -> bool {
        self.peers.len() >= self.max_peers && !self.peers.contains_key(user_id)
    }

    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }

    pub fn peer_ids(&self) -> Vec<String> {
        self.peers.keys().cloned().collect()
    }

    pub fn add_peer(&mut self, user_id: String, session_id: u64, tx: mpsc::Sender<SignalMessage>) -> Option<Peer> {
        self.touch();
        let peer = Peer {
            user_id: user_id.clone(),
            session_id,
            tx,
        };
        self.peers.insert(user_id, peer)
    }

    pub fn remove_peer(&mut self, user_id: &str) -> Option<Peer> {
        self.touch();
        self.peers.remove(user_id)
    }

    pub fn remove_peer_by_session(&mut self, user_id: &str, session_id: u64) -> bool {
        match self.peers.get(user_id) {
            Some(peer) if peer.session_id == session_id => {
                self.peers.remove(user_id);
                self.touch();
                true
            }
            _ => false,
        }
    }

    pub fn send_to(&self, user_id: &str, msg: SignalMessage) {
        if let Some(peer) = self.peers.get(user_id)
            && let Err(e) = peer.tx.try_send(msg)
        {
            tracing::warn!(
                peer = %user_id,
                room = %self.id,
                "Failed to send message to peer: {e}"
            );
        }
    }

    pub fn broadcast(&self, from: &str, msg: &SignalMessage) {
        for (id, peer) in &self.peers {
            if id != from
                && let Err(e) = peer.tx.try_send(msg.clone())
            {
                tracing::warn!(
                    peer = %id,
                    room = %self.id,
                    "Failed to broadcast to peer: {e}"
                );
            }
        }
    }

    pub fn broadcast_all(&self, msg: &SignalMessage) {
        for (id, peer) in &self.peers {
            if let Err(e) = peer.tx.try_send(msg.clone()) {
                tracing::warn!(
                    peer = %id,
                    room = %self.id,
                    "Failed to broadcast to peer: {e}"
                );
            }
        }
    }
}

pub fn new_room_id() -> String {
    Uuid::now_v7().to_string()
}
