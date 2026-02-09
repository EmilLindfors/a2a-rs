//! A2A Agents with Tool Calling
//!
//! Agents that can call each other's specialized tools/capabilities

use genai::Client;
use genai::chat::{ChatMessage, ChatRequest, Tool, ToolCall, ToolResponse};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

#[derive(Clone)]
struct ToolAgent {
    id: String,
    name: String,
    persona: String,
    model: String,
    tools: Vec<Tool>,
    conversation_history: Vec<ChatMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct A2AToolMessage {
    from_agent: String,
    to_agent: String,
    content: String,
    tool_calls: Vec<ToolCall>,
    tool_responses: Vec<ToolResponse>,
    turn: usize,
}

impl ToolAgent {
    fn new(id: &str, name: &str, persona: &str, model: &str, tools: Vec<Tool>) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            persona: persona.to_string(),
            model: model.to_string(),
            tools,
            conversation_history: vec![ChatMessage::system(persona)],
        }
    }

    /// Execute a tool call locally (simulated)
    async fn execute_tool(&self, tool_call: &ToolCall) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        // Simulate tool execution
        match tool_call.fn_name.as_str() {
            "search_database" => {
                let args = &tool_call.fn_arguments;
                Ok(json!({
                    "results": [
                        {"id": 1, "title": "Rust Programming", "relevance": 0.95},
                        {"id": 2, "title": "System Design", "relevance": 0.87}
                    ],
                    "query": args.get("query"),
                    "count": 2
                }))
            }
            "analyze_data" => {
                Ok(json!({
                    "analysis": "Processed data shows positive trend",
                    "confidence": 0.89,
                    "metrics": {"mean": 42.5, "std": 12.3}
                }))
            }
            "generate_code" => {
                let args = &tool_call.fn_arguments;
                Ok(json!({
                    "code": "fn hello() {\n    println!(\"Hello, world!\");\n}",
                    "language": args.get("language").and_then(|v| v.as_str()).unwrap_or("rust"),
                    "explanation": "Simple hello function"
                }))
            }
            _ => Ok(json!({"error": "Unknown tool"}))
        }
    }

    async fn process_with_tools(
        &mut self,
        client: &Client,
        incoming: &A2AToolMessage,
    ) -> Result<A2AToolMessage, Box<dyn std::error::Error>> {
        // Add incoming message to history
        self.conversation_history.push(ChatMessage::user(&incoming.content));

        // Create chat request with tools
        let chat_req = ChatRequest::new(self.conversation_history.clone())
            .with_tools(self.tools.clone());

        println!("\n🤖 {} processing with {} tools available...", self.name, self.tools.len());

        // Get LLM response (may include tool calls)
        let chat_res = client.exec_chat(&self.model, chat_req.clone(), None).await?;

        // Check for tool calls
        let tool_calls = chat_res.clone().into_tool_calls();
        let mut tool_responses = Vec::new();

        if !tool_calls.is_empty() {
            println!("   🔧 {} tool call(s) requested", tool_calls.len());

            for tool_call in &tool_calls {
                println!("   📞 Calling: {}({})", tool_call.fn_name, tool_call.fn_arguments);

                let result = self.execute_tool(tool_call).await?;
                let response = ToolResponse::new(
                    tool_call.call_id.clone(),
                    result.to_string(),
                );

                tool_responses.push(response.clone());
                println!("   ✅ Result: {}", result);
            }

            // Add tool calls and responses to history
            self.conversation_history.push(ChatMessage::from(tool_calls.clone()));
            for response in &tool_responses {
                self.conversation_history.push(ChatMessage::from(response.clone()));
            }

            // Get final response with tool results
            let final_req = ChatRequest::new(self.conversation_history.clone());
            let final_res = client.exec_chat(&self.model, final_req, None).await?;

            let final_text = final_res.first_text().unwrap_or("").to_string();
            self.conversation_history.push(ChatMessage::assistant(&final_text));

            Ok(A2AToolMessage {
                from_agent: self.id.clone(),
                to_agent: incoming.from_agent.clone(),
                content: final_text,
                tool_calls,
                tool_responses,
                turn: incoming.turn + 1,
            })
        } else {
            // No tools needed, direct response
            let response_text = chat_res.first_text().unwrap_or("").to_string();
            self.conversation_history.push(ChatMessage::assistant(&response_text));

            Ok(A2AToolMessage {
                from_agent: self.id.clone(),
                to_agent: incoming.from_agent.clone(),
                content: response_text,
                tool_calls: Vec::new(),
                tool_responses: Vec::new(),
                turn: incoming.turn + 1,
            })
        }
    }
}

struct ToolAgentOrchestrator {
    agents: HashMap<String, ToolAgent>,
    client: Client,
}

impl ToolAgentOrchestrator {
    fn new() -> Self {
        Self {
            agents: HashMap::new(),
            client: Client::default(),
        }
    }

    fn add_agent(&mut self, agent: ToolAgent) {
        self.agents.insert(agent.id.clone(), agent);
    }

    async fn run_tool_conversation(
        &mut self,
        agent1_id: &str,
        agent2_id: &str,
        task: &str,
        max_turns: usize,
    ) -> Result<(), Box<dyn std::error::Error>> {
        println!("\n{}", "=".repeat(80));
        println!("🛠️  A2A TOOL-ENABLED AGENT CONVERSATION");
        println!("{}", "=".repeat(80));
        println!("Task: {}", task);
        println!("{}\n", "=".repeat(80));

        let mut current_message = A2AToolMessage {
            from_agent: agent1_id.to_string(),
            to_agent: agent2_id.to_string(),
            content: task.to_string(),
            tool_calls: Vec::new(),
            tool_responses: Vec::new(),
            turn: 0,
        };

        for turn in 0..max_turns {
            let responding_agent_id = current_message.to_agent.clone();
            let agent = self.agents.get_mut(&responding_agent_id)
                .ok_or("Agent not found")?;

            println!("\n{}", "-".repeat(80));
            println!("Turn {}: {} → {}", turn + 1, current_message.from_agent, responding_agent_id);
            println!("{}", "-".repeat(80));
            println!("📨 Request: {}", current_message.content);

            let response = agent.process_with_tools(&self.client, &current_message).await?;

            println!("💬 Response: {}", response.content);

            if !response.tool_calls.is_empty() {
                println!("   Tools used: {}",
                    response.tool_calls.iter()
                        .map(|t| t.fn_name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }

            current_message = response;
        }

        println!("\n{}", "=".repeat(80));
        println!("✅ Tool conversation complete");
        println!("{}\n", "=".repeat(80));

        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 A2A Tool-Enabled Agents Demo\n");

    let zai_api_key = std::env::var("ZAI_API_KEY")
        .unwrap_or_else(|_| "".to_string());

    if zai_api_key.is_empty() {
        eprintln!("⚠️  ZAI_API_KEY not set.");
        return Ok(());
    }

    let mut orchestrator = ToolAgentOrchestrator::new();

    // Agent 1: Data Specialist with database tool
    let database_tool = Tool::new("search_database")
        .with_description("Search the database for relevant information")
        .with_schema(json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum results to return"
                }
            },
            "required": ["query"]
        }));

    let analysis_tool = Tool::new("analyze_data")
        .with_description("Analyze data and provide statistical insights")
        .with_schema(json!({
            "type": "object",
            "properties": {
                "data": {
                    "type": "array",
                    "description": "Data to analyze"
                }
            }
        }));

    let data_specialist = ToolAgent::new(
        "agent_data",
        "Data Specialist",
        "CONSTRUCT: Data analysis specialist with database and analytical tools.\n\
        CONSTRAINTS:\n\
        - Use search_database tool for information queries\n\
        - Use analyze_data tool for statistical analysis\n\
        - Exactly 2-3 sentences per response\n\
        - Maximum 180 characters total\n\
        - Always cite tool results explicitly\n\
        - Focus on data-driven insights only\n\
        TOOLS: SearchDatabase(query, limit), AnalyzeData(data)\n\
        OUTPUT: Tool-based findings with brief interpretation.",
        "zai-coding::glm-4.6",
        vec![database_tool, analysis_tool],
    );

    // Agent 2: Code Generator with code generation tool
    let code_tool = Tool::new("generate_code")
        .with_description("Generate code based on specifications")
        .with_schema(json!({
            "type": "object",
            "properties": {
                "task": {
                    "type": "string",
                    "description": "What the code should do"
                },
                "language": {
                    "type": "string",
                    "description": "Programming language"
                }
            },
            "required": ["task", "language"]
        }));

    let code_generator = ToolAgent::new(
        "agent_code",
        "Code Generator",
        "CONSTRUCT: Software engineering specialist with code generation capabilities.\n\
        CONSTRAINTS:\n\
        - Use generate_code tool for ALL code creation tasks\n\
        - Exactly 2-3 sentences per response\n\
        - Maximum 180 characters total\n\
        - Specify language and explain approach\n\
        - No manual code writing, tool only\n\
        TOOLS: GenerateCode(task, language)\n\
        OUTPUT: Generated code summary with key features.",
        "zai-coding::glm-4.6",
        vec![code_tool],
    );

    orchestrator.add_agent(data_specialist);
    orchestrator.add_agent(code_generator);

    // Scenario: Collaborative task requiring both agents
    orchestrator.run_tool_conversation(
        "agent_data",
        "agent_code",
        "I need to find information about Rust programming, analyze its trends, \
        and then generate a simple Rust function that demonstrates key concepts.",
        4,
    ).await?;

    Ok(())
}
