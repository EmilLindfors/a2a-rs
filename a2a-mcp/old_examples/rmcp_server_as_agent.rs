//! Example showing how to expose RMCP tools as an A2A agent

use a2a_mcp::{RmcpA2aServer, ToolToAgentAdapter};
use rmcp::{ServerJsonRpcMessage, ToolCall, ToolResponse};
use std::net::SocketAddr;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create RMCP server with tools
    let rmcp_server = create_rmcp_server();
    
    // List available tools
    let tools = rmcp_server.list_tools();
    println!("Available RMCP tools: {:?}", tools);
    
    // Create adapter to expose tools as A2A agent
    let adapter = ToolToAgentAdapter::new(
        tools,
        "RMCP Tool Agent".to_string(),
        "An agent that provides access to various RMCP tools".to_string()
    );
    
    // Create integrated server
    let server = RmcpA2aServer::new(rmcp_server, adapter);
    
    // Start server on localhost:3000
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!("Starting A2A agent server on http://{}", addr);
    println!("Agent card available at: http://{}/agent-card", addr);
    
    server.serve(addr).await?;
    
    Ok(())
}

fn create_rmcp_server() -> rmcp::Server {
    // Create a new RMCP server with some sample tools
    let mut server = rmcp::Server::new();
    
    // Add a simple echo tool
    server.add_tool(rmcp::Tool {
        name: "echo".to_string(),
        description: "Echo back the input".to_string(),
        parameters: None,
    });
    
    // Define echo tool handler
    let (sender, mut receiver) = mpsc::channel(32);
    let sender_clone = sender.clone();
    
    // Tool handler for echo
    tokio::spawn(async move {
        while let Some((call, respond)) = receiver.recv().await {
            if call.method == "echo" {
                let result = call.params.clone();
                let response = ToolResponse { result };
                let _ = respond.send(Ok(response));
            }
        }
    });
    
    // Add a calculator tool
    server.add_tool(rmcp::Tool {
        name: "calculator".to_string(),
        description: "Perform basic calculations".to_string(),
        parameters: None,
    });
    
    // Calculator tool handler
    tokio::spawn(async move {
        while let Some((call, respond)) = receiver.recv().await {
            if call.method == "calculator" {
                let result = match call.params.get("operation") {
                    Some(serde_json::Value::String(op)) => {
                        let a = call.params.get("a").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        let b = call.params.get("b").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        
                        match op.as_str() {
                            "add" => a + b,
                            "subtract" => a - b,
                            "multiply" => a * b,
                            "divide" => {
                                if b == 0.0 {
                                    return let _ = respond.send(Err(ServerJsonRpcMessage {
                                        jsonrpc: "2.0".to_string(),
                                        id: None,
                                        result: None,
                                        error: Some(rmcp::ErrorResponse {
                                            code: -32000,
                                            message: "Division by zero".to_string(),
                                            data: None,
                                        }),
                                    }));
                                }
                                a / b
                            },
                            _ => {
                                return let _ = respond.send(Err(ServerJsonRpcMessage {
                                    jsonrpc: "2.0".to_string(),
                                    id: None,
                                    result: None,
                                    error: Some(rmcp::ErrorResponse {
                                        code: -32000,
                                        message: format!("Unknown operation: {}", op),
                                        data: None,
                                    }),
                                }));
                            }
                        }
                    },
                    _ => {
                        return let _ = respond.send(Err(ServerJsonRpcMessage {
                            jsonrpc: "2.0".to_string(),
                            id: None,
                            result: None,
                            error: Some(rmcp::ErrorResponse {
                                code: -32602,
                                message: "Invalid parameters".to_string(),
                                data: None,
                            }),
                        }));
                    }
                };
                
                let response = ToolResponse { 
                    result: serde_json::json!({ "result": result }) 
                };
                let _ = respond.send(Ok(response));
            }
        }
    });
    
    // Set the tool handler
    server.set_handler(Box::new(move |call| {
        let sender = sender_clone.clone();
        Box::pin(async move {
            let (response_tx, response_rx) = tokio::sync::oneshot::channel();
            if sender.send((call, response_tx)).await.is_err() {
                return Err(ServerJsonRpcMessage {
                    jsonrpc: "2.0".to_string(),
                    id: None,
                    result: None,
                    error: Some(rmcp::ErrorResponse {
                        code: -32603,
                        message: "Internal error".to_string(),
                        data: None,
                    }),
                });
            }
            
            response_rx.await.unwrap_or_else(|_| {
                Err(ServerJsonRpcMessage {
                    jsonrpc: "2.0".to_string(),
                    id: None,
                    result: None,
                    error: Some(rmcp::ErrorResponse {
                        code: -32603,
                        message: "Internal error".to_string(),
                        data: None,
                    }),
                })
            })
        })
    }));
    
    server
}