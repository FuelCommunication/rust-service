use kafka::schemas::{Action, KafkaMessage};
use serde::{Deserialize, Serialize};

#[test]
fn test_kafka_message_serialization() -> anyhow::Result<()> {
    let msg = KafkaMessage {
        user_id: "user123".to_owned(),
        action: Action::Create,
        data: Some("test data".to_owned()),
    };

    let json = serde_json::to_string(&msg)?;
    assert!(json.contains("user123"));
    assert!(json.contains("create"));
    assert!(json.contains("test data"));

    Ok(())
}

#[test]
fn test_kafka_message_deserialization() -> anyhow::Result<()> {
    let json = r#"{
            "user_id": "user456",
            "action": "update",
            "data": "updated data"
        }"#;

    let msg = serde_json::from_str::<KafkaMessage>(json)?;
    assert_eq!(msg.user_id, "user456");
    assert_eq!(msg.action, Action::Update);
    assert_eq!(msg.data, Some("updated data".to_string()));

    Ok(())
}

#[test]
fn test_kafka_message_with_generic_data() -> anyhow::Result<()> {
    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct CustomData {
        field1: String,
        field2: i32,
    }

    let custom_data = CustomData {
        field1: "test".to_string(),
        field2: 42,
    };

    let msg = KafkaMessage {
        user_id: "user789".to_string(),
        action: Action::Delete,
        data: Some(custom_data),
    };

    let json = serde_json::to_string(&msg)?;
    let deserialized = serde_json::from_str::<KafkaMessage<CustomData>>(&json)?;

    assert_eq!(deserialized.user_id, "user789");
    assert_eq!(deserialized.action, Action::Delete);
    assert_eq!(deserialized.data.unwrap().field2, 42);

    Ok(())
}

#[test]
fn test_kafka_message_without_data() -> anyhow::Result<()> {
    let msg: KafkaMessage<String> = KafkaMessage {
        user_id: "user000".to_string(),
        action: Action::Delete,
        data: None,
    };

    let json = serde_json::to_string(&msg)?;
    let deserialized: KafkaMessage<String> = serde_json::from_str(&json)?;

    assert_eq!(deserialized.data, None);
    Ok(())
}

#[test]
fn test_action_serialization() -> anyhow::Result<()> {
    assert_eq!(serde_json::to_string(&Action::Create)?, r#""create""#);
    assert_eq!(serde_json::to_string(&Action::Update)?, r#""update""#);
    assert_eq!(serde_json::to_string(&Action::Delete)?, r#""delete""#);

    Ok(())
}
