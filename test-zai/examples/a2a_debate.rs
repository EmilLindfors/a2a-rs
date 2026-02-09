//! A2A Agent Debate System
//!
//! Two agents debate a topic with a moderator agent managing the conversation

use genai::Client;
use genai::chat::{ChatMessage, ChatRequest};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone)]
struct DebateAgent {
    id: String,
    name: String,
    position: String,
    persona: String,
    model: String,
    conversation_history: Vec<ChatMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DebateMessage {
    from_agent: String,
    to_agent: Option<String>,  // None = broadcast to all
    content: String,
    turn: usize,
    message_type: MessageType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum MessageType {
    Opening,
    Argument,
    Rebuttal,
    Question,
    Answer,
    Closing,
    Moderation,
}

impl DebateAgent {
    fn new(id: &str, name: &str, position: &str, persona: &str, model: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            position: position.to_string(),
            persona: persona.to_string(),
            model: model.to_string(),
            conversation_history: vec![ChatMessage::system(persona)],
        }
    }

    async fn respond(
        &mut self,
        client: &Client,
        incoming: &DebateMessage,
        context: &str,
    ) -> Result<DebateMessage, Box<dyn std::error::Error>> {
        // Build context-aware prompt
        let prompt = format!("{}\n\nContext: {}", incoming.content, context);

        self.conversation_history.push(ChatMessage::user(&prompt));

        let chat_req = ChatRequest::new(self.conversation_history.clone());
        let chat_res = client.exec_chat(&self.model, chat_req, None).await?;

        let response_text = chat_res
            .first_text()
            .ok_or("No response")?
            .to_string();

        self.conversation_history.push(ChatMessage::assistant(&response_text));

        Ok(DebateMessage {
            from_agent: self.id.clone(),
            to_agent: incoming.from_agent.clone().into(),
            content: response_text,
            turn: incoming.turn + 1,
            message_type: MessageType::Argument,
        })
    }
}

struct DebateOrchestrator {
    agents: HashMap<String, DebateAgent>,
    moderator: DebateAgent,
    client: Client,
    transcript: Vec<DebateMessage>,
}

impl DebateOrchestrator {
    fn new(moderator_model: &str) -> Self {
        let moderator = DebateAgent::new(
            "moderator",
            "Moderator",
            "Neutral",
            "CONSTRUCT: Impartial debate moderator with bounded facilitation.\n\
            CONSTRAINTS:\n\
            - Exactly 2-3 sentences per intervention\n\
            - Maximum 200 characters total\n\
            - Maintain strict neutrality\n\
            - Ensure equal speaking time\n\
            - Summarize key arguments only\n\
            - No personal opinions expressed\n\
            OUTPUT: Procedural guidance or balanced summary.",
            moderator_model,
        );

        Self {
            agents: HashMap::new(),
            moderator,
            client: Client::default(),
            transcript: Vec::new(),
        }
    }

    fn add_debater(&mut self, agent: DebateAgent) {
        self.agents.insert(agent.id.clone(), agent);
    }

    async fn run_debate(
        &mut self,
        topic: &str,
        rounds: usize,
    ) -> Result<(), Box<dyn std::error::Error>> {
        println!("\n{}", "=".repeat(80));
        println!("⚖️  A2A AGENT DEBATE");
        println!("{}", "=".repeat(80));
        println!("Topic: {}", topic);
        println!("Rounds: {}", rounds);

        // Introduce debaters
        for (_, agent) in &self.agents {
            println!("\n🎯 {}: {}", agent.name, agent.position);
        }
        println!("{}\n", "=".repeat(80));

        // Opening statements
        println!("📢 OPENING STATEMENTS\n");
        for (id, agent) in &mut self.agents {
            let opening = DebateMessage {
                from_agent: "moderator".to_string(),
                to_agent: Some(id.clone()),
                content: format!("Please provide your opening statement on: {}", topic),
                turn: 0,
                message_type: MessageType::Opening,
            };

            println!("{}", "-".repeat(80));
            println!("🎤 {} ({})", agent.name, agent.position);
            println!("{}", "-".repeat(80));

            let response = agent.respond(&self.client, &opening, "Opening statement").await?;
            println!("{}", response.content);
            self.transcript.push(response);
        }

        // Debate rounds
        let agent_ids: Vec<String> = self.agents.keys().cloned().collect();

        for round in 0..rounds {
            println!("\n{}", "=".repeat(80));
            println!("🔄 ROUND {}", round + 1);
            println!("{}\n", "=".repeat(80));

            for (idx, agent_id) in agent_ids.iter().enumerate() {
                let opponent_id = &agent_ids[(idx + 1) % agent_ids.len()];

                // Get opponent's last argument
                let context = self.transcript.last()
                    .map(|m| m.content.clone())
                    .unwrap_or_else(|| "Begin your argument.".to_string());

                let agent = self.agents.get_mut(agent_id).unwrap();

                println!("{}", "-".repeat(80));
                println!("💭 {} responds", agent.name);
                println!("{}", "-".repeat(80));

                let prompt = DebateMessage {
                    from_agent: opponent_id.clone(),
                    to_agent: Some(agent_id.clone()),
                    content: format!("Present your argument considering the opponent's points."),
                    turn: round * 2 + idx,
                    message_type: MessageType::Argument,
                };

                let response = agent.respond(&self.client, &prompt, &context).await?;
                println!("{}", response.content);
                self.transcript.push(response.clone());

                // Brief pause between speakers
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            }
        }

        // Moderator summary
        println!("\n{}", "=".repeat(80));
        println!("📋 MODERATOR SUMMARY");
        println!("{}\n", "=".repeat(80));

        let full_debate = self.transcript.iter()
            .map(|m| format!("{}: {}", m.from_agent, m.content))
            .collect::<Vec<_>>()
            .join("\n\n");

        let summary_prompt = DebateMessage {
            from_agent: "system".to_string(),
            to_agent: Some("moderator".to_string()),
            content: format!("Provide a balanced summary of this debate, highlighting key arguments from both sides."),
            turn: 999,
            message_type: MessageType::Moderation,
        };

        let summary = self.moderator.respond(&self.client, &summary_prompt, &full_debate).await?;
        println!("{}", summary.content);

        println!("\n{}", "=".repeat(80));
        println!("✅ Debate complete! Total exchanges: {}", self.transcript.len());
        println!("{}\n", "=".repeat(80));

        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 A2A Agent Debate System\n");

    let zai_api_key = std::env::var("ZAI_API_KEY")
        .unwrap_or_else(|_| "".to_string());

    if zai_api_key.is_empty() {
        eprintln!("⚠️  ZAI_API_KEY not set.");
        return Ok(());
    }

    let mut orchestrator = DebateOrchestrator::new("zai-coding::glm-4.6");

    // Debater 1: Pro-AI
    let pro_ai = DebateAgent::new(
        "debater_pro",
        "AI Optimist",
        "Pro-AI Development",
        "CONSTRUCT: Pro-AI development advocate with bounded argumentation.\n\
        CONSTRAINTS:\n\
        - Exactly 3-4 sentences per argument\n\
        - Maximum 250 characters total\n\
        - Emphasize benefits and opportunities\n\
        - Reference human oversight mechanisms\n\
        - Remain respectful and evidence-based\n\
        - Address opponent's concerns directly\n\
        OUTPUT: Persuasive argument supporting rapid AI development.",
        "zai-coding::glm-4.6",
    );

    // Debater 2: Cautious approach
    let cautious = DebateAgent::new(
        "debater_cautious",
        "AI Realist",
        "Cautious AI Development",
        "CONSTRUCT: Cautious AI development advocate with bounded argumentation.\n\
        CONSTRAINTS:\n\
        - Exactly 3-4 sentences per argument\n\
        - Maximum 250 characters total\n\
        - Emphasize risks and ethical concerns\n\
        - Reference need for regulatory safeguards\n\
        - Remain respectful and evidence-based\n\
        - Address opponent's claims directly\n\
        OUTPUT: Persuasive argument supporting measured AI development.",
        "zai-coding::glm-4.6",
    );

    orchestrator.add_debater(pro_ai);
    orchestrator.add_debater(cautious);

    // Run debate
    orchestrator.run_debate(
        "Should AI development proceed at maximum speed or should we implement significant regulatory safeguards first?",
        3, // 3 rounds
    ).await?;

    Ok(())
}
