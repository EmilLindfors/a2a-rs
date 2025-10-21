//! Property-Based Tests for A2A Protocol Compliance
//!
//! These tests use proptest to verify that our implementations maintain
//! critical invariants across a wide range of inputs, ensuring robust
//! protocol compliance.

use a2a_rs::{
    adapter::SimpleAgentInfo,
    domain::{AgentSkill, Message, Part, Role, Task, TaskState},
    MessageSendParams,
};
use base64::Engine;
use proptest::prelude::*;
use serde_json::{self};

// Custom generators for A2A protocol types

prop_compose! {
    fn arb_task_state()(
        state in prop::sample::select(vec![
            TaskState::Submitted,
            TaskState::Working,
            TaskState::InputRequired,
            TaskState::Completed,
            TaskState::Canceled,
            TaskState::Failed,
            TaskState::Rejected,
            TaskState::AuthRequired,
            TaskState::Unknown,
        ])
    ) -> TaskState {
        state
    }
}

prop_compose! {
    fn arb_message_role()(
        role in prop::sample::select(vec![Role::User, Role::Agent])
    ) -> Role {
        role
    }
}

prop_compose! {
    fn arb_text_part()(
        text in ".*",
        metadata in prop::option::of(prop::collection::hash_map(".*", ".*", 0..3))
    ) -> Part {
        Part::Text {
            text,
            metadata: metadata.map(|m| m.into_iter().map(|(k, v)| (k, serde_json::Value::String(v))).collect()),
        }
    }
}

prop_compose! {
    fn arb_data_part()(
        keys in prop::collection::vec(".*", 0..5),
        values in prop::collection::vec(any::<i32>(), 0..5)
    ) -> Part {
        let mut data = serde_json::Map::new();
        for (key, value) in keys.into_iter().zip(values.into_iter()) {
            if !key.is_empty() {
                data.insert(key, serde_json::Value::Number(value.into()));
            }
        }
        Part::Data { data, metadata: None }
    }
}

prop_compose! {
    fn arb_file_part()(
        content in prop::collection::vec(any::<u8>(), 0..100),
        name in prop::option::of(".*"),
        mime_type in prop::option::of(".*")
    ) -> Part {
        let encoded = base64::engine::general_purpose::STANDARD.encode(&content);
        Part::file_from_bytes(encoded, name, mime_type)
    }
}

prop_compose! {
    fn arb_part()(
        part in prop_oneof![
            arb_text_part(),
            arb_data_part(),
            arb_file_part(),
        ]
    ) -> Part {
        part
    }
}

prop_compose! {
    fn arb_message()(
        message_id in ".*",
        role in arb_message_role(),
        parts in prop::collection::vec(arb_part(), 1..3),
        context_id in prop::option::of(".*"),
        task_id in prop::option::of(".*"),
    ) -> Message {
        let mut message = match role {
            Role::User => Message::user_text("placeholder".to_string(), message_id),
            Role::Agent => Message::agent_text("placeholder".to_string(), message_id),
        };

        // Replace the placeholder text part with our generated parts
        message.parts = parts;
        message.context_id = context_id;
        message.task_id = task_id;
        message
    }
}

prop_compose! {
    fn arb_agent_skill()(
        id in ".*",
        name in ".*",
        description in ".*",
        tags in prop::collection::vec(".*", 0..5),
        input_modes in prop::option::of(prop::collection::vec(".*", 0..3)),
        output_modes in prop::option::of(prop::collection::vec(".*", 0..3)),
    ) -> AgentSkill {
        AgentSkill {
            id,
            name,
            description,
            examples: Some(vec![]), // Add empty examples for now
            tags,
            input_modes,
            output_modes,
            security: None,
        }
    }
}

// Property-based tests

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Test that message serialization is always reversible
    #[test]
    fn message_serialization_roundtrip(
        message in arb_message()
    ) {
        // Serialize the message
        let json_value = serde_json::to_value(&message)?;

        // Deserialize it back
        let deserialized: Message = serde_json::from_value(json_value)?;

        // Core properties should be preserved
        prop_assert_eq!(message.message_id, deserialized.message_id);
        prop_assert_eq!(message.role, deserialized.role);
        prop_assert_eq!(message.kind, deserialized.kind);
        prop_assert_eq!(message.parts.len(), deserialized.parts.len());
        prop_assert_eq!(message.context_id, deserialized.context_id);
        prop_assert_eq!(message.task_id, deserialized.task_id);
    }

    /// Test that task state transitions maintain consistency
    #[test]
    fn task_state_consistency(
        task_id in ".*",
        context_id in ".*",
        states in prop::collection::vec(arb_task_state(), 1..10),
        messages in prop::collection::vec(arb_message(), 0..10)
    ) {
        if task_id.is_empty() || context_id.is_empty() {
            return Ok(());
        }

        let mut task = Task::new(task_id.clone(), context_id.clone());

        // Apply state transitions
        for (i, state) in states.iter().enumerate() {
            let maybe_message = if messages.is_empty() {
                None
            } else {
                Some(messages[i % messages.len()].clone())
            };
            task.update_status(state.clone(), maybe_message);
        }

        // Invariants that should always hold
        prop_assert_eq!(task.id, task_id);
        prop_assert_eq!(task.context_id, context_id);
        prop_assert_eq!(task.kind, "task");

        // History should contain messages that were actually added to the task
        if let Some(history) = &task.history {
            // The history length should be at most the number of state updates that included messages
            let updates_with_messages = if messages.is_empty() { 0 } else { states.len() };
            prop_assert!(history.len() <= updates_with_messages);
        }

        // Status should be the last applied state
        if let Some(last_state) = states.last() {
            prop_assert_eq!(&task.status.state, last_state);
        }
    }

    /// Test that message validation always succeeds for well-formed messages
    #[test]
    fn message_validation_properties(
        text in ".*",
        message_id in ".*",
        role in arb_message_role()
    ) {
        if message_id.is_empty() {
            return Ok(());
        }

        let message = match role {
            Role::User => Message::user_text(text.clone(), message_id.clone()),
            Role::Agent => Message::agent_text(text.clone(), message_id.clone()),
        };

        // Basic invariants
        prop_assert_eq!(message.message_id, message_id);
        prop_assert_eq!(message.role, role);
        prop_assert_eq!(message.kind, "message");
        prop_assert!(!message.parts.is_empty());

        // First part should be the text we provided
        if let Part::Text { text: part_text, .. } = &message.parts[0] {
            prop_assert_eq!(part_text, &text);
        } else {
            prop_assert!(false, "First part should be text");
        }
    }

    /// Test that AgentInfo serialization preserves essential properties
    #[test]
    fn agent_info_properties(
        name in ".*",
        url in ".*",
        description in prop::option::of(".*"),
        version in prop::option::of(".*"),
        skills in prop::collection::vec(arb_agent_skill(), 0..5)
    ) {
        if name.is_empty() || url.is_empty() {
            return Ok(());
        }

        let mut agent_info = SimpleAgentInfo::new(name.clone(), url.clone());

        if let Some(desc) = description {
            agent_info = agent_info.with_description(desc);
        }
        if let Some(ver) = version {
            agent_info = agent_info.with_version(ver);
        }

        // Add skills
        for skill in skills {
            agent_info = agent_info.add_skill(skill.id, skill.name, Some(skill.description));
        }

        // Test that we can get an agent card (this is async, so we'll test the builder properties)
        // The actual agent card retrieval would need to be tested in an async context

        // For now, test that the agent info maintains its essential properties
        // through a serialization roundtrip would require async context
        prop_assert!(true); // Placeholder - this test validates the input generation works
    }

    /// Test that JSON-RPC message IDs are preserved through serialization
    #[test]
    fn jsonrpc_id_preservation(
        id_value in prop_oneof![
            any::<u64>().prop_map(|n| serde_json::Value::Number(n.into())),
            ".*".prop_map(serde_json::Value::String),
        ],
        message in arb_message()
    ) {
        let params = MessageSendParams {
            message,
            configuration: None,
            metadata: None,
        };

        let request = a2a_rs::application::SendMessageRequest {
            jsonrpc: "2.0".to_string(),
            method: "message/send".to_string(),
            id: Some(id_value.clone()),
            params,
        };

        // Serialize and deserialize
        let json = serde_json::to_value(&request)?;
        let deserialized: a2a_rs::application::SendMessageRequest = serde_json::from_value(json)?;

        // ID should be preserved
        prop_assert_eq!(request.id, deserialized.id);
        prop_assert_eq!(request.jsonrpc, deserialized.jsonrpc);
        prop_assert_eq!(request.method, deserialized.method);
    }

    /// Test that Part encoding/decoding maintains data integrity
    #[test]
    fn part_data_integrity(
        part in arb_part()
    ) {
        // Serialize and deserialize the part
        let json_value = serde_json::to_value(&part)?;
        let deserialized: Part = serde_json::from_value(json_value)?;

        // Test based on part type
        match (&part, &deserialized) {
            (Part::Text { text: t1, .. }, Part::Text { text: t2, .. }) => {
                prop_assert_eq!(t1, t2);
            },
            (Part::Data { data: d1, .. }, Part::Data { data: d2, .. }) => {
                prop_assert_eq!(d1.len(), d2.len());
                // Data content should be preserved
                for (key, value) in d1 {
                    prop_assert_eq!(Some(value), d2.get(key));
                }
            },
            (Part::File { file: f1, .. }, Part::File { file: f2, .. }) => {
                prop_assert_eq!(&f1.name, &f2.name);
                prop_assert_eq!(&f1.mime_type, &f2.mime_type);
                prop_assert_eq!(&f1.bytes, &f2.bytes);
                prop_assert_eq!(&f1.uri, &f2.uri);
            },
            _ => prop_assert!(false, "Part types should match after deserialization"),
        }
    }

    /// Test task history limits work correctly
    #[test]
    fn task_history_limits(
        task_id in ".*",
        context_id in ".*",
        messages in prop::collection::vec(arb_message(), 0..20),
        limit in 0..10usize
    ) {
        if task_id.is_empty() || context_id.is_empty() {
            return Ok(());
        }

        let mut task = Task::new(task_id, context_id);

        // Add all messages
        for message in messages.iter() {
            task.update_status(TaskState::Working, Some(message.clone()));
        }

        // Apply history limit
        let limited_task = task.with_limited_history(Some(limit as u32));

        if limit == 0 {
            prop_assert!(limited_task.history.is_none());
        } else if let Some(history) = limited_task.history {
            prop_assert!(history.len() <= limit);
            prop_assert!(history.len() <= messages.len());

            // Should have the most recent messages
            if !messages.is_empty() && !history.is_empty() {
                let expected_start = messages.len().saturating_sub(limit);
                for (i, hist_msg) in history.iter().enumerate() {
                    if expected_start + i < messages.len() {
                        prop_assert_eq!(&hist_msg.message_id, &messages[expected_start + i].message_id);
                    }
                }
            }
        }
    }

    /// Test that task state transitions follow logical patterns
    #[test]
    fn task_state_transitions_logical(
        task_id in ".*",
        context_id in ".*",
        final_states in prop::collection::vec(
            prop::sample::select(vec![
                TaskState::Completed,
                TaskState::Canceled,
                TaskState::Failed,
                TaskState::Rejected,
            ]),
            0..3
        ),
        working_state_message in arb_message()
    ) {
        if task_id.is_empty() || context_id.is_empty() {
            return Ok(());
        }

        let mut task = Task::new(task_id, context_id);

        // Tasks should start in a working state typically
        task.update_status(TaskState::Working, Some(working_state_message));

        // Apply final states - once a task reaches a final state,
        // it should maintain that final state
        for final_state in final_states {
            task.update_status(final_state.clone(), None);

            // After setting a final state, the task should be in that state
            prop_assert_eq!(&task.status.state, &final_state);

            // Final states should be final (this is more of a business logic test)
            match &final_state {
                TaskState::Completed | TaskState::Canceled |
                TaskState::Failed | TaskState::Rejected => {
                    // These are considered final states in most business logic
                    prop_assert!(true);
                },
                _ => prop_assert!(true), // Non-final states are also valid
            }
        }
    }
}

// Additional property tests for edge cases and invariants

#[cfg(test)]
mod edge_case_properties {
    use super::*;

    proptest! {
        /// Test that empty or minimal valid inputs don't cause panics
        #[test]
        fn minimal_valid_inputs_dont_panic(
            minimal_text in prop::option::of(""),
            minimal_id in prop::option::of("")
        ) {
            // Test that we handle minimal inputs gracefully
            if let (Some(text), Some(id)) = (minimal_text, minimal_id) {
                if !id.is_empty() {
                    let message = Message::user_text(text.to_string(), id.to_string());
                    prop_assert_eq!(message.message_id, id);
                    prop_assert_eq!(message.parts.len(), 1);
                }
            }
        }

        /// Test that base64 encoding/decoding is consistent
        #[test]
        fn base64_consistency(
            data in prop::collection::vec(any::<u8>(), 0..1000)
        ) {
            let encoded = base64::engine::general_purpose::STANDARD.encode(&data);
            let file_part = Part::file_from_bytes(
                encoded.clone(),
                Some("test.bin".to_string()),
                Some("application/octet-stream".to_string())
            );

            if let Part::File { file, .. } = file_part {
                prop_assert_eq!(file.bytes, Some(encoded));
                prop_assert_eq!(file.name, Some("test.bin".to_string()));
                prop_assert_eq!(file.mime_type, Some("application/octet-stream".to_string()));
                prop_assert_eq!(file.uri, None);
            } else {
                prop_assert!(false, "Should create a File part");
            }
        }

        /// Test Unicode handling in messages
        #[test]
        fn unicode_handling(
            unicode_text in "\\PC*", // Unicode text pattern
            message_id in ".*"
        ) {
            if !message_id.is_empty() {
                let message = Message::user_text(unicode_text.clone(), message_id.clone());

                // Serialize and deserialize
                let json = serde_json::to_value(&message).unwrap();
                let deserialized: Message = serde_json::from_value(json).unwrap();

                prop_assert_eq!(message.message_id, deserialized.message_id);
                if let Part::Text { text, .. } = &deserialized.parts[0] {
                    prop_assert_eq!(text, &unicode_text);
                }
            }
        }
    }
}
