use clap::Parser;
use a2a_rs::domain::{AgentCapabilities, AgentCard, AgentSkill};
use a2a_agents::reimbursement_agent::agent::ReimbursementAgent;
use a2a_agents::reimbursement_agent::task_manager::AgentTaskManager;
use a2a_agents::reimbursement_agent::server::A2AServer;

/// Command-line arguments for the reimbursement server
#[derive(Parser, Debug)]
#[clap(author, version, about)]
struct Args {
    /// Host to bind the server to
    #[clap(long, default_value = "localhost")]
    host: String,
    
    /// Port to listen on
    #[clap(long, default_value = "10002")]
    port: u16,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();
    
    // Parse command-line arguments
    let args = Args::parse();
    
    // Create the agent capabilities
    let capabilities = AgentCapabilities {
        streaming: true,
        push_notifications: false,
        state_transition_history: false,
    };
    
    // Create the agent skill
    let skill = AgentSkill {
        id: "process_reimbursement".to_string(),
        name: "Process Reimbursement Tool".to_string(),
        description: Some("Helps with the reimbursement process for users given the amount and purpose of the reimbursement.".to_string()),
        tags: Some(vec!["reimbursement".to_string()]),
        examples: Some(vec!["Can you reimburse me $20 for my lunch with the clients?".to_string()]),
        input_modes: None,
        output_modes: None,
    };
    
    // Create the agent card
    let agent_card = AgentCard {
        name: "Reimbursement Agent".to_string(),
        description: Some("This agent handles the reimbursement process for the employees given the amount and purpose of the reimbursement.".to_string()),
        url: format!("http://{}:{}/", args.host, args.port),
        provider: None,
        version: "1.0.0".to_string(),
        documentation_url: None,
        capabilities,
        authentication: None,
        default_input_modes: ReimbursementAgent::SUPPORTED_CONTENT_TYPES.iter().map(|&s| s.to_string()).collect(),
        default_output_modes: ReimbursementAgent::SUPPORTED_CONTENT_TYPES.iter().map(|&s| s.to_string()).collect(),
        skills: vec![skill],
    };
    
    // Create the reimbursement agent
    let agent = ReimbursementAgent::new();
    
    // Create the task manager
    let task_manager = AgentTaskManager::new(agent);
    
    // Create the server
    let server = A2AServer::new(
        agent_card,
        task_manager,
        args.host,
        args.port,
    );
    
    // Start the server
    server.start().await?;
    
    Ok(())
}
