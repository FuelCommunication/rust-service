use crate::{ValkeyError, ValkeyResult};
use futures_util::{Stream, StreamExt as _};
use redis::{Msg, aio::PubSub};
use serde::de::DeserializeOwned;
use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

#[derive(Debug, Clone)]
pub struct PubSubMessage {
    pub channel: String,
    pub pattern: Option<String>,
    pub payload: Vec<u8>,
}

impl PubSubMessage {
    pub fn decode<T: DeserializeOwned>(&self) -> ValkeyResult<T> {
        serde_json::from_slice(&self.payload).map_err(|e| ValkeyError::decode(&self.channel, e))
    }

    pub fn payload_str(&self) -> Option<&str> {
        std::str::from_utf8(&self.payload).ok()
    }
}

pub struct Subscription {
    stream: Pin<Box<dyn Stream<Item = Msg> + Send>>,
    prefix: Option<Arc<str>>,
}

impl Subscription {
    pub(crate) fn new(pubsub: PubSub, prefix: Option<Arc<str>>) -> Self {
        Self {
            stream: Box::pin(pubsub.into_on_message()),
            prefix,
        }
    }

    fn strip(&self, name: &str) -> String {
        match &self.prefix {
            Some(prefix) => name
                .strip_prefix(&**prefix)
                .and_then(|n| n.strip_prefix(':'))
                .unwrap_or(name)
                .to_owned(),
            None => name.to_owned(),
        }
    }

    fn convert(&self, msg: Msg) -> PubSubMessage {
        let pattern = msg
            .from_pattern()
            .then(|| msg.get_pattern::<String>().ok())
            .flatten()
            .map(|p| self.strip(&p));
        PubSubMessage {
            channel: self.strip(msg.get_channel_name()),
            pattern,
            payload: msg.get_payload_bytes().to_vec(),
        }
    }

    pub async fn next_message(&mut self) -> Option<PubSubMessage> {
        let msg = self.stream.next().await?;
        Some(self.convert(msg))
    }
}

impl Stream for Subscription {
    type Item = PubSubMessage;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.stream.as_mut().poll_next(cx) {
            Poll::Ready(Some(msg)) => Poll::Ready(Some(self.convert(msg))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}
