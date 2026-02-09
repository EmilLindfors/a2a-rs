//! A2A Agents with Tool Calling
//!
//! Demonstrates agents calling tools during their conversation

use genai::Client;
use genai::chat::{ChatMessage, ChatRequest, Tool, ToolResponse};
use serde_json::json;

#[derive(Clone)]
struct ToolAgent {
    name: String,
    persona: String,
    model: String,
    tools: Vec<Tool>,
    history: Vec<ChatMessage>,
}

impl ToolAgent {
    fn new(name: &str, persona: &str, model: &str, tools: Vec<Tool>) -> Self {
        Self {
            name: name.to_string(),
            persona: persona.to_string(),
            model: model.to_string(),
            tools,
            history: vec![ChatMessage::system(persona)],
        }
    }

    async fn respond_with_tools(
        &mut self,
        client: &Client,
        message: &str,
    ) -> Result<(String, Vec<String>), Box<dyn std::error::Error>> {
        self.history.push(ChatMessage::user(message));

        let chat_req = ChatRequest::new(self.history.clone())
            .with_tools(self.tools.clone());

        println!("   🔧 {} has {} tools available", self.name, self.tools.len());

        let chat_res = client.exec_chat(&self.model, chat_req, None).await?;

        let tool_calls = chat_res.clone().into_tool_calls();
        let mut tools_used = Vec::new();

        if !tool_calls.is_empty() {
            println!("   📞 Calling {} tool(s)...", tool_calls.len());

            for tool_call in &tool_calls {
                println!("      • {}({})", tool_call.fn_name, tool_call.fn_arguments);
                tools_used.push(tool_call.fn_name.clone());

                // Simulate tool execution
                let result = match tool_call.fn_name.as_str() {
                    "calculate" => {
                        let args = &tool_call.fn_arguments;
                        let a = args.get("a").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        let b = args.get("b").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        let op = args.get("operation").and_then(|v| v.as_str()).unwrap_or("add");

                        let result = match op {
                            "add" => a + b,
                            "multiply" => a * b,
                            "subtract" => a - b,
                            "divide" if b != 0.0 => a / b,
                            _ => 0.0,
                        };

                        json!({"result": result})
                    }
                    "search" => {
                        json!({"results": ["Result 1", "Result 2", "Result 3"], "count": 3})
                    }
                    _ => json!({"error": "Unknown tool"}),
                };

                let response = ToolResponse::new(tool_call.call_id.clone(), result.to_string());

                self.history.push(ChatMessage::from(tool_calls.clone()));
                self.history.push(ChatMessage::from(response));
            }

            // Get final response with tool results
            let final_req = ChatRequest::new(self.history.clone());
            let final_res = client.exec_chat(&self.model, final_req, None).await?;
            let final_text = final_res.first_text().unwrap_or("").to_string();
            self.history.push(ChatMessage::assistant(&final_text));

            Ok((final_text, tools_used))
        } else {
            let response_text = chat_res.first_text().unwrap_or("").to_string();
            self.history.push(ChatMessage::assistant(&response_text));
            Ok((response_text, tools_used))
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n🛠️  A2A AGENTS WITH TOOL CALLING\n");

    let zai_key = std::env::var("ZAI_API_KEY").unwrap_or_default();
    if zai_key.is_empty() {
        eprintln!("⚠️  Set ZAI_API_KEY to run this example");
        return Ok(());
    }

    let client = Client::default();

    // Create calculator tool
    let calc_tool = Tool::new("calculate")
        .with_description("Perform mathematical calculations")
        .with_schema(json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": ["add", "subtract", "multiply", "divide"]
                },
                "a": {"type": "number"},
                "b": {"type": "number"}
            },
            "required": ["operation", "a", "b"]
        }));

    // Create search tool
    let search_tool = Tool::new("search")
        .with_description("Search for information")
        .with_schema(json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"}
            },
            "required": ["query"]
        }));

    // Agent with calculator
    let mut math_agent = ToolAgent::new(
        "MathAgent",
        "You are a helpful math assistant. Use the calculator tool when needed. Keep responses to 2-3 sentences.",
        "zai-coding::glm-4.6",
        vec![calc_tool],
    );

    // Agent with search
    let mut researcher = ToolAgent::new(
        "Researcher",
        "You are a research assistant. Use the search tool when needed. Keep responses to 2-3 sentences.",
        "zai-coding::glm-4.6",
        vec![search_tool],
    );

    println!("{}", "=".repeat(70));
    println!("\n🧮 MATH AGENT SCENARIO\n");

    let math_query = "What is 42 multiplied by 17?";
    println!("Query: {}\n", math_query);

    print!("🤖 MathAgent thinking... ");
    let (response, tools) = math_agent.respond_with_tools(&client, math_query).await?;
    println!("done!");
    println!("💬 {}", response);
    if !tools.is_empty() {
        println!("   ✅ Tools used: {}", tools.join(", "));
    }

    println!("\n{}", "=".repeat(70));
    println!("\n🔍 RESEARCHER SCENARIO\n");

    let research_query = "Find information about Rust programming language";
    println!("Query: {}\n", research_query);

    print!("🤖 Researcher thinking... ");
    let (response, tools) = researcher.respond_with_tools(&client, research_query).await?;
    println!("done!");
    println!("💬 {}", response);
    if !tools.is_empty() {
        println!("   ✅ Tools used: {}", tools.join(", "));
    }

    println!("\n{}", "=".repeat(70));
    println!("✅ Tool calling demonstration complete!\n");

    Ok(())
}
