//! Wire-format probe + golden tests for the JSON-RPC / HTTP+JSON adapter.
//!
//! These tests pin down whether the `buffa`-generated domain types serialize as
//! canonical **ProtoJSON** (the wire format the A2A spec and the official SDK
//! use). If they do, the JSON-RPC adapter can serialize the generated types
//! directly (plan "Option A"); the only hand-written serde it adds is the
//! tag-free field-presence unions.
//!
//! The `probe_*` tests print the actual JSON so the exact shape (Timestamp
//! format, `Struct`/metadata shape, `bytes` encoding) is visible in test output.

#![cfg(feature = "server")]

use a2a_rs::domain::{Message, Part, Task, TaskState, TaskStatus};

/// Recursively sort object keys so two JSON values compare modulo key order.
fn canonical(mut v: serde_json::Value) -> serde_json::Value {
    fn sort(v: &mut serde_json::Value) {
        match v {
            serde_json::Value::Object(map) => {
                let mut sorted: std::collections::BTreeMap<String, serde_json::Value> =
                    std::mem::take(map).into_iter().collect();
                for val in sorted.values_mut() {
                    sort(val);
                }
                *map = sorted.into_iter().collect();
            }
            serde_json::Value::Array(arr) => arr.iter_mut().for_each(sort),
            _ => {}
        }
    }
    sort(&mut v);
    v
}

// ---------------------------------------------------------------------------
// Probes — print real output to settle R1 (Timestamp) and R2 (Struct/metadata).
// ---------------------------------------------------------------------------

#[test]
fn timestamp_serializes_as_rfc3339() {
    // R1: `google.protobuf.Timestamp` must be an RFC3339 string, not
    // `{seconds, nanos}`. If this ever regresses, any Timestamp-bearing type
    // needs an Option-B wire conversion.
    let status = TaskStatus {
        state: buffa::EnumValue::from(TaskState::TASK_STATE_WORKING),
        timestamp: buffa::MessageField::some(buffa_types::google::protobuf::Timestamp {
            seconds: 1_700_000_000,
            nanos: 0,
            ..Default::default()
        }),
        ..Default::default()
    };
    let json = serde_json::to_value(&status).unwrap();
    assert_eq!(
        canonical(json),
        canonical(serde_json::json!({
            "state": "TASK_STATE_WORKING",
            "timestamp": "2023-11-14T22:13:20Z",
        })),
    );
}

#[test]
fn metadata_struct_serializes_as_bare_object() {
    // R2: `google.protobuf.Struct` must be a bare JSON object, not
    // `{fields: {...}}`. Note proto `Struct` numbers are doubles (42 -> 42.0).
    let mut message = Message::user_text("hello".to_string(), "msg-1".to_string());
    let struct_val: buffa_types::google::protobuf::Struct = serde_json::from_value(
        serde_json::json!({ "foo": "bar", "n": 42, "nested": { "a": true } }),
    )
    .unwrap();
    message.metadata = buffa::MessageField::some(struct_val);
    let json = serde_json::to_value(&message).unwrap();
    assert_eq!(
        canonical(json),
        canonical(serde_json::json!({
            "messageId": "msg-1",
            "role": "ROLE_USER",
            "parts": [{ "text": "hello" }],
            "metadata": { "foo": "bar", "n": 42.0, "nested": { "a": true } },
        })),
    );
}

#[test]
fn bytes_part_serializes_as_base64_under_raw() {
    let part = Part {
        content: Some(a2a_rs::domain::generated::part::Content::Raw(vec![
            1, 2, 3, 255,
        ])),
        ..Default::default()
    };
    let json = serde_json::to_value(&part).unwrap();
    assert_eq!(
        canonical(json),
        canonical(serde_json::json!({ "raw": "AQID/w==" }))
    );
}

#[test]
fn enums_and_oneof_are_tag_free_proto_names() {
    let message = Message::agent_text("hi".to_string(), "m2".to_string());
    let json = serde_json::to_value(&message).unwrap();
    // Role must be the SCREAMING_SNAKE proto name, not an int.
    assert_eq!(json["role"], serde_json::json!("ROLE_AGENT"));
    // Text part flattens to {"text": "hi"} with no discriminator tag.
    assert_eq!(json["parts"][0]["text"], serde_json::json!("hi"));
}

#[test]
fn message_round_trips_through_protojson() {
    // Round-trip catches alias/`skip_if`/`null_as_default` asymmetry: a wire
    // body deserializes into the domain type and re-serializes byte-identically.
    let wire = serde_json::json!({
        "messageId": "m3",
        "contextId": "ctx",
        "role": "ROLE_USER",
        "parts": [{ "text": "round" }, { "raw": "AQID/w==" }],
    });
    let message: Message = serde_json::from_value(wire.clone()).unwrap();
    let back = serde_json::to_value(&message).unwrap();
    assert_eq!(canonical(back), canonical(wire));
}

#[test]
fn task_serializes_as_protojson() {
    let task = Task {
        id: "task-1".to_string(),
        context_id: "ctx-1".to_string(),
        status: buffa::MessageField::some(TaskStatus {
            state: buffa::EnumValue::from(TaskState::TASK_STATE_COMPLETED),
            ..Default::default()
        }),
        ..Default::default()
    };
    let json = serde_json::to_value(&task).unwrap();
    assert_eq!(json["id"], serde_json::json!("task-1"));
    assert_eq!(json["contextId"], serde_json::json!("ctx-1"));
    assert_eq!(
        json["status"]["state"],
        serde_json::json!("TASK_STATE_COMPLETED")
    );
    // proto3 default fields (empty artifacts/history) must be omitted.
    assert!(
        json.get("artifacts").is_none(),
        "empty artifacts should be omitted"
    );
}

#[test]
fn canonical_sorts_keys() {
    let a = serde_json::json!({ "b": 1, "a": 2 });
    let b = serde_json::json!({ "a": 2, "b": 1 });
    assert_eq!(canonical(a), canonical(b));
}

#[test]
fn generated_response_types_are_field_presence_unions() {
    // The generated `SendMessageResponse`/`StreamResponse` oneofs already
    // serialize tag-free, so the adapter reuses them as the JSON-RPC `result`
    // rather than hand-writing union serde.
    use a2a_rs::domain::generated::{
        SendMessageResponse, StreamResponse, TaskStatusUpdateEvent, send_message_response,
        stream_response,
    };
    let r = SendMessageResponse {
        payload: Some(send_message_response::Payload::Task(Box::new(Task {
            id: "t1".into(),
            ..Default::default()
        }))),
        ..Default::default()
    };
    assert_eq!(
        canonical(serde_json::to_value(&r).unwrap()),
        canonical(serde_json::json!({ "task": { "id": "t1" } })),
    );

    let s = StreamResponse {
        payload: Some(stream_response::Payload::StatusUpdate(Box::new(
            TaskStatusUpdateEvent {
                task_id: "t1".into(),
                ..Default::default()
            },
        ))),
        ..Default::default()
    };
    assert_eq!(
        canonical(serde_json::to_value(&s).unwrap()),
        canonical(serde_json::json!({ "statusUpdate": { "taskId": "t1" } })),
    );
}
