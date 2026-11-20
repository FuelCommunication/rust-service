use crate::error::BoxError;

pub trait Encode<T: ?Sized>: Send + Sync {
    fn content_type(&self) -> &'static str;
    fn encode(&self, value: &T) -> Result<Vec<u8>, BoxError>;
}

pub trait Decode<T>: Send + Sync {
    fn decode(&self, bytes: &[u8]) -> Result<T, BoxError>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct JsonCodec;

impl<T: serde::Serialize + ?Sized> Encode<T> for JsonCodec {
    fn content_type(&self) -> &'static str {
        "application/json"
    }

    fn encode(&self, value: &T) -> Result<Vec<u8>, BoxError> {
        Ok(serde_json::to_vec(value)?)
    }
}

impl<T: serde::de::DeserializeOwned> Decode<T> for JsonCodec {
    fn decode(&self, bytes: &[u8]) -> Result<T, BoxError> {
        Ok(serde_json::from_slice(bytes)?)
    }
}

#[cfg(feature = "protobuf")]
#[derive(Debug, Clone, Copy, Default)]
pub struct ProtobufCodec;

#[cfg(feature = "protobuf")]
impl<T: prost::Message> Encode<T> for ProtobufCodec {
    fn content_type(&self) -> &'static str {
        "application/x-protobuf"
    }

    fn encode(&self, value: &T) -> Result<Vec<u8>, BoxError> {
        Ok(value.encode_to_vec())
    }
}

#[cfg(feature = "protobuf")]
impl<T: prost::Message + Default> Decode<T> for ProtobufCodec {
    fn decode(&self, bytes: &[u8]) -> Result<T, BoxError> {
        Ok(T::decode(bytes)?)
    }
}
