//! Tests for a2a-mcp integration

#[cfg(test)]
mod message_converter_tests {
    use crate::message::MessageConverter;
    use a2a_rs::domain::message::{Message, MessagePart};
    use rmcp::{ClientJsonRpcMessage, ServerJsonRpcMessage};
    use serde_json::json;

    #[test]
    fn test_rmcp_to_a2a_request() {
        let converter = MessageConverter::new();
        
        let rmcp_request = ClientJsonRpcMessage {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "test_method".to_string(),
            params: Some(json!({"key": "value"})),
        };
        
        let a2a_message = converter.rmcp_to_a2a_request(&rmcp_request).unwrap();
        
        assert_eq!(a2a_message.role, "user");
        assert_eq!(a2a_message.parts.len(), 2);
        
        // Check text part
        match &a2a_message.parts[0] {
            MessagePart::Text { text } => {
                assert!(text.contains("test_method"));
            },
            _ => panic!("Expected text part"),
        }
        
        // Check data part
        match &a2a_message.parts[1] {
            MessagePart::Data { data, .. } => {
                assert_eq!(data["key"], "value");
            },
            _ => panic!("Expected data part"),
        }
    }

    #[test]
    fn test_a2a_to_rmcp_response() {
        let converter = MessageConverter::new();
        
        let a2a_message = Message {
            role: "agent".to_string(),
            parts: vec![
                MessagePart::Text { text: "Test response".to_string() },
                MessagePart::Data { 
                    data: json!({"result": "success"}),
                    mime_type: Some("application/json".to_string()),
                },
            ],
        };
        
        let id = Some(json!(123));
        let rmcp_response = converter.a2a_to_rmcp_response(&a2a_message, id.clone()).unwrap();
        
        assert_eq!(rmcp_response.jsonrpc, "2.0");
        assert_eq!(rmcp_response.id, id);
        assert!(rmcp_response.error.is_none());
        
        // Data part should be prioritized over text
        assert_eq!(rmcp_response.result.unwrap()["result"], "success");
    }
}

#[cfg(test)]
mod adapter_tests {
    use crate::adapter::{ToolToAgentAdapter, AgentToToolAdapter};
    use rmcp::{Tool, ToolCall};
    use serde_json::json;

    #[test]
    fn test_tool_to_agent_adapter() {
        let tools = vec![
            Tool {
                name: "test_tool".to_string(),
                description: "A test tool".to_string(),
                parameters: None,
            },
        ];
        
        let adapter = ToolToAgentAdapter::new(
            tools, 
            "Test Agent".to_string(),
            "An agent for testing".to_string(),
        );
        
        let agent_card = adapter.generate_agent_card();
        
        assert_eq!(agent_card.name, "Test Agent");
        assert_eq!(agent_card.description, "An agent for testing");
        assert_eq!(agent_card.skills.len(), 1);
        assert_eq!(agent_card.skills[0].name, "test_tool");
    }

    #[test]
    fn test_tool_call_to_task() {
        let tools = vec![
            Tool {
                name: "test_tool".to_string(),
                description: "A test tool".to_string(),
                parameters: None,
            },
        ];
        
        let adapter = ToolToAgentAdapter::new(
            tools, 
            "Test Agent".to_string(),
            "An agent for testing".to_string(),
        );
        
        let tool_call = ToolCall {
            method: "test_tool".to_string(),
            params: json!({"input": "test_input"}),
        };
        
        let task = adapter.tool_call_to_task(&tool_call).unwrap();
        
        assert_eq!(task.status.state, a2a_rs::domain::task::TaskState::Submitted);
        assert_eq!(task.messages.len(), 1);
        assert_eq!(task.messages[0].role, "user");
    }
}