use a2a_rs::adapter::{
    business::DefaultBusinessHandler, DefaultRequestProcessor, HttpServer, WebSocketServer,
    InMemoryTaskStorage, NoopPushNotificationSender, SimpleAgentInfo,
};
// use a2a_rs::domain::AgentCapabilities; // For future use

/// Modern A2A server setup using the framework's DefaultBusinessHandler
pub struct ModernReimbursementServer {
    host: String,
    port: u16,
}

impl ModernReimbursementServer {
    /// Create a new modern reimbursement server
    pub fn new(host: String, port: u16) -> Self {
        Self {
            host,
            port,
        }
    }

    /// Start the HTTP server
    pub async fn start_http(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Create server components using framework architecture
        let push_sender = NoopPushNotificationSender;
        let storage = InMemoryTaskStorage::with_push_sender(push_sender);
        let handler = DefaultBusinessHandler::with_storage(storage);
        let processor = DefaultRequestProcessor::with_handler(handler);

        // Create agent info with reimbursement capabilities
        let agent_info = SimpleAgentInfo::new(
            "Reimbursement Agent".to_string(),
            format!("http://{}:{}", self.host, self.port),
        )
        .with_description("An intelligent agent that handles employee reimbursement requests, from form generation to approval processing.".to_string())
        .with_provider(
            "Example Organization".to_string(),
            "https://example.org".to_string(),
        )
        .with_documentation_url("https://example.org/docs/reimbursement-agent".to_string())
        .with_streaming()
        .add_comprehensive_skill(
            "process_reimbursement".to_string(),
            "Process Reimbursement".to_string(),
            Some("Helps with the reimbursement process for users given the amount and purpose of the reimbursement. Generates forms, validates submissions, and processes approvals.".to_string()),
            Some(vec![
                "reimbursement".to_string(),
                "expense".to_string(),
                "finance".to_string(),
                "forms".to_string(),
            ]),
            Some(vec![
                "Can you reimburse me $20 for my lunch with the clients?".to_string(),
                "I need to submit a reimbursement for $150 for office supplies".to_string(),
                "Process my travel expense of $500 for the conference".to_string(),
            ]),
            Some(vec!["text".to_string(), "data".to_string()]),
            Some(vec!["text".to_string(), "data".to_string()]),
        );

        // Create HTTP server
        let server = HttpServer::new(
            processor,
            agent_info,
            format!("{}:{}", self.host, self.port),
        );

        println!("🌐 Starting HTTP reimbursement server on {}:{}", self.host, self.port);
        println!("📋 Agent card: http://{}:{}/agent-card", self.host, self.port);
        println!("🛠️  Skills: http://{}:{}/skills", self.host, self.port);
        
        server.start().await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }

    /// Start the WebSocket server
    pub async fn start_websocket(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Create server components using framework architecture
        let push_sender = NoopPushNotificationSender;
        let storage = InMemoryTaskStorage::with_push_sender(push_sender);
        let handler = DefaultBusinessHandler::with_storage(storage);
        let processor = DefaultRequestProcessor::with_handler(handler.clone());

        // Create agent info (same as HTTP)
        let agent_info = SimpleAgentInfo::new(
            "Reimbursement Agent".to_string(),
            format!("ws://{}:{}", self.host, self.port + 1),
        )
        .with_description("WebSocket interface for the reimbursement agent with streaming support.".to_string())
        .with_streaming()
        .add_comprehensive_skill(
            "process_reimbursement".to_string(),
            "Process Reimbursement".to_string(),
            Some("Real-time reimbursement processing with streaming updates.".to_string()),
            Some(vec!["reimbursement".to_string(), "streaming".to_string()]),
            Some(vec!["Can you reimburse me $20 for my lunch?".to_string()]),
            Some(vec!["text".to_string()]),
            Some(vec!["text".to_string()]),
        );

        // Create WebSocket server
        let server = WebSocketServer::new(
            processor,
            agent_info,
            handler,
            format!("{}:{}", self.host, self.port + 1),
        );

        println!("🔌 Starting WebSocket reimbursement server on {}:{}", self.host, self.port + 1);
        println!("📡 WebSocket endpoint: ws://{}:{}", self.host, self.port + 1);
        
        server.start().await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }

    /// Start both HTTP and WebSocket servers (simplified for now)
    pub async fn start_all(&self) -> Result<(), Box<dyn std::error::Error>> {
        println!("🚀 Starting modern reimbursement agent...");
        println!("Note: Starting HTTP server only for now. WebSocket support coming soon.");
        self.start_http().await
    }
}