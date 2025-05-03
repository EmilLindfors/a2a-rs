use crate::reimbursement_agent::task_manager::AgentTaskManager;
use a2a_rs::adapter::server::{DefaultRequestProcessor, HttpServer, WebSocketServer};
use a2a_rs::domain::AgentCard;

/// A2A Server implementation that combines both HTTP and WebSocket servers
pub struct A2AServer {
    http_server: Option<HttpServer<DefaultRequestProcessor<AgentTaskManager>, SimpleAgentInfo>>,
    ws_server: Option<
        WebSocketServer<
            DefaultRequestProcessor<AgentTaskManager>,
            SimpleAgentInfo,
            AgentTaskManager,
        >,
    >,
    host: String,
    port: u16,
}

/// Simple implementation of AgentInfoProvider
pub struct SimpleAgentInfo {
    card: AgentCard,
}

#[async_trait::async_trait]
impl a2a_rs::port::server::AgentInfoProvider for SimpleAgentInfo {
    async fn get_agent_card(&self) -> Result<AgentCard, a2a_rs::domain::A2AError> {
        Ok(self.card.clone())
    }
}

impl A2AServer {
    /// Create a new A2A server with the given agent card and task manager
    pub fn new(
        agent_card: AgentCard,
        task_manager: AgentTaskManager,
        host: String,
        port: u16,
    ) -> Self {
        let _agent_info = SimpleAgentInfo { card: agent_card };

        // Create the request processor
        let _processor = DefaultRequestProcessor::new(task_manager.clone());

        // Create the HTTP server
        #[cfg(feature = "http-server")]
        let http_server = Some(HttpServer::new(
            _processor.clone(),
            _agent_info.clone(),
            format!("{}:{}", host, port),
        ));

        #[cfg(not(feature = "http-server"))]
        let http_server = None;

        // Create the WebSocket server
        #[cfg(feature = "ws-server")]
        let ws_server = Some(WebSocketServer::new(
            _processor,
            _agent_info,
            task_manager,
            format!("{}:{}", host, port),
        ));

        #[cfg(not(feature = "ws-server"))]
        let ws_server = None;

        Self {
            http_server,
            ws_server,
            host,
            port,
        }
    }

    /// Start the server
    pub async fn start(&self) -> Result<(), Box<dyn std::error::Error>> {
        println!("Starting A2A server on {}:{}", self.host, self.port);

        // Start the appropriate server based on which one is available
        if let Some(http_server) = &self.http_server {
            println!("Using HTTP server");
            http_server.start().await?;
        } else if let Some(ws_server) = &self.ws_server {
            println!("Using WebSocket server");
            ws_server.start().await?;
        } else {
            return Err("No server implementation available. Enable either http-server or ws-server feature.".into());
        }

        Ok(())
    }
}

impl Clone for SimpleAgentInfo {
    fn clone(&self) -> Self {
        Self {
            card: self.card.clone(),
        }
    }
}
