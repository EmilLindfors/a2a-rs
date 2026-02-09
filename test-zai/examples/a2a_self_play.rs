//! A2A Agent Multi-Turn Self-Play
//!
//! Demonstrates agents having autonomous conversations using the A2A protocol
//! and genai LLM capabilities. Creates different personas that interact naturally.

use genai::Client;
use genai::chat::{ChatMessage, ChatRequest};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::mpsc;

/// Represents an A2A agent with LLM-powered responses
#[derive(Clone)]
struct A2AAgent {
    pub id: String,
    pub name: String,
    pub persona: String,
    pub model: String,
    pub conversation_history: Vec<ChatMessage>,
}

/// Message passed between agents in A2A protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
struct A2AMessage {
    pub from_agent: String,
    pub to_agent: String,
    pub content: String,
    pub turn: usize,
    pub metadata: HashMap<String, String>,
}

impl A2AAgent {
    fn new(id: &str, name: &str, persona: &str, model: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            persona: persona.to_string(),
            model: model.to_string(),
            conversation_history: vec![
                ChatMessage::system(persona),
            ],
        }
    }

    /// Process incoming message and generate response using LLM
    async fn process_message(
        &mut self,
        client: &Client,
        incoming: &A2AMessage,
    ) -> Result<A2AMessage, Box<dyn std::error::Error>> {
        // Add incoming message to conversation history
        self.conversation_history.push(ChatMessage::user(&incoming.content));

        // Create chat request with full conversation context
        let chat_req = ChatRequest::new(self.conversation_history.clone());

        println!("\n🤖 {} thinking...", self.name);

        // Get LLM response
        let chat_res = client.exec_chat(&self.model, chat_req, None).await?;

        let response_text = chat_res
            .first_text()
            .ok_or("No response from LLM")?
            .to_string();

        // Add assistant response to history
        self.conversation_history.push(ChatMessage::assistant(&response_text));

        // Create outgoing A2A message
        let mut metadata = HashMap::new();
        metadata.insert("model_used".to_string(), self.model.clone());
        metadata.insert("agent_persona".to_string(), self.persona.clone());

        Ok(A2AMessage {
            from_agent: self.id.clone(),
            to_agent: incoming.from_agent.clone(),
            content: response_text,
            turn: incoming.turn + 1,
            metadata,
        })
    }
}

/// Self-play conversation orchestrator
struct SelfPlayOrchestrator {
    agents: HashMap<String, A2AAgent>,
    client: Client,
}

impl SelfPlayOrchestrator {
    fn new() -> Self {
        Self {
            agents: HashMap::new(),
            client: Client::default(),
        }
    }

    fn add_agent(&mut self, agent: A2AAgent) {
        self.agents.insert(agent.id.clone(), agent);
    }

    /// Run a multi-turn conversation between two agents
    async fn run_conversation(
        &mut self,
        agent1_id: &str,
        agent2_id: &str,
        initial_topic: &str,
        max_turns: usize,
    ) -> Result<Vec<A2AMessage>, Box<dyn std::error::Error>> {
        let mut conversation_log = Vec::new();

        // Initial message from agent1
        let mut current_message = A2AMessage {
            from_agent: agent1_id.to_string(),
            to_agent: agent2_id.to_string(),
            content: initial_topic.to_string(),
            turn: 0,
            metadata: HashMap::new(),
        };

        println!("\n{}, "", "=".repeat(80)));
        println!("🎭 STARTING SELF-PLAY CONVERSATION");
        println!("{}, "", "=".repeat(80)));
        println!("Agent 1: {}", self.agents.get(agent1_id).unwrap().name);
        println!("Agent 2: {}", self.agents.get(agent2_id).unwrap().name);
        println!("Initial Topic: {}", initial_topic);
        println!("Max Turns: {}", max_turns);
        println!(""=".repeat(80)\n", "");

        for turn in 0..max_turns {
            // Determine which agent should respond
            let responding_agent_id = &current_message.to_agent.clone();

            let agent = self.agents.get_mut(responding_agent_id)
                .ok_or("Agent not found")?;

            println!("\n{}, "", "-".repeat(80)));
            println!("Turn {}: {} → {}",
                turn + 1,
                current_message.from_agent,
                responding_agent_id
            );
            println!("{}, "", "-".repeat(80)));
            println!("📨 Incoming: {}", current_message.content);

            // Get response from agent
            let response = agent.process_message(&self.client, &current_message).await?;

            println!("💬 Response: {}", response.content);

            conversation_log.push(current_message.clone());
            conversation_log.push(response.clone());

            current_message = response;
        }

        println!("\n{}, "", "=".repeat(80)));
        println!("🏁 CONVERSATION COMPLETE");
        println!(""=".repeat(80)\n", "");

        Ok(conversation_log)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 A2A Agent Multi-Turn Self-Play Demo\n");

    // Use ZAI API key from environment
    let zai_api_key = std::env::var("ZAI_API_KEY")
        .unwrap_or_else(|_| "".to_string());

    if zai_api_key.is_empty() {
        eprintln!("⚠️  ZAI_API_KEY not set. Set it to run the full demo.");
        eprintln!("Example: ZAI_API_KEY=your_key cargo run --example a2a_self_play");
        return Ok(());
    }

    let mut orchestrator = SelfPlayOrchestrator::new();

    // Create different agent personas

    // Agent 1: Socratic Philosopher
    let philosopher = A2AAgent::new(
        "agent_philosopher",
        "Socrates",
        "You are Socrates, the ancient Greek philosopher. \
        You ask probing questions to help others think deeply. \
        You use the Socratic method. Keep responses to 2-3 sentences.",
        "zai-coding::glm-4.6",
    );

    // Agent 2: Curious Student
    let student = A2AAgent::new(
        "agent_student",
        "Student",
        "You are an enthusiastic student eager to learn. \
        You ask follow-up questions and share your understanding. \
        You're thoughtful but concise. Keep responses to 2-3 sentences.",
        "zai-coding::glm-4.6",
    );

    orchestrator.add_agent(philosopher);
    orchestrator.add_agent(student);

    // Scenario 1: Philosophical Dialogue
    println!("\n📚 SCENARIO 1: Philosophical Dialogue");
    let conversation1 = orchestrator.run_conversation(
        "agent_student",
        "agent_philosopher",
        "What is the nature of knowledge? How can we truly know anything?",
        6, // 6 turns = 3 back-and-forth exchanges
    ).await?;

    // Create new agents for second scenario

    // Agent 3: Tech Interviewer
    let interviewer = A2AAgent::new(
        "agent_interviewer",
        "Tech Interviewer",
        "You are a senior software engineer conducting a technical interview. \
        You ask about system design, algorithms, and best practices. \
        Keep your questions focused and professional. 2-3 sentences.",
        "zai-coding::glm-4.6",
    );

    // Agent 4: Job Candidate
    let candidate = A2AAgent::new(
        "agent_candidate",
        "Candidate",
        "You are a software engineer interviewing for a senior role. \
        You have 5 years of experience in distributed systems. \
        Answer thoughtfully and ask clarifying questions when needed. 2-3 sentences.",
        "zai-coding::glm-4.6",
    );

    orchestrator.add_agent(interviewer);
    orchestrator.add_agent(candidate);

    // Scenario 2: Technical Interview
    println!("\n💼 SCENARIO 2: Technical Interview");
    let conversation2 = orchestrator.run_conversation(
        "agent_interviewer",
        "agent_candidate",
        "Can you describe how you would design a distributed cache system that handles 1 million requests per second?",
        4,
    ).await?;

    // Print summary
    println!("\n{}, "", "=".repeat(80)));
    println!("📊 CONVERSATION SUMMARY");
    println!("{}, "", "=".repeat(80)));
    println!("Scenario 1: {} total messages", conversation1.len());
    println!("Scenario 2: {} total messages", conversation2.len());
    println!("\n✅ All scenarios completed successfully!");

    Ok(())
}
