//! Multi-provider LLM test using genai
//!
//! Demonstrates how to use different AI providers with automatic endpoint routing

use genai::Client;
use genai::chat::{ChatMessage, ChatRequest};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	println!("🚀 Testing genai multi-provider LLM support\n");

	let client = Client::builder().build();

	// Test cases demonstrating different providers
	let test_cases = vec![
		// ZAI (Zhipu AI) - requires ZAI_API_KEY
		("zai-coding::glm-4.6", "ZAI - Coding subscription model"),

		// Ollama (local) - requires Ollama running locally
		("ollama::llama3.2", "Ollama - Local Llama 3.2"),

		// OpenAI - requires OPENAI_API_KEY
		("gpt-4o-mini", "OpenAI - GPT-4o Mini"),

		// Anthropic - requires ANTHROPIC_API_KEY
		("claude-3-5-sonnet-latest", "Anthropic - Claude 3.5 Sonnet"),

		// Groq - requires GROQ_API_KEY
		("groq::llama-3.3-70b-versatile", "Groq - Llama 3.3 70B"),
	];

	let simple_prompt = "Say 'hello' in one word only.";

	for (model_name, description) in test_cases {
		println!("\n{}", "=".repeat(60));
		println!("📝 {}", description);
		println!("🤖 Model: {}", model_name);
		println!("{}", "-".repeat(60));

		let chat_req = ChatRequest::default()
			.with_system("You are a helpful assistant.")
			.append_message(ChatMessage::user(simple_prompt));

		match client.exec_chat(model_name, chat_req, None).await {
			Ok(response) => {
				println!("✅ Success!");
				if let Some(content) = response.first_text() {
					println!("💬 Response: {}", content);
				}
				if response.usage.prompt_tokens.is_some() || response.usage.completion_tokens.is_some() {
					println!(
						"📊 Usage: prompt={} tokens, output={} tokens",
						response.usage.prompt_tokens.unwrap_or(0),
						response.usage.completion_tokens.unwrap_or(0)
					);
				}
			}
			Err(e) => {
				let error_str = e.to_string();
				println!("❌ Error: {}", error_str);

				// Provide helpful hints based on error type
				if error_str.contains("ApiKeyEnvNotFound") {
					if let Some(env_var) = extract_env_var(&error_str) {
						println!("💡 Hint: Set the {} environment variable", env_var);
					}
				} else if error_str.contains("insufficient balance") {
					println!("💡 Hint: This model requires credits or subscription");
				} else if error_str.contains("Connection refused") || error_str.contains("could not connect") {
					println!("💡 Hint: Make sure the local service is running (e.g., Ollama)");
				} else if error_str.contains("401") {
					println!("💡 Hint: Invalid or missing API key");
				} else if error_str.contains("404") {
					println!("💡 Hint: Model not found or not available");
				}
			}
		}
	}

	println!("\n{}", "=".repeat(60));
	println!("\n📚 SUMMARY");
	println!("{}", "-".repeat(60));
	println!("✅ genai supports multiple AI providers with namespace routing");
	println!("🔑 Required environment variables:");
	println!("   - ZAI_API_KEY       (for Zhipu AI models)");
	println!("   - OPENAI_API_KEY    (for OpenAI models)");
	println!("   - ANTHROPIC_API_KEY (for Anthropic models)");
	println!("   - GROQ_API_KEY      (for Groq models)");
	println!("   - Ollama running    (for local models: http://localhost:11434)");
	println!("\n💡 Tip: Set any API key to test that provider!");

	Ok(())
}

fn extract_env_var(error: &str) -> Option<&str> {
	// Extract environment variable name from error message
	if let Some(start) = error.find("env_name: \"") {
		let start = start + 11; // length of 'env_name: "'
		if let Some(end) = error[start..].find('"') {
			return Some(&error[start..start + end]);
		}
	}
	None
}
