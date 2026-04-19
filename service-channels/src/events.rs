use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum ChannelEvent {
    ChannelUpdated { channel_id: String },
    ChannelDeleted { channel_id: String },
    UserSubscribed { channel_id: String, user_id: String },
    UserUnsubscribed { channel_id: String, user_id: String },
}
