use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Offer { to: String, sdp: String },
    Answer { to: String, sdp: String },
    IceCandidate { to: String, candidate: String },
}

impl ClientMessage {
    pub fn validate_size(&self, max_size: usize) -> Result<(), &'static str> {
        match self {
            ClientMessage::Offer { sdp, .. } | ClientMessage::Answer { sdp, .. } => {
                if sdp.len() > max_size {
                    return Err("SDP payload too large");
                }
            }
            ClientMessage::IceCandidate { candidate, .. } => {
                if candidate.len() > max_size {
                    return Err("ICE candidate payload too large");
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SignalMessage {
    Joined { user_id: String, peers: Vec<String> },
    UserJoined { user_id: String },
    UserLeft { user_id: String },
    Offer { from: String, sdp: String },
    Answer { from: String, sdp: String },
    IceCandidate { from: String, candidate: String },
    Error { message: String },
}
