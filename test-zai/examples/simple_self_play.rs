//! Simple A2A Agent Multi-Turn Self-Play
//!
//! Two LLM-powered agents have a conversation using A2A-style message passing

use genai::Client;
use genai::chat::{ChatMessage, ChatRequest};

#[derive(Clone)]
struct Agent {
    name: String,
    persona: String,
    model: String,
    history: Vec<ChatMessage>,
}

impl Agent {
    fn new(name: &str, persona: &str, model: &str) -> Self {
        Self {
            name: name.to_string(),
            persona: persona.to_string(),
            model: model.to_string(),
            history: vec![ChatMessage::system(persona)],
        }
    }

    async fn respond(
        &mut self,
        client: &Client,
        message: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        // Add user message
        self.history.push(ChatMessage::user(message));

        // Get LLM response
        let chat_req = ChatRequest::new(self.history.clone());
        let chat_res = client.exec_chat(&self.model, chat_req, None).await?;

        let response = chat_res.first_text().ok_or("No response")?.to_string();

        // Add assistant response to history
        self.history.push(ChatMessage::assistant(&response));

        Ok(response)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n🎭 A2A AGENT SELF-PLAY CONVERSATION\n");

    let zai_key = std::env::var("ZAI_API_KEY").unwrap_or_default();
    if zai_key.is_empty() {
        eprintln!("⚠️  Set ZAI_API_KEY to run this example");
        return Ok(());
    }

    let client = Client::default();

    // Create two agents with different personas
    let mut socrates = Agent::new(
        "Socrates",
        "You are Socrates. Ask probing questions. Keep responses to 2-3 sentences.",
        "zai-coding::glm-4.6",
    );

    let mut student = Agent::new(
        "Student",
        "You are a curious student. Ask questions and share your thoughts. Keep responses to 2-3 sentences.",
        "zai-coding::glm-4.6",
    );

    // Start conversation
    let topic = "What is the nature of knowledge?";
    println!("Topic: {}\n", topic);
    println!("{}", "=".repeat(70));

    let mut current_message = topic.to_string();
    let mut current_speaker = "Student";

    // Multi-turn conversation
    for turn in 0..6 {
        println!("\n[Turn {}] {} → {}",
            turn + 1,
            current_speaker,
            if current_speaker == "Student" { "Socrates" } else { "Student" }
        );
        println!("{}", "-".repeat(70));
        println!("📨 {}", current_message);
        println!();

        // Get response from the appropriate agent
        let (responder, responder_name) = if current_speaker == "Student" {
            (&mut socrates, "Socrates")
        } else {
            (&mut student, "Student")
        };

        print!("🤖 {} thinking... ", responder_name);
        let response = responder.respond(&client, &current_message).await?;
        println!("done!");
        println!("💬 {}", response);

        // Update for next turn
        current_message = response;
        current_speaker = responder_name;
    }

    println!("\n{}", "=".repeat(70));
    println!("✅ Conversation complete!\n");

    Ok(())
}
