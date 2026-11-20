use rdkafka::message::{Header, Headers as _, Message as _, OwnedHeaders, OwnedMessage};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Headers(Vec<(String, Option<Vec<u8>>)>);

impl Headers {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(mut self, key: impl Into<String>, value: impl Into<Vec<u8>>) -> Self {
        self.0.push((key.into(), Some(value.into())));
        self
    }

    pub fn push(&mut self, key: impl Into<String>, value: impl Into<Vec<u8>>) {
        self.0.push((key.into(), Some(value.into())));
    }

    pub fn get(&self, key: &str) -> Option<&[u8]> {
        self.0.iter().find(|(k, _)| k == key).and_then(|(_, v)| v.as_deref())
    }

    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(|v| std::str::from_utf8(v).ok())
    }

    pub fn contains(&self, key: &str) -> bool {
        self.0.iter().any(|(k, _)| k == key)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, Option<&[u8]>)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_deref()))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(crate) fn to_owned_headers(&self) -> OwnedHeaders {
        self.0
            .iter()
            .fold(OwnedHeaders::new_with_capacity(self.0.len()), |h, (key, value)| {
                h.insert(Header {
                    key,
                    value: value.as_deref(),
                })
            })
    }

    pub(crate) fn from_rdkafka(headers: Option<&rdkafka::message::BorrowedHeaders>) -> Self {
        let Some(headers) = headers else {
            return Self::default();
        };
        Self(
            headers
                .iter()
                .map(|h| (h.key.to_owned(), h.value.map(<[u8]>::to_vec)))
                .collect(),
        )
    }
}

#[derive(Debug, Clone)]
pub struct Record {
    pub topic: String,
    pub partition: i32,
    pub offset: i64,
    pub key: Option<Vec<u8>>,
    pub payload: Option<Vec<u8>>,
    pub headers: Headers,
    pub timestamp: Option<i64>,
}

impl Record {
    pub(crate) fn from_borrowed(msg: &rdkafka::message::BorrowedMessage<'_>) -> Self {
        let owned: OwnedMessage = msg.detach();
        Self {
            topic: owned.topic().to_owned(),
            partition: owned.partition(),
            offset: owned.offset(),
            key: owned.key().map(<[u8]>::to_vec),
            payload: owned.payload().map(<[u8]>::to_vec),
            headers: Headers::from_rdkafka(msg.headers()),
            timestamp: owned.timestamp().to_millis(),
        }
    }

    pub fn key_str(&self) -> Option<&str> {
        self.key.as_deref().and_then(|k| std::str::from_utf8(k).ok())
    }
}

#[derive(Debug, Clone)]
pub struct Message<T> {
    pub value: T,
    pub record: Record,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeliveryReport {
    pub partition: i32,
    pub offset: i64,
}
