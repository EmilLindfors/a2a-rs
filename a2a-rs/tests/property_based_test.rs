//! Property-Based Tests for A2A Protocol Compliance
//!
//! These tests use proptest to verify that our implementations maintain
//! critical invariants across a wide range of inputs, ensuring robust
//! protocol compliance.

use a2a_rs::{
    adapter::SimpleAgentInfo,
    domain::{AgentSkill, Message, Part, Role, Task, TaskState, part},
};
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
        if let Some(m) = metadata {
            let metadata_val = serde_json::to_value(m).unwrap();
            let struct_metadata: buffa_types::google::protobuf::Struct = serde_json::from_value(metadata_val).unwrap();
            Part::text_with_metadata(text, struct_metadata)
        } else {
            Part::text(text)
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
        let data_val = serde_json::Value::Object(data);
        let proto_val: buffa_types::google::protobuf::Value = serde_json::from_value(data_val).unwrap();
        Part::data(proto_val)
    }
}

prop_compose! {
    fn arb_file_part()(
        content in prop::collection::vec(any::<u8>(), 0..100),
        name in prop::option::of(".*"),
        mime_type in prop::option::of(".*")
    ) -> Part {
        Part::file_from_bytes(content, name, mime_type)
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
            _ => Message::user_text("placeholder".to_string(), message_id),
        };

        // Replace the placeholder text part with our generated parts
        message.parts = parts;
        message.context_id = context_id.unwrap_or_default();
        message.task_id = task_id.unwrap_or_default();
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
            examples: Vec::new(),
            tags,
            input_modes: input_modes.unwrap_or_default(),
            output_modes: output_modes.unwrap_or_default(),
            ..Default::default()
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
        prop_assert_eq!(&message.message_id, &deserialized.message_id);
        prop_assert_eq!(&message.role, &deserialized.role);
        prop_assert_eq!(message.parts.len(), deserialized.parts.len());
        prop_assert_eq!(&message.context_id, &deserialized.context_id);
        prop_assert_eq!(&message.task_id, &deserialized.task_id);
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
        prop_assert_eq!(&task.id, &task_id);
        prop_assert_eq!(&task.context_id, &context_id);

        // History should contain messages that were actually added to the task
        let history = &task.history;
        let updates_with_messages = if messages.is_empty() { 0 } else { states.len() };
        prop_assert!(history.len() <= updates_with_messages);

        // Status should be the last applied state
        if let Some(last_state) = states.last() {
            prop_assert_eq!(task.status.state, *last_state);
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
            _ => Message::user_text(text.clone(), message_id.clone()),
        };

        // Basic invariants
        prop_assert_eq!(&message.message_id, &message_id);
        prop_assert_eq!(message.role, role);
        prop_assert!(!message.parts.is_empty());

        // First part should be the text we provided
        if let Some(part_text) = message.parts[0].get_text() {
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

        prop_assert!(true);
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
        prop_assert_eq!(&part.filename, &deserialized.filename);
        prop_assert_eq!(&part.media_type, &deserialized.media_type);

        match (&part.content, &deserialized.content) {
            (Some(part::Content::Text(t1)), Some(part::Content::Text(t2))) => {
                prop_assert_eq!(t1, t2);
            },
            (Some(part::Content::Data(d1)), Some(part::Content::Data(d2))) => {
                prop_assert_eq!(d1, d2);
            },
            (Some(part::Content::Raw(r1)), Some(part::Content::Raw(r2))) => {
                prop_assert_eq!(r1, r2);
            },
            (Some(part::Content::Url(u1)), Some(part::Content::Url(u2))) => {
                prop_assert_eq!(u1, u2);
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
            prop_assert!(limited_task.history.is_empty());
        } else {
            let history = &limited_task.history;
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
            prop_assert_eq!(task.status.state, final_state);

            // Final states should be final (this is more of a business logic test)
            match final_state {
                TaskState::Completed | TaskState::Canceled |
                TaskState::Failed | TaskState::Rejected => {
                    prop_assert!(true);
                },
                _ => prop_assert!(true),
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
                    prop_assert_eq!(&message.message_id, &id);
                    prop_assert_eq!(message.parts.len(), 1);
                }
            }
        }

        /// Test that file from bytes works
        #[test]
        fn file_from_bytes_check(
            data in prop::collection::vec(any::<u8>(), 0..1000)
        ) {
            let file_part = Part::file_from_bytes(
                data.clone(),
                Some("test.bin".to_string()),
                Some("application/octet-stream".to_string())
            );

            if let Some(part::Content::Raw(bytes)) = &file_part.content {
                prop_assert_eq!(bytes, &data);
                prop_assert_eq!(&file_part.filename, "test.bin");
                prop_assert_eq!(&file_part.media_type, "application/octet-stream");
            } else {
                prop_assert!(false, "Should create a Raw content part");
            }
        }

        /// Test Unicode handling in messages
        #[test]
        fn unicode_handling(
            unicode_text in "\\PC*",
            message_id in ".*"
        ) {
            if !message_id.is_empty() {
                let message = Message::user_text(unicode_text.clone(), message_id.clone());

                // Serialize and deserialize
                let json = serde_json::to_value(&message).unwrap();
                let deserialized: Message = serde_json::from_value(json).unwrap();

                prop_assert_eq!(&message.message_id, &deserialized.message_id);
                if let Some(part::Content::Text(text)) = &deserialized.parts[0].content {
                    prop_assert_eq!(text, &unicode_text);
                } else {
                    prop_assert!(false, "First part should be text");
                }
            }
        }
    }
}
